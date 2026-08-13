//! System audio capture through Core Audio process taps (macOS 14.4+).
//!
//! A tap is attached to an aggregate device that carries no output of its own, so the
//! audio the user hears is untouched. Unlike ScreenCaptureKit this asks for audio
//! recording rather than screen recording, and needs no window enumeration, so a capture
//! starts in milliseconds.

use super::pcm::PcmConverter;
use super::TARGET_SAMPLE_RATE;
use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::AnyThread;
use objc2_core_audio::{
    kAudioAggregateDeviceIsPrivateKey, kAudioAggregateDeviceIsStackedKey,
    kAudioAggregateDeviceMainSubDeviceKey, kAudioAggregateDeviceNameKey,
    kAudioAggregateDeviceSubDeviceListKey, kAudioAggregateDeviceTapAutoStartKey,
    kAudioAggregateDeviceTapListKey, kAudioAggregateDeviceUIDKey, kAudioDevicePropertyDeviceUID,
    kAudioDevicePropertyNominalSampleRate, kAudioHardwarePropertyDefaultOutputDevice,
    kAudioHardwarePropertyTranslatePIDToProcessObject, kAudioObjectPropertyElementMain,
    kAudioObjectPropertyScopeGlobal, kAudioObjectSystemObject, kAudioSubDeviceUIDKey,
    kAudioSubTapDriftCompensationKey, kAudioSubTapUIDKey, kAudioTapPropertyFormat,
    AudioDeviceCreateIOProcIDWithBlock, AudioDeviceDestroyIOProcID, AudioDeviceIOProcID,
    AudioDeviceStart, AudioDeviceStop, AudioHardwareCreateAggregateDevice,
    AudioHardwareCreateProcessTap, AudioHardwareDestroyAggregateDevice,
    AudioHardwareDestroyProcessTap, AudioObjectGetPropertyData, AudioObjectID,
    AudioObjectPropertyAddress, AudioObjectPropertySelector, CATapDescription,
};
use objc2_core_audio_types::{AudioBufferList, AudioStreamBasicDescription};
use objc2_core_foundation::{CFDictionary, CFRetained, CFString};
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString, NSUUID};
use std::ffi::CStr;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};

const SYSTEM_OBJECT: AudioObjectID = kAudioObjectSystemObject as AudioObjectID;

pub struct SystemAudioCapture {
    active: Option<Active>,
}

/// Everything the OS handed out, in the order it has to be handed back.
struct Active {
    aggregate: AudioObjectID,
    tap: AudioObjectID,
    io_proc: AudioDeviceIOProcID,
    format_watch: FormatWatch,
}

/// The format the tap is currently delivering. Not fixed for the life of the tap: a
/// Bluetooth headset entering headset mode re-rates the device mid-capture, and resampling
/// from the start-time rate would hand the translator half-speed audio.
struct TapFormat(AtomicU64);

impl TapFormat {
    fn new(sample_rate: u32, channels: usize) -> Self {
        Self(AtomicU64::new(packed(sample_rate, channels)))
    }

    // One word, so a callback can never mix the old rate with the new channel count.
    fn get(&self) -> (u32, usize) {
        let packed = self.0.load(Ordering::Relaxed);
        ((packed >> 32) as u32, packed as u32 as usize)
    }

    fn set(&self, sample_rate: u32, channels: usize) {
        self.0
            .store(packed(sample_rate, channels), Ordering::Relaxed);
    }
}

fn packed(sample_rate: u32, channels: usize) -> u64 {
    (u64::from(sample_rate) << 32) | channels as u64
}

/// Stops the polling thread; Core Audio sends no notification when a tap re-rates.
struct FormatWatch(Arc<AtomicBool>);

impl Drop for FormatWatch {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

impl SystemAudioCapture {
    pub fn new() -> Self {
        Self { active: None }
    }

    pub fn start(
        &mut self,
        application_bundle_id: Option<&str>,
    ) -> Result<mpsc::Receiver<Vec<u8>>, String> {
        if self.active.is_some() {
            return Err("Already capturing".to_string());
        }

        let description = tap_description(application_bundle_id)?;
        let uid = unsafe { description.UUID() }.UUIDString().to_string();

        let mut tap: AudioObjectID = 0;
        let status = unsafe { AudioHardwareCreateProcessTap(Some(&description), &mut tap) };
        status_ok(status, "create audio tap")?;

        let started = self.start_tapped(tap, &uid);
        if started.is_err() {
            unsafe { AudioHardwareDestroyProcessTap(tap) };
        }
        started
    }

