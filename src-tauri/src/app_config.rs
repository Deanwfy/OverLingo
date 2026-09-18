use crate::geometry::Rect;
use crate::persistence::write_atomic;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(default, rename_all = "camelCase")]
pub struct AppConfig {
    pub schema_version: u32,
    pub locale: String,
    pub audio: AudioConfig,
    pub routes: RoutePair,
    pub overlay: OverlayConfig,
    pub qwen: QwenConfig,
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
#[serde(default, rename_all = "camelCase")]
pub struct AudioConfig {
    pub system: SystemAudioConfig,
    pub microphone: MicrophoneConfig,
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
#[serde(default, rename_all = "camelCase")]
pub struct MicrophoneConfig {
    /// Capture device name; `None` follows the system default.
    pub device: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(default, rename_all = "camelCase")]
pub struct SystemAudioConfig {
    pub scope: String,
    pub application: Option<ApplicationReference>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationReference {
    pub bundle_id: String,
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(default, rename_all = "camelCase")]
pub struct RoutePair {
    pub system: RouteConfig,
    pub microphone: RouteConfig,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(default, rename_all = "camelCase")]
pub struct RouteConfig {
    pub enabled: bool,
    pub input: String,
    pub engine: String,
    pub model: String,
    pub source_language: String,
    pub target_language: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(default, rename_all = "camelCase")]
pub struct OverlayConfig {
    pub enabled: bool,
    pub opacity: f64,
    pub font_scale: f64,
    pub always_on_top: bool,
    pub click_through: bool,
    pub show_original: bool,
    pub show_translation: bool,
    /// "split" or "merged"; see [`is_overlay_layout`].
    pub layout: String,
    /// Where the subtitle box sits.
    pub frame: Option<Rect>,
}

pub(crate) fn normalize_overlay(overlay: &mut OverlayConfig) {
    overlay.frame = overlay
        .frame
        .filter(|frame| frame.is_finite() && frame.width > 0.0 && frame.height > 0.0);
    overlay.opacity = overlay.opacity.clamp(0.0, 1.0);
    // Mirrored by FONT_SCALE_RANGE in OverlaySettingsPanel.svelte.
    overlay.font_scale = overlay.font_scale.clamp(0.5, 2.0);
    if !overlay.show_original && !overlay.show_translation {
        overlay.show_translation = true;
    }
    if !is_overlay_layout(&overlay.layout) {
        overlay.layout = "split".into();
    }
}

pub(crate) fn is_overlay_layout(layout: &str) -> bool {
    matches!(layout, "split" | "merged")
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(default, rename_all = "camelCase")]
pub struct QwenConfig {
    pub region: String,
    pub workspace_id: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            locale: "auto".into(),
            audio: AudioConfig::default(),
            routes: RoutePair::default(),
            overlay: OverlayConfig::default(),
            qwen: QwenConfig::default(),
        }
    }
}

impl Default for SystemAudioConfig {
    fn default() -> Self {
        Self {
            scope: "all".into(),
            application: None,
        }
    }
}

impl Default for RoutePair {
    fn default() -> Self {
        Self {
            system: RouteConfig {
                input: "system".into(),
                ..RouteConfig::default()
            },
            microphone: RouteConfig {
                input: "microphone".into(),
                ..RouteConfig::default()
            },
        }
    }
}

impl Default for RouteConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            input: "system".into(),
            // No translator until the user picks one: a preselected model would look chosen
            // while its provider has no key.
            engine: String::new(),
            model: String::new(),
            source_language: String::new(),
            target_language: String::new(),
        }
    }
}

impl Default for OverlayConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            opacity: 0.75,
            font_scale: 1.0,
            always_on_top: true,
            click_through: true,
            show_original: true,
            show_translation: true,
            layout: "split".into(),
            frame: None,
        }
    }
}

impl Default for QwenConfig {
    fn default() -> Self {
        Self {
            region: "beijing".into(),
            workspace_id: String::new(),
        }
    }
}

impl AppConfig {
    pub fn load(app: &AppHandle) -> Self {
        let Ok(path) = config_path(app) else {
            return Self::default();
        };
        let loaded = fs::read_to_string(&path)
            .ok()
            .and_then(|content| serde_json::from_str::<Self>(&content).ok());

        match loaded {
            Some(mut config) => {
                config.normalize();
                config
            }
            None => Self::default().normalized(),
        }
    }

