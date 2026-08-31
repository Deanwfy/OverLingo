use super::pcm::PcmConverter;
use super::TARGET_SAMPLE_RATE;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

pub struct MicCapture {
    // Per-session flag, not a shared one: a stale worker must never be kept alive
    // by the next session flipping the flag back to true.
    active: Option<Arc<AtomicBool>>,
}

impl MicCapture {
    pub fn new() -> Self {
        Self { active: None }
    }

    pub fn start(&mut self, device: Option<&str>) -> Result<mpsc::Receiver<Vec<u8>>, String> {
        if self.active.is_some() {
            return Err("Already capturing".into());
        }

        let (audio_sender, audio_receiver) = mpsc::channel();
        let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
        let is_capturing = Arc::new(AtomicBool::new(true));
        let worker_flag = is_capturing.clone();
        let device = device.map(str::to_string);
        std::thread::spawn(move || {
            run_capture(device, audio_sender, worker_flag, ready_sender);
        });

        let outcome = match ready_receiver.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => Ok(audio_receiver),
            Ok(Err(error)) => Err(error),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                Err("Microphone capture did not start in time".into())
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err("Microphone capture stopped before it was ready".into())
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

impl Default for MicCapture {
    fn default() -> Self {
        Self::new()
    }
}

fn run_capture(
    device_name: Option<String>,
    sender: mpsc::Sender<Vec<u8>>,
    is_capturing: Arc<AtomicBool>,
    ready: mpsc::SyncSender<Result<(), String>>,
) {
    match build_stream(device_name.as_deref(), sender, is_capturing.clone()) {
        Ok(stream) => {
            let _ = ready.send(Ok(()));
            while is_capturing.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(50));
            }
            drop(stream);
        }
        Err(error) => {
            is_capturing.store(false, Ordering::SeqCst);
            let _ = ready.send(Err(error));
        }
    }
}

fn build_stream(
    device_name: Option<&str>,
    sender: mpsc::Sender<Vec<u8>>,
    is_capturing: Arc<AtomicBool>,
) -> Result<cpal::Stream, String> {
    let host = cpal::default_host();
    let device_count = host
        .input_devices()
        .map(|devices| devices.count())
        .unwrap_or_default();
    crate::diagnostics::log(
        "audio:microphone",
        format!("input_device_count={device_count}"),
    );
    if device_count == 0 {
        return Err("No microphone found. Connect a microphone or headset.".into());
    }

    // A stored device may be unplugged; falling back to the default keeps the route
    // alive instead of failing until the user reopens settings.
    let device = device_name
        .and_then(|name| {
            let found = host.input_devices().ok().and_then(|mut devices| {
                devices.find(|device| device.name().is_ok_and(|current| current == name))
            });
            if found.is_none() {
                crate::diagnostics::log(
                    "audio:microphone",
                    format!("device_missing name={}", crate::diagnostics::field(name)),
                );
            }
            found
        })
        .map(Ok)
        .unwrap_or_else(|| {
            host.default_input_device()
                .ok_or("No default microphone found. Connect a microphone or headset.")
        })?;
    let input_config = preferred_input_config(&device)?;
    let sample_rate = input_config.sample_rate().0;
    let channels = input_config.channels() as usize;
    crate::diagnostics::log(
        "audio:microphone",
        format!(
            "config rate={sample_rate} channels={channels} format={:?}",
            input_config.sample_format()
        ),
    );

    let stream_config = cpal::StreamConfig {
        channels: input_config.channels(),
        sample_rate: input_config.sample_rate(),
        buffer_size: cpal::BufferSize::Default,
    };
    // Killing the flag drops the stream and its sender, which the forwarder upstream
    // reports as a dead capture; logging alone would leave a silent route looking Live.
    let error_callback = {
        let is_capturing = is_capturing.clone();
        move |error: cpal::StreamError| {
            crate::diagnostics::log(
                "audio:microphone",
                format!(
                    "stream_error={}",
                    crate::diagnostics::field(&error.to_string())
                ),
            );
            is_capturing.store(false, Ordering::SeqCst);
        }
    };

    let mut rate_check = RateCheck::new(sample_rate, channels, is_capturing.clone());
    let mut converter = PcmConverter::new(sample_rate, channels, TARGET_SAMPLE_RATE)?;
    let stream = match input_config.sample_format() {
        cpal::SampleFormat::F32 => device.build_input_stream(
            &stream_config,
            move |data: &[f32], _| {
                if is_capturing.load(Ordering::SeqCst) {
                    rate_check.note(data.len());
                    send_audio(&sender, converter.convert_f32(data));
                }
            },
            error_callback,
            None,
        ),
        cpal::SampleFormat::I16 => device.build_input_stream(
            &stream_config,
            move |data: &[i16], _| {
                if is_capturing.load(Ordering::SeqCst) {
                    rate_check.note(data.len());
                    send_audio(&sender, converter.convert_i16(data));
                }
            },
            error_callback,
            None,
        ),
        format => return Err(format!("Unsupported microphone sample format: {format:?}")),
    }
    .map_err(|error| format!("Failed to build microphone stream: {error}"))?;
    stream
        .play()
        .map_err(|error| format!("Failed to start microphone stream: {error}"))?;
    Ok(stream)
}

