use super::model::{OverlaySettingsPatch, QwenSettingsPatch, RouteSettingsPatch};
use super::{enabled_route_ids, route_config, route_config_mut, Action, ControllerActor};
use crate::app_config::{is_overlay_layout, ApplicationReference, RouteConfig};
use crate::commands::audio;
use crate::credentials::CredentialState;
use crate::translators::{engine_of, route_config_error, start_blocker};
use tauri::Manager;

impl ControllerActor {
    pub(super) fn route_config(&self, route_id: &str) -> Option<&RouteConfig> {
        route_config(&self.config, route_id)
    }

    pub(super) fn route_config_error(&self, route_id: &str) -> Option<&'static str> {
        let route = self.route_config(route_id)?;
        route_config_error(&route.model, &route.source_language, &route.target_language)
    }

    pub(super) fn enabled_route_ids(&self) -> Vec<&'static str> {
        enabled_route_ids(&self.config)
    }

    /// The bundle id system audio is scoped to, or `None` for everything.
    pub(super) fn captured_application(&self) -> Option<String> {
        let system = &self.config.audio.system;
        (system.scope == "application")
            .then(|| {
                system
                    .application
                    .as_ref()
                    .map(|application| application.bundle_id.clone())
            })
            .flatten()
    }

    pub(super) fn update_overlay_settings(&mut self, patch: OverlaySettingsPatch) {
        let overlay = &mut self.config.overlay;
        if let Some(opacity) = patch.opacity.filter(|value| value.is_finite()) {
            overlay.opacity = opacity.clamp(0.0, 1.0);
        }
        if let Some(font_scale) = patch.font_scale.filter(|value| value.is_finite()) {
            overlay.font_scale = font_scale.clamp(0.75, 1.8);
        }
        if let Some(always_on_top) = patch.always_on_top {
            overlay.always_on_top = always_on_top;
        }
        if let Some(click_through) = patch.click_through {
            overlay.click_through = click_through;
        }
        if let Some(show_original) = patch.show_original {
            overlay.show_original = show_original;
        }
        if let Some(show_translation) = patch.show_translation {
            overlay.show_translation = show_translation;
        }
        if !overlay.show_original && !overlay.show_translation {
            overlay.show_translation = true;
        }
        if let Some(layout) = patch.layout.filter(|value| is_overlay_layout(value)) {
            overlay.layout = layout;
        }
        if patch.always_on_top.is_some() || patch.click_through.is_some() {
            self.apply_overlay_window_flags();
        }
        self.save_config();
        self.publish();
    }

    pub(super) fn update_route_enabled(&mut self, route_id: &str, enabled: bool) {
        // The last remaining source cannot be turned off; there would be nothing to translate.
        if !enabled && self.enabled_route_ids() == [route_id] {
            return;
        }
        let Some(route) = route_config_mut(&mut self.config, route_id) else {
            return;
        };
        route.enabled = enabled;
        self.save_config();
        self.reconfigure_if_active();
        self.publish();
    }

    pub(super) fn update_route_settings(&mut self, route_id: &str, patch: RouteSettingsPatch) {
        let Some(route) = route_config_mut(&mut self.config, route_id) else {
            return;
        };
        let model = patch.model.unwrap_or_else(|| route.model.clone());
        let source = patch
            .source_language
            .unwrap_or_else(|| route.source_language.clone());
        let target = patch
            .target_language
            .unwrap_or_else(|| route.target_language.clone());
        // The model picks the provider, so an unknown one is rejected rather than guessed at.
        // A language the model cannot handle is stored anyway: the user needs to be able to
        // change translator and language in either order, and only the combination matters.
        let Some(engine) = engine_of(&model) else {
            return;
        };
        route.engine = engine.into();
        route.model = model;
        route.source_language = source;
        route.target_language = target;
        self.save_config();
        self.reconfigure_if_active();
        self.publish();
    }

    pub(super) fn update_qwen_settings(&mut self, patch: QwenSettingsPatch) {
        if let Some(region) = patch.region {
            if crate::translators::qwen_region(&region).is_none() {
                return;
            }
            self.config.qwen.region = region;
        }
        if let Some(workspace_id) = patch.workspace_id {
            self.config.qwen.workspace_id = workspace_id.trim().into();
        }
        self.save_config();
        self.reconfigure_if_active();
        self.publish();
    }

    pub(super) fn update_locale(&mut self, locale: &str) {
        if !crate::app_config::is_supported_locale(locale) {
            return;
        }
        self.config.locale = locale.into();
        self.save_config();
        self.publish();
    }

    pub(super) fn update_capture(&mut self, bundle_id: String) {
        let system = &mut self.config.audio.system;
        if bundle_id == "all" {
            system.scope = "all".into();
            system.application = None;
        } else if let Some(application) = self
            .capture_applications
            .iter()
            .find(|application| application.bundle_id == bundle_id)
        {
            system.scope = "application".into();
            system.application = Some(ApplicationReference {
                bundle_id: application.bundle_id.clone(),
                name: application.name.clone(),
            });
        } else {
            return;
        }
        self.save_config();
        self.reconfigure_if_active();
        self.publish();
    }

    pub(super) fn load_capture_options(&mut self) {
        if self.capture_loading {
            return;
        }
        self.capture_loading = true;
        self.publish();
        let sender = self.sender.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let result = audio::list_capturable_applications();
            let _ = sender.send(Action::CaptureOptionsLoaded(result));
        });
    }

    pub(super) fn apply_overlay_window_flags(&self) {
        let Some(overlay) = self.app.get_webview_window("overlay") else {
            return;
        };
        let _ = overlay.set_always_on_top(self.config.overlay.always_on_top);
        crate::overlay_pointer::apply(
            &self.app,
            self.config.overlay.click_through && self.overlay_visible,
        );
    }

    pub(super) fn set_overlay_visible(&mut self, visible: bool, persist: bool) {
        self.overlay_visible = visible;
        self.config.overlay.enabled = visible;
        if let Some(overlay) = self.app.get_webview_window("overlay") {
            let _ = if visible {
                overlay.show()
            } else {
                overlay.hide()
            };
        }
        self.apply_overlay_window_flags();
        if persist {
            self.save_config();
        }
        self.publish();
    }

    /// The notice code blocking a session start, if any.
    pub(super) fn validate_start(&self) -> Option<&'static str> {
        let enabled = self.enabled_route_ids();
        if enabled.is_empty() {
            return Some("chooseAudioSource");
        }
        if self.config.routes.system.enabled
            && self.config.audio.system.scope == "application"
            && self.config.audio.system.application.is_none()
        {
            return Some("chooseApplication");
        }
        let credentials = self.app.state::<CredentialState>().0.lock().ok()?.clone();
        for route_id in enabled {
            let route = self.route_config(route_id)?;
            if let Some(code) =
                route_config_error(&route.model, &route.source_language, &route.target_language)
            {
                return Some(code);
            }
            if let Some(code) = start_blocker(&route.engine, &self.config, &credentials) {
                return Some(code);
            }
        }
        None
    }

    pub(super) fn save_config(&mut self) {
        let saved = self.config.clone().normalized();
        match saved.save(&self.app) {
            Ok(()) => self.config = saved,
            Err(error) => self.set_notice(None, error),
        }
    }
}