    pub fn save(&self, app: &AppHandle) -> Result<(), String> {
        let mut config = self.clone();
        config.normalize();
        let path = config_path(app)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("Failed to create config directory: {error}"))?;
        }
        let bytes = serde_json::to_vec_pretty(&config)
            .map_err(|error| format!("Failed to serialize app config: {error}"))?;
        write_atomic(&path, &bytes)
    }

    /// Routes on `engine` lose their translator; says whether any did.
    pub fn drop_engine(&mut self, engine: &str) -> bool {
        let mut dropped = false;
        for route in [&mut self.routes.system, &mut self.routes.microphone] {
            if route.engine == engine {
                route.model.clear();
                route.engine.clear();
                dropped = true;
            }
        }
        dropped
    }

    pub fn normalized(mut self) -> Self {
        self.normalize();
        self
    }

    fn normalize(&mut self) {
        self.schema_version = SCHEMA_VERSION;
        if !is_supported_locale(&self.locale) {
            self.locale = "auto".into();
        }
        let interface = interface_language(&resolve_locale(&self.locale));
        normalize_route(&mut self.routes.system, "system", interface);
        normalize_route(&mut self.routes.microphone, "microphone", interface);
        normalize_system_audio(&mut self.audio.system);
        if let Some(device) = &self.audio.microphone.device {
            let device = device.trim();
            self.audio.microphone.device = (!device.is_empty()).then(|| device.to_string());
        }
        self.qwen.workspace_id = self.qwen.workspace_id.trim().into();
        normalize_overlay(&mut self.overlay);
        if crate::translators::qwen_region(&self.qwen.region).is_none() {
            self.qwen.region = "beijing".into();
        }
    }
}

pub fn resolve_locale(configured: &str) -> String {
    if configured != "auto" {
        return configured.into();
    }
    let system = sys_locale::get_locale().unwrap_or_default().to_lowercase();
    if system.starts_with("zh") {
        "zh-Hans".into()
    } else if system.starts_with("es") {
        "es".into()
    } else if system.starts_with("ja") {
        "ja".into()
    } else if system.starts_with("ko") {
        "ko".into()
    } else if system.starts_with("vi") {
        "vi".into()
    } else {
        "en".into()
    }
}

pub(crate) fn is_supported_locale(locale: &str) -> bool {
    matches!(
        locale,
        "auto" | "en" | "es" | "ja" | "ko" | "vi" | "zh-Hans"
    )
}

fn normalize_system_audio(system: &mut SystemAudioConfig) {
    let valid_application = system.application.as_mut().is_some_and(|application| {
        application.bundle_id = application.bundle_id.trim().into();
        application.name = application.name.trim().into();
        !application.bundle_id.is_empty()
    });
    if system.scope != "application" || !valid_application {
        system.scope = "all".into();
        system.application = None;
    }
}

/// The language code behind an interface locale. Everything but Simplified Chinese already
/// names a language.
fn interface_language(locale: &str) -> &'static str {
    match locale {
        "zh-Hans" => "zh",
        "es" => "es",
        "ja" => "ja",
        "ko" => "ko",
        "vi" => "vi",
        _ => "en",
    }
}

/// The foreign side of a fresh route. "The other language is English" holds for everyone
/// but English readers, who need a second guess; Chinese is the widest one every translator
/// here handles.
fn counterpart_language(interface: &str) -> &'static str {
    if interface == "en" {
        "zh"
    } else {
        "en"
    }
}

fn normalize_route(route: &mut RouteConfig, input: &str, interface: &str) {
    route.input = input.into();
    // A retired model falls back to its provider's current one; one nobody recognises
    // leaves the route without a translator rather than handing it one. The engine is
    // derived from the model so the two can never disagree.
    if crate::translators::engine_of(&route.model).is_none() {
        route.model = crate::translators::default_model(&route.engine).into();
    }
    route.engine = crate::translators::engine_of(&route.model)
        .unwrap_or_default()
        .into();
    // Subtitles are read in the interface language, and the microphone route is the same
    // exchange the other way round.
    let counterpart = counterpart_language(interface);
    if route.source_language.trim().is_empty() || route.source_language == "auto" {
        route.source_language = if input == "microphone" {
            interface
        } else {
            counterpart
        }
        .into();
    }
    if route.target_language.trim().is_empty() {
        route.target_language = if input == "microphone" {
            counterpart
        } else {
            interface
        }
        .into();
    }
}