/// Catches the failure that raises no error: macOS silently moving the default input to
/// another device mid-stream. The callbacks keep firing at the new device's rate, so audio
/// resampled under the old rate reaches the translator at the wrong speed. A sustained
/// wall-clock rate mismatch kills the stream so the capture is rebuilt on the new format.
struct RateCheck {
    expected: f64,
    samples: usize,
    since: Option<Instant>,
    alive: Arc<AtomicBool>,
}

const RATE_CHECK_WINDOW: Duration = Duration::from_secs(3);
/// Real drift is parts per million; a device swap changes the rate by whole factors.
const RATE_TOLERANCE: f64 = 0.2;

impl RateCheck {
    fn new(sample_rate: u32, channels: usize, alive: Arc<AtomicBool>) -> Self {
        Self {
            expected: f64::from(sample_rate) * channels.max(1) as f64,
            samples: 0,
            since: None,
            alive,
        }
    }

    fn note(&mut self, samples: usize) {
        // Windowed from the first callback, so device spin-up never counts as missing samples.
        let since = *self.since.get_or_insert_with(Instant::now);
        self.samples += samples;
        let elapsed = since.elapsed();
        if elapsed < RATE_CHECK_WINDOW {
            return;
        }
        let measured = self.samples as f64 / elapsed.as_secs_f64();
        if (measured - self.expected).abs() > self.expected * RATE_TOLERANCE {
            crate::diagnostics::log(
                "audio:microphone",
                format!(
                    "rate_mismatch expected={} measured={measured:.0}",
                    self.expected
                ),
            );
            self.alive.store(false, Ordering::SeqCst);
        }
        self.samples = 0;
        self.since = Some(Instant::now());
    }
}

fn preferred_input_config(device: &cpal::Device) -> Result<cpal::SupportedStreamConfig, String> {
    device.default_input_config().or_else(|default_error| {
        crate::diagnostics::log(
            "audio:microphone",
            format!(
                "default_config_failed error={}",
                crate::diagnostics::field(&default_error.to_string())
            ),
        );
        let mut configs = device
            .supported_input_configs()
            .map_err(|error| format!("No supported microphone formats: {error}"))?;
        configs
            .find(|config| config.sample_format() == cpal::SampleFormat::F32)
            .or_else(|| device.supported_input_configs().ok()?.next())
            .map(|config| {
                let rate = if config.min_sample_rate().0 <= 48_000
                    && config.max_sample_rate().0 >= 48_000
                {
                    cpal::SampleRate(48_000)
                } else {
                    config.max_sample_rate()
                };
                config.with_sample_rate(rate)
            })
            .ok_or_else(|| format!("No usable microphone format: {default_error}"))
    })
}

fn send_audio(sender: &mpsc::Sender<Vec<u8>>, audio: Vec<u8>) {
    if !audio.is_empty() {
        let _ = sender.send(audio);
    }
}
