use super::model::{CaptureCapabilities, CaptureSnapshot, ControllerSnapshot, RouteSnapshot};
use super::state::RouteState;
use super::{ControllerActor, ROUTE_IDS};
use crate::credentials::CredentialState;
use std::collections::HashMap;
use tauri::{AppHandle, Manager};

const VISIBLE_TURNS: usize = 24;

impl ControllerActor {
    pub(super) fn publish(&mut self) {
        let snapshot = self.build_snapshot();
        if let Ok(mut current) = self.snapshot.lock() {
            *current = Some(snapshot.clone());
        }
        if let Ok(mut subscribers) = self.subscribers.lock() {
            subscribers.retain(|_, channel| channel.send(snapshot.clone()).is_ok());
        }
        crate::shell::update_tray_for_app(
            &self.app,
            crate::shell::TrayPresentation {
                locale: snapshot.locale.clone(),
                translation_active: self.translation_state.is_active(),
                translation_running: self.translation_state.is_translating(),
                overlay_visible: snapshot.overlay_visible,
            },
        );
    }

    fn build_snapshot(&self) -> ControllerSnapshot {
        let routes = ROUTE_IDS
            .into_iter()
            .map(|route_id| {
                let active = self.routes.get(route_id);
                let turns = self
                    .turns
                    .get(route_id)
                    .map(|turns| turns[turns.len().saturating_sub(VISIBLE_TURNS)..].to_vec())
                    .unwrap_or_default();
                (
                    route_id.into(),
                    RouteSnapshot {
                        config: self.route_config(route_id).cloned().unwrap_or_default(),
                        state: active.map_or(RouteState::Stopped, |route| route.state),
                        error: active.map_or("", |route| route.error.as_str()).into(),
                        turns,
                        draft: active
                            .map(|route| route.assembler.draft.clone())
                            .unwrap_or_default(),
                    },
                )
            })
            .collect();
        ControllerSnapshot {
            locale: crate::app_config::resolve_locale(&self.config.locale),
            preferred_locale: self.config.locale.clone(),
            translation_state: self.translation_state,
            elapsed_seconds: self.session_clock.elapsed().as_secs(),
            overlay_visible: self.overlay_visible,
            config: self.config.overlay.clone(),
            audio: self.config.audio.clone(),
            qwen: self.config.qwen.clone(),
            capture: CaptureSnapshot {
                capabilities: CaptureCapabilities {
                    application_capture: cfg!(target_os = "macos"),
                },
                applications: self.capture_applications.clone(),
                microphones: self.capture_microphones.clone(),
                loading: self.capture_loading,
            },
            credentials: credential_snapshot(&self.app),
            routes,
            notice: self.notice.clone(),
        }
    }
}

pub(super) fn credential_snapshot(app: &AppHandle) -> HashMap<String, bool> {
    app.try_state::<CredentialState>()
        .and_then(|state| state.0.lock().ok().map(|store| store.status()))
        .unwrap_or_default()
}
