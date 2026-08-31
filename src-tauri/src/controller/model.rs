use super::state::{RouteState, TranslationState};
use super::transcript::{TranscriptDraft, TranscriptTurn};
use crate::app_config::{AudioConfig, OverlayConfig, QwenConfig, RouteConfig};
use crate::audio::CapturableApplication;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Debug, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ControllerRequest {
    Translation {
        command: TranslationCommand,
    },
    ToggleTranslation,
    ToggleOverlay,
    ShowOverlay {
        visible: bool,
    },
    Hide,
    Settings {
        patch: OverlaySettingsPatch,
    },
    Route {
        route_id: String,
        enabled: bool,
    },
    RouteSettings {
        route_id: String,
        patch: RouteSettingsPatch,
    },
    RetryRoute {
        route_id: String,
    },
    QwenSettings {
        patch: QwenSettingsPatch,
    },
    Locale {
        locale: String,
    },
    Capture {
        bundle_id: String,
    },
    MicrophoneDevice {
        device: String,
    },
    RequestCaptureOptions,
    Exit,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TranslationCommand {
    Start,
    Stop,
    Pause,
    Resume,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct OverlaySettingsPatch {
    pub(super) opacity: Option<f64>,
    pub(super) font_scale: Option<f64>,
    pub(super) always_on_top: Option<bool>,
    pub(super) click_through: Option<bool>,
    pub(super) show_original: Option<bool>,
    pub(super) show_translation: Option<bool>,
    pub(super) layout: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RouteSettingsPatch {
    pub(super) model: Option<String>,
    pub(super) source_language: Option<String>,
    pub(super) target_language: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct QwenSettingsPatch {
    pub(super) region: Option<String>,
    pub(super) workspace_id: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ControllerSnapshot {
    pub(super) locale: String,
    pub(super) preferred_locale: String,
    pub(super) translation_state: TranslationState,
    pub(super) elapsed_seconds: u64,
    pub(super) overlay_visible: bool,
    pub(super) config: OverlayConfig,
    pub(super) audio: AudioConfig,
    pub(super) qwen: QwenConfig,
    pub(super) capture: CaptureSnapshot,
    pub(super) credentials: HashMap<String, bool>,
    pub(super) routes: HashMap<String, RouteSnapshot>,
    pub(super) notice: Option<ControllerNotice>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CaptureSnapshot {
    pub(super) capabilities: CaptureCapabilities,
    pub(super) applications: Vec<CapturableApplication>,
    pub(super) microphones: Vec<String>,
    pub(super) loading: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CaptureCapabilities {
    pub(super) application_capture: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RouteSnapshot {
    pub(super) config: RouteConfig,
    pub(super) state: RouteState,
    pub(super) error: String,
    pub(super) turns: Vec<TranscriptTurn>,
    pub(super) draft: TranscriptDraft,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ControllerNotice {
    pub(super) id: u64,
    pub(super) code: Option<String>,
    pub(super) message: String,
}
