use crate::audio::microphone::MicCapture;
use crate::audio::{CapturableApplication, SystemAudioCapture};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

pub type CaptureCallback = Box<dyn FnOnce(Result<(), String>) + Send>;
/// Called at most once, when a running capture dies without being stopped.
pub type CaptureFailure = Box<dyn FnOnce(String) + Send>;

/// A microphone delivers at a constant rate whether anyone speaks or not, so this long
/// with no bytes means the stream is dead. Never applied to system audio, where zero
/// bytes is what silence legitimately looks like.
const MICROPHONE_STALL: Duration = Duration::from_secs(5);

/// Capture start/stop are blocking OS calls (ScreenCaptureKit, CoreAudio, WASAPI) that can
/// take seconds. They run on a dedicated worker so callers never block, and stay serialized
/// so a stop always completes before the start queued behind it.
pub struct AudioState {
    commands: mpsc::Sender<Command>,
}

impl AudioState {
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || Worker::default().run(receiver));
        Self { commands: sender }
    }

    pub fn start_capture(
        &self,
        route_id: String,
        capture: CaptureRequest,
        on_audio: AudioSink,
        done: CaptureCallback,
        on_failed: CaptureFailure,
    ) {
        if let Err(error) = validate_route_id(&route_id) {
            done(Err(error));
            return;
        }
        if let Err(error) = self.commands.send(Command::Start {
            route_id,
            capture,
            on_audio,
            done,
            on_failed,
        }) {
            let Command::Start { done, .. } = error.0 else {
                return;
            };
            done(Err("Audio capture worker is unavailable".into()));
        }
    }

    pub fn stop_capture(&self, route_id: &str) {
        let _ = self.commands.send(Command::Stop {
            route_id: route_id.into(),
        });
    }
}

impl Default for AudioState {
    fn default() -> Self {
        Self::new()
    }
}

enum Command {
    Start {
        route_id: String,
        capture: CaptureRequest,
        on_audio: AudioSink,
        done: CaptureCallback,
        on_failed: CaptureFailure,
    },
    Stop {
        route_id: String,
    },
}

#[derive(Clone)]
pub struct AudioSink(Arc<dyn Fn(Vec<u8>) + Send + Sync>);

impl AudioSink {
    pub fn callback(handler: impl Fn(Vec<u8>) + Send + Sync + 'static) -> Self {
        Self(Arc::new(handler))
    }

    fn send(&self, audio: Vec<u8>) {
        (self.0)(audio);
    }
}

#[derive(Clone, Copy, PartialEq)]
enum AudioSource {
    System,
    Microphone,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum CaptureRequest {
    System {
        #[serde(rename = "applicationBundleId")]
        application_bundle_id: Option<String>,
    },
    Microphone {
        device: Option<String>,
    },
}

struct ActiveCapture {
    source: AudioSource,
    stop: Arc<AtomicBool>,
}

#[derive(Default)]
struct Worker {
    system_audio: SystemAudioCapture,
    microphone: MicCapture,
    routes: HashMap<String, ActiveCapture>,
}

impl Worker {
    fn run(mut self, receiver: mpsc::Receiver<Command>) {
        while let Ok(command) = receiver.recv() {
            match command {
                Command::Start {
                    route_id,
                    capture,
                    on_audio,
                    done,
                    on_failed,
                } => {
                    let result = self.start(route_id, capture, on_audio, on_failed);
                    done(result);
                }
                Command::Stop { route_id } => self.stop(&route_id),
            }
        }
    }

    fn start(
        &mut self,
        route_id: String,
        capture: CaptureRequest,
        on_audio: AudioSink,
        on_failed: CaptureFailure,
    ) -> Result<(), String> {
        let source = match &capture {
            CaptureRequest::System { .. } => AudioSource::System,
            CaptureRequest::Microphone { .. } => AudioSource::Microphone,
        };

        self.stop(&route_id);
        if self.routes.values().any(|route| route.source == source) {
            return Err("Audio source is already in use".into());
        }

        crate::diagnostics::log(
            "audio",
            format!(
                "start route={} source={}",
                crate::diagnostics::field(&route_id),
                source.name()
            ),
        );
        let receiver = match capture {
            CaptureRequest::System {
                application_bundle_id,
            } => self.system_audio.start(application_bundle_id.as_deref()),
            CaptureRequest::Microphone { device } => self.microphone.start(device.as_deref()),
        }
        .inspect_err(|error| {
            crate::diagnostics::log(
                "audio",
                format!(
                    "start_failed route={} source={} error={}",
                    crate::diagnostics::field(&route_id),
                    source.name(),
                    crate::diagnostics::field(error)
                ),
            );
        })?;

        let stop = Arc::new(AtomicBool::new(false));
        forward_audio(
            route_id.clone(),
            receiver,
            on_audio,
            stop.clone(),
            (source == AudioSource::Microphone).then_some(on_failed),
        );
        crate::diagnostics::log(
            "audio",
            format!(
                "started route={} source={}",
                crate::diagnostics::field(&route_id),
                source.name()
            ),
        );
        self.routes.insert(route_id, ActiveCapture { source, stop });
        Ok(())
    }