    /// Split out so a failure anywhere past tap creation still destroys the tap.
    fn start_tapped(
        &mut self,
        tap: AudioObjectID,
        uid: &str,
    ) -> Result<mpsc::Receiver<Vec<u8>>, String> {
        let aggregate = create_aggregate(uid)?;
        // Seeded from the aggregate, like every later read: the tap alone can already be
        // stale here when the output device is in headset mode as the capture starts.
        let Some((sample_rate, channels)) = current_format(tap, aggregate) else {
            unsafe { AudioHardwareDestroyAggregateDevice(aggregate) };
            return Err("Audio tap reported an empty format".into());
        };
        let format = Arc::new(TapFormat::new(sample_rate, channels));
        let converter = PcmConverter::new(sample_rate, channels, TARGET_SAMPLE_RATE)?;
        let (sender, receiver) = mpsc::channel::<Vec<u8>>();
        let block = RcBlock::new({
            let format = format.clone();
            // Mutex only because the block is a `Fn`; the IO thread is the sole taker.
            let converter = Mutex::new(((sample_rate, channels), converter));
            move |_now: NonNull<_>,
                  input: NonNull<AudioBufferList>,
                  _input_time: NonNull<_>,
                  _output: NonNull<AudioBufferList>,
                  _output_time: NonNull<_>| {
                let (sample_rate, channels) = format.get();
                let audio = unsafe { interleaved(input.as_ref(), channels) };
                if audio.is_empty() {
                    return;
                }
                let Ok(mut guard) = converter.lock() else {
                    return;
                };
                if guard.0 != (sample_rate, channels) {
                    match PcmConverter::new(sample_rate, channels, TARGET_SAMPLE_RATE) {
                        Ok(rebuilt) => *guard = ((sample_rate, channels), rebuilt),
                        Err(error) => {
                            crate::diagnostics::log("audio:system", error);
                            return;
                        }
                    }
                }
                let pcm = guard.1.convert_f32(&audio);
                if !pcm.is_empty() {
                    let _ = sender.send(pcm);
                }
            }
        });

        let mut io_proc: AudioDeviceIOProcID = None;
        let status = unsafe {
            AudioDeviceCreateIOProcIDWithBlock(
                NonNull::from(&mut io_proc),
                aggregate,
                None,
                RcBlock::as_ptr(&block),
            )
        };
        if let Err(error) = status_ok(status, "attach audio callback") {
            unsafe { AudioHardwareDestroyAggregateDevice(aggregate) };
            return Err(error);
        }

        let status = unsafe { AudioDeviceStart(aggregate, io_proc) };
        if let Err(error) = status_ok(status, "start audio device") {
            unsafe {
                AudioDeviceDestroyIOProcID(aggregate, io_proc);
                AudioHardwareDestroyAggregateDevice(aggregate);
            }
            return Err(error);
        }

        self.active = Some(Active {
            aggregate,
            tap,
            io_proc,
            format_watch: watch_format(tap, aggregate, format),
        });
        Ok(receiver)
    }

