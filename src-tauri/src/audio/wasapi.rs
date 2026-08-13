use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use super::pcm::PcmConverter;
use super::TARGET_SAMPLE_RATE;

pub struct SystemAudioCapture {
    // Per-session flag, not a shared one: a stale worker must never be kept alive
    // by the next session flipping the flag back to true.
    active: Option<Arc<AtomicBool>>,
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
        if application_bundle_id.is_some() {
            return Err("Application-specific audio capture is unavailable on Windows".into());
        }

        let (sender, receiver) = mpsc::channel::<Vec<u8>>();
        let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
        let is_capturing = Arc::new(AtomicBool::new(true));
        let capture_flag = is_capturing.clone();
        std::thread::spawn(move || {
            run_loopback(sender, capture_flag, ready_sender);
        });

        let outcome = match ready_receiver.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => Ok(receiver),
            Ok(Err(error)) => Err(error),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                Err("System audio capture did not start in time".into())
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err("System audio capture stopped before it was ready".into())
            }
        };
        match outcome {
            Ok(receiver) => {
                self.active = Some(is_capturing);
                Ok(receiver)
            }
            Err(error) => {
                is_capturing.store(false, Ordering::SeqCst);
                Err(error)
            }
        }
    }

    pub fn stop(&mut self) {
        if let Some(is_capturing) = self.active.take() {
            is_capturing.store(false, Ordering::SeqCst);
        }
    }
}

impl Default for SystemAudioCapture {
    fn default() -> Self {
        Self::new()
    }
}

use windows::Win32::Media::Audio::{
    eConsole, eRender, IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator, MMDeviceEnumerator,
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK,
    WAVEFORMATEX,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
    COINIT_MULTITHREADED,
};

struct ComApartment;

impl Drop for ComApartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() }
    }
}

struct WaveFormat(*mut WAVEFORMATEX);

impl Drop for WaveFormat {
    fn drop(&mut self) {
        unsafe { CoTaskMemFree(Some(self.0.cast())) }
    }
}

fn run_loopback(
    sender: mpsc::Sender<Vec<u8>>,
    is_capturing: Arc<AtomicBool>,
    ready: mpsc::SyncSender<Result<(), String>>,
) {
    unsafe {
        if let Err(error) = CoInitializeEx(None, COINIT_MULTITHREADED).ok() {
            fail_start(
                &ready,
                &is_capturing,
                format!("Failed to initialize Windows audio: {error}"),
            );
            return;
        }
        let _apartment = ComApartment;

        let enumerator: IMMDeviceEnumerator =
            match CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) {
                Ok(e) => e,
                Err(e) => {
                    fail_start(
                        &ready,
                        &is_capturing,
                        format!("Failed to create Windows audio device enumerator: {e}"),
                    );
                    return;
                }
            };

        let device = match enumerator.GetDefaultAudioEndpoint(eRender, eConsole) {
            Ok(d) => d,
            Err(e) => {
                fail_start(
                    &ready,
                    &is_capturing,
                    format!("Failed to get the default Windows audio output: {e}"),
                );
                return;
            }
        };

        let audio_client: IAudioClient = match device.Activate(CLSCTX_ALL, None) {
            Ok(c) => c,
            Err(e) => {
                fail_start(
                    &ready,
                    &is_capturing,
                    format!("Failed to open Windows system audio: {e}"),
                );
                return;
            }
        };

        let mix_format = WaveFormat(match audio_client.GetMixFormat() {
            Ok(f) => f,
            Err(e) => {
                fail_start(
                    &ready,
                    &is_capturing,
                    format!("Failed to read the Windows audio format: {e}"),
                );
                return;
            }
        });
        let format = &*mix_format.0;

        let source_rate = format.nSamplesPerSec;
        let source_channels = format.nChannels as u32;
        let bits_per_sample = format.wBitsPerSample;

        if let Err(e) = audio_client.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            AUDCLNT_STREAMFLAGS_LOOPBACK,
            10_000_000,
            0,
            mix_format.0,
            None,
        ) {
            fail_start(
                &ready,
                &is_capturing,
                format!("Failed to initialize Windows system audio capture: {e}"),
            );
            return;
        }

        let capture_client: IAudioCaptureClient = match audio_client.GetService() {
            Ok(c) => c,
            Err(e) => {
                fail_start(
                    &ready,
                    &is_capturing,
                    format!("Failed to create Windows system audio capture: {e}"),
                );
                return;
            }
        };

        if let Err(e) = audio_client.Start() {
            fail_start(
                &ready,
                &is_capturing,
                format!("Failed to start Windows system audio capture: {e}"),
            );
            return;
        }

        if source_channels == 0 {
            fail_start(
                &ready,
                &is_capturing,
                "Windows system audio reported an invalid channel count".into(),
            );
            let _ = audio_client.Stop();
            return;
        }

        let mut converter =
            match PcmConverter::new(source_rate, source_channels as usize, TARGET_SAMPLE_RATE) {
                Ok(converter) => converter,
                Err(error) => {
                    fail_start(&ready, &is_capturing, error);
                    let _ = audio_client.Stop();
                    return;
                }
            };
        let _ = ready.send(Ok(()));
        crate::diagnostics::log(
            "audio:system",
            format!(
                "capturing rate={source_rate} channels={source_channels} bits={bits_per_sample}"
            ),
        );

        while is_capturing.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(10));

            let packet_size = match capture_client.GetNextPacketSize() {
                Ok(size) => size,
                Err(_) => continue,
            };

            if packet_size == 0 {
                continue;
            }

            let mut buffer_ptr = std::ptr::null_mut();
            let mut num_frames = 0u32;
            let mut flags = 0u32;

            if capture_client
                .GetBuffer(&mut buffer_ptr, &mut num_frames, &mut flags, None, None)
                .is_err()
            {
                continue;
            }

            if num_frames > 0 && !buffer_ptr.is_null() {
                let is_silent = (flags & (AUDCLNT_BUFFERFLAGS_SILENT.0 as u32)) != 0;

                if !is_silent {
                    let pcm_data = convert_to_pcm_s16_16k(
                        buffer_ptr,
                        num_frames,
                        source_channels,
                        bits_per_sample,
                        &mut converter,
                    );

                    if !pcm_data.is_empty() && sender.send(pcm_data).is_err() {
                        let _ = capture_client.ReleaseBuffer(num_frames);
                        break;
                    }
                }
            }

            let _ = capture_client.ReleaseBuffer(num_frames);
        }

        let _ = audio_client.Stop();
        is_capturing.store(false, Ordering::SeqCst);
    }
}

fn fail_start(
    ready: &mpsc::SyncSender<Result<(), String>>,
    is_capturing: &AtomicBool,
    error: String,
) {
    crate::diagnostics::log(
        "audio:system",
        format!("start_failed error={}", crate::diagnostics::field(&error)),
    );
    is_capturing.store(false, Ordering::SeqCst);
    let _ = ready.send(Err(error));
}

unsafe fn convert_to_pcm_s16_16k(
    buffer_ptr: *mut u8,
    num_frames: u32,
    source_channels: u32,
    bits_per_sample: u16,
    converter: &mut PcmConverter,
) -> Vec<u8> {
    let frame_count = num_frames as usize;

    if bits_per_sample != 32 || source_channels == 0 {
        return Vec::new();
    }

    let ptr = buffer_ptr as *const f32;
    let total_samples = frame_count * source_channels as usize;
    let f32_samples = std::slice::from_raw_parts(ptr, total_samples);

    converter.convert_f32(f32_samples)
}