    fn stop(&mut self, route_id: &str) {
        let Some(route) = self.routes.remove(route_id) else {
            return;
        };
        route.stop.store(true, Ordering::SeqCst);
        match route.source {
            AudioSource::System => self.system_audio.stop(),
            AudioSource::Microphone => self.microphone.stop(),
        }
    }
}

pub fn list_input_devices() -> Vec<String> {
    use cpal::traits::{DeviceTrait, HostTrait};
    cpal::default_host()
        .input_devices()
        .map(|devices| devices.filter_map(|device| device.name().ok()).collect())
        .unwrap_or_default()
}

pub fn list_capturable_applications() -> Result<Vec<CapturableApplication>, String> {
    #[cfg(target_os = "macos")]
    {
        Ok(crate::audio::macos_apps::running())
    }
    #[cfg(target_os = "windows")]
    {
        Ok(Vec::new())
    }
}

/// `on_dead` is only ever `Some` for the microphone: its constant flow makes a stall or
/// an unexpected end of the stream proof of death rather than of quiet.
fn forward_audio(
    route_id: String,
    receiver: mpsc::Receiver<Vec<u8>>,
    channel: AudioSink,
    stop: Arc<AtomicBool>,
    mut on_dead: Option<CaptureFailure>,
) {
    std::thread::spawn(move || {
        let mut buffer = Vec::with_capacity(12_800);
        let interval = Duration::from_millis(120);
        let mut last_flush = Instant::now();
        let mut last_data = Instant::now();
        let mut flow = Throughput::new(route_id);

        loop {
            if stop.load(Ordering::SeqCst) {
                send_buffer(&channel, &mut buffer);
                break;
            }
            match receiver.recv_timeout(Duration::from_millis(10)) {
                Ok(data) => {
                    flow.add(&data);
                    last_data = Instant::now();
                    buffer.extend_from_slice(&data);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    send_buffer(&channel, &mut buffer);
                    if let Some(on_dead) = on_dead.take() {
                        on_dead("Microphone stream ended unexpectedly".into());
                    }
                    break;
                }
            }
            if on_dead.is_some() && last_data.elapsed() >= MICROPHONE_STALL {
                send_buffer(&channel, &mut buffer);
                if let Some(on_dead) = on_dead.take() {
                    on_dead("Microphone stream stopped delivering audio".into());
                }
                break;
            }
            if last_flush.elapsed() >= interval && !buffer.is_empty() {
                channel.send(std::mem::take(&mut buffer));
                last_flush = Instant::now();
            }
            flow.report();
        }
    });
}

/// Diagnostics only: how much audio a capture delivers and how loud it is, so a stream
/// that stalls or goes all-zero without erroring is visible in the log.
struct Throughput {
    route_id: String,
    bytes: usize,
    square_sum: f64,
    peak: i16,
    since: Instant,
}

impl Throughput {
    fn new(route_id: String) -> Self {
        Self {
            route_id,
            bytes: 0,
            square_sum: 0.0,
            peak: 0,
            since: Instant::now(),
        }
    }

    /// A healthy microphone hovers above rms=0 even in a quiet room; a flatline here is a
    /// muted or phantom device.
    fn add(&mut self, data: &[u8]) {
        if !crate::diagnostics::enabled() {
            return;
        }
        self.bytes += data.len();
        for &sample in data.as_chunks::<2>().0 {
            let value = i16::from_le_bytes(sample);
            self.square_sum += f64::from(value) * f64::from(value);
            self.peak = self.peak.max(value.saturating_abs());
        }
    }

    fn report(&mut self) {
        if !crate::diagnostics::enabled() || self.since.elapsed() < Duration::from_secs(2) {
            return;
        }
        let samples = (self.bytes / 2).max(1);
        crate::diagnostics::log(
            "audio",
            format!(
                "flow route={} bytes={} rms={:.0} peak={} window_ms={}",
                crate::diagnostics::field(&self.route_id),
                self.bytes,
                (self.square_sum / samples as f64).sqrt(),
                self.peak,
                self.since.elapsed().as_millis()
            ),
        );
        self.bytes = 0;
        self.square_sum = 0.0;
        self.peak = 0;
        self.since = Instant::now();
    }
}

fn send_buffer(channel: &AudioSink, buffer: &mut Vec<u8>) {
    if !buffer.is_empty() {
        channel.send(std::mem::take(buffer));
    }
}

impl AudioSource {
    fn name(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Microphone => "microphone",
        }
    }
}

fn validate_route_id(route_id: &str) -> Result<(), String> {
    if route_id.is_empty()
        || route_id.len() > 32
        || !route_id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return Err("Invalid audio route id".into());
    }
    Ok(())
}