    pub fn stop(&mut self) {
        let Some(active) = self.active.take() else {
            return;
        };
        drop(active.format_watch);
        unsafe {
            AudioDeviceStop(active.aggregate, active.io_proc);
            AudioDeviceDestroyIOProcID(active.aggregate, active.io_proc);
            AudioHardwareDestroyAggregateDevice(active.aggregate);
            AudioHardwareDestroyProcessTap(active.tap);
        }
    }
}

impl Default for SystemAudioCapture {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for SystemAudioCapture {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Fast enough that a device switching mode costs a fraction of a second of misread audio.
const FORMAT_POLL: std::time::Duration = std::time::Duration::from_millis(500);

/// Polls the delivered format for as long as the tap runs; there is no notification to
/// subscribe to.
fn watch_format(
    tap: AudioObjectID,
    aggregate: AudioObjectID,
    format: Arc<TapFormat>,
) -> FormatWatch {
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = stop.clone();
    std::thread::spawn(move || {
        while !stopped.load(Ordering::SeqCst) {
            std::thread::sleep(FORMAT_POLL);
            // The ids are handed back the moment a capture stops.
            if stopped.load(Ordering::SeqCst) {
                break;
            }
            let Some((sample_rate, channels)) = current_format(tap, aggregate) else {
                continue;
            };
            if (sample_rate, channels) == format.get() {
                continue;
            }
            format.set(sample_rate, channels);
            crate::diagnostics::log(
                "audio:system",
                format!("tap_format_changed rate={sample_rate} channels={channels}"),
            );
        }
    });
    FormatWatch(stop)
}

/// The rate frames actually arrive at and the tap's channel count. The rate deliberately
/// comes from the aggregate, not the tap: the IOProc is clocked by the aggregate's nominal
/// rate, and the tap keeps advertising its start-time rate after the device re-rates
/// (observed with AirPods dropping to 24 kHz: the tap still claimed 48 kHz).
fn current_format(tap: AudioObjectID, aggregate: AudioObjectID) -> Option<(u32, usize)> {
    let format: AudioStreamBasicDescription =
        unsafe { property(tap, kAudioTapPropertyFormat, &[]) }.ok()?;
    let channels = format.mChannelsPerFrame as usize;
    let device_rate: f64 =
        unsafe { property(aggregate, kAudioDevicePropertyNominalSampleRate, &[]) }.unwrap_or(0.0);
    let sample_rate = if device_rate > 0.0 {
        device_rate as u32
    } else {
        format.mSampleRate as u32
    };
    (sample_rate != 0 && channels != 0).then_some((sample_rate, channels))
}

/// Whole-system audio minus our own, or exactly one application.
fn tap_description(
    application_bundle_id: Option<&str>,
) -> Result<Retained<CATapDescription>, String> {
    let allocated = CATapDescription::alloc();
    let description = match application_bundle_id {
        Some(bundle_id) => {
            let processes = NSArray::from_retained_slice(&process_objects(bundle_id)?);
            unsafe { CATapDescription::initStereoMixdownOfProcesses(allocated, &processes) }
        }
        None => {
            // Excluding ourselves keeps any audio OverLingo itself plays out of the stream.
            // Having never played any, we usually have no audio object at all, and then
            // there is nothing to exclude.
            let own = process_object(std::process::id() as i32);
            let processes = NSArray::from_retained_slice(&own.into_iter().collect::<Vec<_>>());
            unsafe {
                CATapDescription::initStereoGlobalTapButExcludeProcesses(allocated, &processes)
            }
        }
    };
    unsafe {
        description.setName(&NSString::from_str("OverLingo"));
        description.setUUID(&NSUUID::new());
        // A private tap belongs to this process alone and never appears in Audio MIDI Setup.
        description.setPrivate(true);
    }
    Ok(description)
}

fn process_objects(bundle_id: &str) -> Result<Vec<Retained<NSNumber>>, String> {
    let pids = super::macos_apps::pids_for_bundle(bundle_id);
    if pids.is_empty() {
        return Err(format!("Selected application is not running: {bundle_id}"));
    }
    let objects = pids
        .into_iter()
        .filter_map(process_object)
        .collect::<Vec<_>>();
    if objects.is_empty() {
        return Err(format!(
            "Selected application has not played any audio yet: {bundle_id}"
        ));
    }
    Ok(objects)
}

/// Core Audio addresses processes by its own object id rather than by pid, and only knows
/// about a process once it has actually played something. An unknown one is `None` rather
/// than an error: a tap built around it is refused outright.
fn process_object(pid: i32) -> Option<Retained<NSNumber>> {
    let object: AudioObjectID = unsafe {
        property(
            SYSTEM_OBJECT,
            kAudioHardwarePropertyTranslatePIDToProcessObject,
            &pid.to_ne_bytes(),
        )
    }
    .ok()?;
    (object != 0).then(|| NSNumber::new_u32(object))
}

/// The tap only produces audio once it is a sub-device of a running aggregate. The default
/// output device rides along as the clock source; the aggregate itself stays private and
/// silent, so nothing the user hears changes.
fn create_aggregate(tap_uid: &str) -> Result<AudioObjectID, String> {
    let output: AudioObjectID = unsafe {
        property(
            SYSTEM_OBJECT,
            kAudioHardwarePropertyDefaultOutputDevice,
            &[],
        )
    }?;
    let output_uid = device_uid(output)?;

    let yes = NSNumber::new_bool(true);
    let no = NSNumber::new_bool(false);
    let output_name = NSString::from_str(&output_uid);
    let tap_name = NSString::from_str(tap_uid);
    let own_name = NSString::from_str("OverLingo Capture");
    let own_uid = NSString::from_str(&NSUUID::new().UUIDString().to_string());

    let tap = dictionary(&[
        (kAudioSubTapUIDKey, &tap_name),
        (kAudioSubTapDriftCompensationKey, &yes),
    ]);
    let sub_device = dictionary(&[(kAudioSubDeviceUIDKey, &output_name)]);
    let taps = NSArray::from_retained_slice(&[tap]);
    let sub_devices = NSArray::from_retained_slice(&[sub_device]);
    let description = dictionary(&[
        (kAudioAggregateDeviceNameKey, &own_name),
        (kAudioAggregateDeviceUIDKey, &own_uid),
        (kAudioAggregateDeviceMainSubDeviceKey, &output_name),
        (kAudioAggregateDeviceIsPrivateKey, &yes),
        (kAudioAggregateDeviceIsStackedKey, &no),
        (kAudioAggregateDeviceTapAutoStartKey, &yes),
        (kAudioAggregateDeviceSubDeviceListKey, &sub_devices),
        (kAudioAggregateDeviceTapListKey, &taps),
    ]);

    let mut aggregate: AudioObjectID = 0;
    // NSDictionary and CFDictionary are the same object; Core Audio wants the C name for it.
    let bridged: &CFDictionary = unsafe { &*(&*description as *const NSDictionary<_, _>).cast() };
    let status =
        unsafe { AudioHardwareCreateAggregateDevice(bridged, NonNull::from(&mut aggregate)) };
    status_ok(status, "create aggregate device")?;
    Ok(aggregate)
}

fn dictionary(entries: &[(&CStr, &AnyObject)]) -> Retained<NSDictionary<NSString, AnyObject>> {
    let keys = entries
        .iter()
        .map(|(name, _)| NSString::from_str(&name.to_string_lossy()))
        .collect::<Vec<_>>();
    let keys = keys.iter().map(|key| &**key).collect::<Vec<_>>();
    let values = entries.iter().map(|(_, value)| *value).collect::<Vec<_>>();
    NSDictionary::from_slices(&keys, &values)
}

fn device_uid(device: AudioObjectID) -> Result<String, String> {
    let uid: *const CFString = unsafe { property(device, kAudioDevicePropertyDeviceUID, &[]) }?;
    let uid = NonNull::new(uid.cast_mut()).ok_or("Default output device has no id")?;
    Ok(unsafe { CFRetained::from_raw(uid) }.to_string())
}

/// The tap hands over one non-interleaved buffer per channel; the resampler wants frames.
unsafe fn interleaved(buffers: &AudioBufferList, channels: usize) -> Vec<f32> {
    let list =
        std::slice::from_raw_parts(buffers.mBuffers.as_ptr(), buffers.mNumberBuffers as usize);
    let samples = |buffer: &objc2_core_audio_types::AudioBuffer| {
        std::slice::from_raw_parts(
            buffer.mData as *const f32,
            buffer.mDataByteSize as usize / size_of::<f32>(),
        )
    };
    match list {
        [] => Vec::new(),
        [single] => samples(single).to_vec(),
        planes => {
            let frames = samples(&planes[0]).len();
            let mut audio = Vec::with_capacity(frames * channels);
            for frame in 0..frames {
                for plane in planes.iter().take(channels) {
                    audio.push(samples(plane).get(frame).copied().unwrap_or(0.0));
                }
            }
            audio
        }
    }
}

/// Reads a fixed-size property, optionally qualified by the bytes Core Audio expects.
unsafe fn property<T>(
    object: AudioObjectID,
    selector: AudioObjectPropertySelector,
    qualifier: &[u8],
) -> Result<T, String> {
    let mut address = AudioObjectPropertyAddress {
        mSelector: selector,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    };
    let mut value = std::mem::MaybeUninit::<T>::zeroed();
    let mut size = size_of::<T>() as u32;
    let status = AudioObjectGetPropertyData(
        object,
        NonNull::from(&mut address),
        qualifier.len() as u32,
        qualifier.as_ptr().cast(),
        NonNull::from(&mut size),
        NonNull::new(value.as_mut_ptr()).unwrap().cast(),
    );
    status_ok(status, "read audio property")?;
    Ok(value.assume_init())
}

fn status_ok(status: i32, what: &str) -> Result<(), String> {
    if status == 0 {
        return Ok(());
    }
    Err(format!(
        "Failed to {what}: Core Audio error {}",
        code(status)
    ))
}

/// Core Audio packs most of its statuses as four characters; the decimal they add up to is
/// unsearchable, so print the characters whenever they are printable.
fn code(status: i32) -> String {
    let packed = status.to_be_bytes();
    if packed.iter().all(u8::is_ascii_graphic) {
        format!("{status} ({})", String::from_utf8_lossy(&packed))
    } else {
        status.to_string()
    }
}
