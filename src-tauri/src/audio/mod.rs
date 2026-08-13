pub mod microphone;
pub mod pcm;
pub mod resampler;

use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapturableApplication {
    pub bundle_id: String,
    pub name: String,
}

#[cfg(target_os = "macos")]
pub mod macos_apps;
#[cfg(target_os = "macos")]
pub mod process_tap;

#[cfg(target_os = "windows")]
pub mod wasapi;

#[cfg(target_os = "macos")]
pub use process_tap::SystemAudioCapture;

#[cfg(target_os = "windows")]
pub use wasapi::SystemAudioCapture;

pub const TARGET_SAMPLE_RATE: u32 = 16000;