fn config_path(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_config_dir()
        .map(|path| path.join("app-config.json"))
        .map_err(|error| format!("Failed to resolve config directory: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_the_system_locale_to_a_supported_one() {
        assert_ne!(resolve_locale("auto"), "auto");
        assert_eq!(resolve_locale("ja"), "ja");
    }

    #[test]
    fn normalizes_route_inputs_and_overlay() {
        let mut config = AppConfig::default();
        config.routes.system.input = "microphone".into();
        config.overlay.opacity = 4.0;
        config.overlay.font_scale = 9.0;
        config.overlay.frame = Some(Rect {
            x: 10.0,
            y: 10.0,
            width: f64::NAN,
            height: 200.0,
        });
        config.overlay.show_original = false;
        config.overlay.show_translation = false;
        config.overlay.layout = "stacked".into();
        config.audio.microphone.device = Some("  ".into());
        config.normalize();

        assert_eq!(config.routes.system.input, "system");
        assert_eq!(config.overlay.opacity, 1.0);
        assert_eq!(config.overlay.font_scale, 2.0);
        assert_eq!(config.overlay.frame, None);
        assert!(!config.overlay.show_original);
        assert!(config.overlay.show_translation);
        assert_eq!(config.overlay.layout, "split");
        assert_eq!(config.audio.microphone.device, None);
    }

    #[test]
    fn rejects_application_scope_without_an_identifier() {
        let mut config = AppConfig::default();
        config.audio.system.scope = "application".into();
        config.audio.system.application = Some(ApplicationReference {
            bundle_id: " ".into(),
            name: "Browser".into(),
        });
        config.normalize();
        assert_eq!(config.audio.system.scope, "all");
        assert!(config.audio.system.application.is_none());
    }

    /// Model Studio runs in more regions than the model is deployed in, so a stored config
    /// naming one of the others has to fall back instead of dialling a host without it.
    #[test]
    fn an_unserved_region_falls_back() {
        let mut config = AppConfig {
            qwen: QwenConfig {
                region: "frankfurt".into(),
                ..QwenConfig::default()
            },
            ..AppConfig::default()
        };
        config.normalize();
        assert_eq!(config.qwen.region, "beijing");
    }

    /// Subtitles are read in the interface language, so a fresh install must not open on
    /// the previous hard-coded English-to-Chinese pair for a non-English reader.
    #[test]
    fn a_fresh_install_translates_into_the_interface_language() {
        for locale in ["es", "ja"] {
            let config = AppConfig {
                locale: locale.into(),
                ..AppConfig::default()
            }
            .normalized();
            assert_eq!(config.routes.system.source_language, "en");
            assert_eq!(config.routes.system.target_language, locale);
            assert_eq!(config.routes.microphone.source_language, locale);
            assert_eq!(config.routes.microphone.target_language, "en");
        }
    }

    /// English readers cannot be paired with English, so they get the second guess — and it
    /// still has to be a pair every translator can actually run.
    #[test]
    fn an_english_interface_gets_a_usable_pair() {
        let config = AppConfig {
            locale: "en".into(),
            ..AppConfig::default()
        }
        .normalized();
        assert_eq!(config.routes.system.source_language, "zh");
        assert_eq!(config.routes.system.target_language, "en");
        assert_eq!(config.routes.microphone.source_language, "en");
        assert_eq!(config.routes.microphone.target_language, "zh");

        for route in [&config.routes.system, &config.routes.microphone] {
            for model in [
                "qwen3.5-livetranslate-flash-realtime",
                "stt-rt-v5",
                "gpt-realtime-translate",
            ] {
                assert_eq!(
                    crate::translators::route_config_error(
                        model,
                        &route.source_language,
                        &route.target_language,
                    ),
                    None,
                    "{model}"
                );
            }
        }
    }

    #[test]
    fn a_route_keeps_its_translator_only_while_the_model_is_known() {
        let config = AppConfig::default().normalized();
        assert_eq!(config.routes.system.model, "");
        assert_eq!(config.routes.system.engine, "");

        let mut retired = AppConfig::default();
        retired.routes.system.engine = "qwen".into();
        retired.routes.system.model = "qwen3-livetranslate-flash-realtime".into();
        let retired = retired.normalized();
        assert_eq!(
            retired.routes.system.model,
            "qwen3.5-livetranslate-flash-realtime"
        );
        assert_eq!(retired.routes.system.engine, "qwen");

        let mut unknown = AppConfig::default();
        unknown.routes.system.engine = "gemini".into();
        unknown.routes.system.model = "gemini-live".into();
        let unknown = unknown.normalized();
        assert_eq!(unknown.routes.system.model, "");
        assert_eq!(unknown.routes.system.engine, "");
    }

    #[test]
    fn clearing_a_key_unsets_the_routes_on_that_translator() {
        let mut config = AppConfig::default();
        config.routes.system.model = "qwen3.5-livetranslate-flash-realtime".into();
        config.routes.microphone.model = "gpt-realtime-translate".into();
        let mut config = config.normalized();

        assert!(config.drop_engine("qwen"));
        assert_eq!(config.routes.system.model, "");
        assert_eq!(config.routes.system.engine, "");
        assert_eq!(config.routes.microphone.model, "gpt-realtime-translate");
        assert!(!config.drop_engine("qwen"));
    }

    /// Normalize runs on every save, so it must never rewrite a pair the user chose.
    #[test]
    fn a_chosen_pair_survives_normalization() {
        let mut config = AppConfig {
            locale: "ja".into(),
            ..AppConfig::default()
        };
        config.routes.system.source_language = "ko".into();
        config.routes.system.target_language = "vi".into();
        config.normalize();
        assert_eq!(config.routes.system.source_language, "ko");
        assert_eq!(config.routes.system.target_language, "vi");
    }

    #[test]
    fn accepts_new_locales_and_clear_background() {
        let mut config = AppConfig {
            locale: "es".into(),
            ..AppConfig::default()
        };
        config.overlay.opacity = 0.0;
        config.normalize();

        assert_eq!(config.locale, "es");
        assert_eq!(config.overlay.opacity, 0.0);
    }
}
