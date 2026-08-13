use super::provider::{self, Event as ProviderEvent, FragmentKind};
use super::route::{ActiveRoute, CaptureKey, ProviderKey, Session};
use super::state::{ClockChange, RouteState, TranslationState};
use super::transcript::{TranscriptTurn, TurnAssembler};
use super::{Action, ControllerActor, MICROPHONE_ROUTE, ROUTE_IDS};
use crate::commands::audio::{AudioSink, AudioState, CaptureRequest};
use crate::diagnostics;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::Manager;

const MAX_RECONNECT_ATTEMPTS: u8 = 3;
/// A flat wait rather than a growing one: the failures worth retrying are blips, and a
/// session that has already dropped out should come back at a predictable pace.
const RECONNECT_DELAY: Duration = Duration::from_secs(2);
/// How long an unpaired original or translation waits for its counterpart.
const FLUSH_DELAY: Duration = Duration::from_millis(4_500);

impl ControllerActor {
    /// Brings a route up, reusing whatever is already healthy: an existing capture stream
    /// is never restarted just because the provider session has to be rebuilt.
    pub(super) fn open_route(&mut self, route_id: &str) {
        let Some(config) = self.route_config(route_id).cloned() else {
            return;
        };
        if !config.enabled {
            return;
        }
        let provider_key = ProviderKey::new(&self.config, &config);
        let capture_key = CaptureKey::new(&self.config, &config);
        match self.routes.get_mut(route_id) {
            Some(route) => {
                route.config = config;
                route.provider_key = provider_key;
                route.capture_key = capture_key;
                route.error.clear();
            }
            None => {
                self.routes.insert(
                    route_id.into(),
                    ActiveRoute::new(route_id, config, provider_key, capture_key),
                );
            }
        }

        if self.routes[route_id].capture.is_none() {
            self.start_capture(route_id);
        }
        if self.routes[route_id].provider.is_none() {
            self.start_provider(route_id, false);
        }
        self.refresh_route_state(route_id);
        self.sync_translation_state();
        self.publish();
    }

    pub(super) fn stop_route(&mut self, route_id: &str) {
        let Some(mut route) = self.routes.remove(route_id) else {
            return;
        };
        self.app.state::<AudioState>().stop_capture(route_id);
        for handle in route.handles().collect::<Vec<_>>() {
            provider::stop(&self.app, handle);
        }
        route.bridge.detach();
        let outputs = route.assembler.flush_all();
        self.record_turns(route_id, outputs);
    }

    pub(super) fn stop_routes(&mut self) {
        for route_id in self.routes.keys().cloned().collect::<Vec<_>>() {
            self.stop_route(&route_id);
        }
    }

    /// Applies a settings change to the running session route by route. Only what actually
    /// changed is rebuilt — an untouched route keeps its websocket and capture stream.
    pub(super) fn reconfigure_if_active(&mut self) {
        if !matches!(
            self.translation_state,
            TranslationState::Starting | TranslationState::Running | TranslationState::Failed
        ) {
            return;
        }
        for route_id in ROUTE_IDS {
            if !self
                .route_config(route_id)
                .is_some_and(|route| route.enabled)
            {
                self.stop_route(route_id);
            } else if self.route_config_error(route_id).is_some() {
                // A half-edited route keeps whatever session it already has; swapping to a
                // combination the translator cannot serve would only kill working subtitles.
            } else if self.routes.contains_key(route_id) {
                self.refresh_route(route_id);
            } else {
                self.open_route(route_id);
            }
        }
        self.sync_translation_state();
    }

    fn refresh_route(&mut self, route_id: &str) {
        let Some(config) = self.route_config(route_id).cloned() else {
            return;
        };
        let provider_key = ProviderKey::new(&self.config, &config);
        let capture_key = CaptureKey::new(&self.config, &config);
        let Some(route) = self.routes.get_mut(route_id) else {
            return;
        };
        if route.state == RouteState::Failed {
            route.reconnect_attempt = 0;
            route.state = RouteState::Connecting;
            self.open_route(route_id);
            return;
        }
        let provider_changed = route.provider_key != provider_key;
        let capture_changed = route.capture_key != capture_key;
        if !provider_changed && !capture_changed {
            return;
        }
        route.config = config;
        route.provider_key = provider_key;
        route.capture_key = capture_key;
        route.reconfiguring = route.state == RouteState::Live;

        if capture_changed {
            self.stop_capture(route_id);
            self.start_capture(route_id);
        }
        if provider_changed {
            self.swap_provider(route_id);
        }
        self.refresh_route_state(route_id);
        self.publish();
    }

    fn swap_provider(&mut self, route_id: &str) {
        let Some(route) = self.routes.get_mut(route_id) else {
            return;
        };
        // Only a session that is actually producing transcripts is worth keeping alive
        // during the handover; a half-open one is cheaper to discard.
        let handover = route.is_ready();
        let mut retired = Vec::from_iter(route.pending.take());
        if !handover {
            retired.extend(route.provider.take());
            route.bridge.detach();
        }
        for session in retired {
            provider::stop(&self.app, session.handle);
        }
        self.start_provider(route_id, handover);
    }

    /// `replace` dials a second session while the current one keeps producing transcripts,
    /// so a settings change costs no dead air (make-before-break).
    fn start_provider(&mut self, route_id: &str, replace: bool) {
        let Some(route) = self.routes.get(route_id) else {
            return;
        };
        let config = route.config.clone();
        let token = self.next_sequence();

        let sender = self.sender.clone();
        let route_key = route_id.to_string();
        let started = provider::start(&self.app, &self.config, route_id, &config, move |event| {
            let _ = sender.send(Action::Provider {
                route_id: route_key.clone(),
                token,
                event,
            });
        });

        match started {
            Ok(handle) => {
                diagnostics::log(
                    "route",
                    format!(
                        "provider_dialling route={route_id} engine={}",
                        diagnostics::field(&config.engine)
                    ),
                );
                let session = Session {
                    handle,
                    token,
                    ready: false,
                };
                if let Some(route) = self.routes.get_mut(route_id) {
                    if replace {
                        route.pending = Some(session);
                        route.reconfiguring = true;
                    } else {
                        route.provider = Some(session);
                    }
                }
            }
            Err(error) => {
                if let Some(route) = self.routes.get_mut(route_id) {
                    route.pending = None;
                }
                self.tear_down_route(route_id, error, false);
            }
        }
    }

    pub(super) fn handle_provider_event(
        &mut self,
        route_id: &str,
        token: u64,
        event: ProviderEvent,
    ) {
        let Some(route) = self.routes.get(route_id) else {
            return;
        };
        // A token that owns neither slot belongs to a session already replaced or torn down.
        let pending = route.owns_pending(token);
        if !route.owns(token) && !pending {
            return;
        }
        match event {
            ProviderEvent::Ready if pending => self.promote_pending(route_id, token),
            ProviderEvent::Ready => self.provider_ready(route_id),
            ProviderEvent::Fragment {
                kind,
                text,
                final_fragment,
            } if !pending => self.push_fragment(route_id, token, kind, text, final_fragment),
            ProviderEvent::Fragment { .. } => {}
            ProviderEvent::Error { message, .. } | ProviderEvent::Closed(message) if pending => {
                self.fail_pending(route_id, token, message)
            }
            ProviderEvent::Error { message, retryable } => {
                self.fail_route(route_id, token, message, retryable)
            }
            ProviderEvent::Closed(reason) => self.fail_route(route_id, token, reason, true),
        }
    }

    fn provider_ready(&mut self, route_id: &str) {
        let Some(route) = self.routes.get_mut(route_id) else {
            return;
        };
        let Some(session) = route.provider.as_mut() else {
            return;
        };
        session.ready = true;
        let handle = session.handle;
        route.error.clear();
        diagnostics::log("route", format!("provider_ready route={route_id}"));
        route.bridge.attach(&self.app, handle);
        self.refresh_route_state(route_id);
        self.sync_translation_state();
        self.publish();
    }

    fn promote_pending(&mut self, route_id: &str, token: u64) {
        let Some(route) = self.routes.get_mut(route_id) else {
            return;
        };
        let Some(mut session) = route.pending.take() else {
            return;
        };
        session.ready = true;
        session.token = token;
        let retired = route.provider.replace(session);
        route.bridge.attach(&self.app, session.handle);
        // The retired session's turns carry the old languages, so close them out before
        // the assembler adopts the new route config.
        let outputs = route.assembler.flush_all();
        route.assembler = TurnAssembler::new(route_id, &route.config);
        route.reconfiguring = false;
        route.reconnect_attempt = 0;
        route.error.clear();
        if let Some(retired) = retired {
            provider::stop(&self.app, retired.handle);
        }

        self.record_turns(route_id, outputs);
        self.refresh_route_state(route_id);
        self.sync_translation_state();
        self.publish();
    }

    /// The replacement session never came up. Drop it and fail the route so the normal
    /// backoff path rebuilds it with the current settings.
    fn fail_pending(&mut self, route_id: &str, token: u64, error: String) {
        let Some(route) = self.routes.get_mut(route_id) else {
            return;
        };
        if !route.owns_pending(token) {
            return;
        }
        let pending = route.pending.take();
        route.reconfiguring = false;
        if let Some(session) = pending {
            provider::stop(&self.app, session.handle);
        }
        self.tear_down_route(route_id, error, true);
    }

    fn start_capture(&mut self, route_id: &str) {
        let Some(route) = self.routes.get(route_id) else {
            return;
        };
        let capture = if route.config.input == MICROPHONE_ROUTE {
            CaptureRequest::Microphone
        } else {
            CaptureRequest::System {
                application_bundle_id: self.captured_application(),
            }
        };
        let bridge = route.bridge.clone();
        let token = self.next_sequence();
        if let Some(route) = self.routes.get_mut(route_id) {
            route.capture = Some(token);
            route.capture_ready = false;
        }

        let app = self.app.clone();
        let sender = self.sender.clone();
        let route_key = route_id.to_string();
        let failed = Arc::new(AtomicBool::new(false));
        let sink = AudioSink::callback({
            let sender = sender.clone();
            let route_key = route_key.clone();
            let failed = failed.clone();
            move |audio| {
                if let Err(error) = bridge.push(&app, audio) {
                    if !failed.swap(true, Ordering::SeqCst) {
                        let _ = sender.send(Action::AudioFailed {
                            route_id: route_key.clone(),
                            token,
                            error,
                        });
                    }
                }
            }
        });
        // Shares the sink's dedupe flag: whichever of "push failed" and "stream died"
        // fires first reports the capture, the other stays quiet.
        let on_failed = Box::new({
            let sender = sender.clone();
            let route_key = route_key.clone();
            move |error: String| {
                if !failed.swap(true, Ordering::SeqCst) {
                    let _ = sender.send(Action::AudioFailed {
                        route_id: route_key,
                        token,
                        error,
                    });
                }
            }
        });
        self.app.state::<AudioState>().start_capture(
            route_id.into(),
            capture,
            sink,
            Box::new(move |result| {
                let _ = sender.send(Action::CaptureStarted {
                    route_id: route_key,
                    token,
                    result,
                });
            }),
            on_failed,
        );
    }

    fn stop_capture(&mut self, route_id: &str) {
        self.app.state::<AudioState>().stop_capture(route_id);
        if let Some(route) = self.routes.get_mut(route_id) {
            route.capture = None;
            route.capture_ready = false;
        }
    }

    pub(super) fn capture_started(
        &mut self,
        route_id: &str,
        token: u64,
        result: Result<(), String>,
    ) {
        let Some(route) = self.routes.get_mut(route_id) else {
            return;
        };
        if route.capture != Some(token) {
            return;
        }
        match result {
            Ok(()) => {
                route.capture_ready = true;
                diagnostics::log("route", format!("capture_ready route={route_id}"));
                self.refresh_route_state(route_id);
                self.sync_translation_state();
                self.publish();
            }
            Err(error) => {
                route.capture = None;
                self.tear_down_route(route_id, error, false);
            }
        }
    }

    /// Retryable: a capture that dies mid-session is usually a device changing mode or
    /// owner (a Bluetooth headset flipping profiles), which a rebuild fixes. A device
    /// that is genuinely gone fails the rebuild itself, and that path is what gives up.
    pub(super) fn fail_capture(&mut self, route_id: &str, token: u64, error: String) {
        if self.routes.get(route_id).and_then(|route| route.capture) != Some(token) {
            return;
        }
        self.stop_capture(route_id);
        self.tear_down_route(route_id, error, true);
    }

    fn fail_route(&mut self, route_id: &str, token: u64, error: String, retryable: bool) {
        if self
            .routes
            .get(route_id)
            .is_some_and(|route| route.owns(token))
        {
            self.tear_down_route(route_id, error, retryable);
        }
    }

    /// Drops every session on the route and either schedules a retry or gives up. The
    /// capture stream stays up so a reconnect costs only the websocket handshake.
    fn tear_down_route(&mut self, route_id: &str, error: String, retryable: bool) {
        if matches!(
            self.translation_state,
            TranslationState::Stopped | TranslationState::Paused
        ) {
            return;
        }
        let Some(route) = self.routes.get_mut(route_id) else {
            return;
        };
        let retired = route.handles().collect::<Vec<_>>();
        route.provider = None;
        route.pending = None;
        route.reconfiguring = false;
        route.error = error.clone();
        route.bridge.detach();
        let retrying = retryable && route.reconnect_attempt < MAX_RECONNECT_ATTEMPTS;
        if retrying {
            route.reconnect_attempt += 1;
            route.state = RouteState::Reconnecting;
        } else {
            route.state = RouteState::Failed;
        }
        diagnostics::log(
            "route",
            format!(
                "torn_down route={route_id} attempt={} retrying={retrying} error={}",
                route.reconnect_attempt,
                diagnostics::field(&error)
            ),
        );
        for handle in retired {
            provider::stop(&self.app, handle);
        }

        if retrying {
            let retry = self.next_sequence();
            if let Some(route) = self.routes.get_mut(route_id) {
                route.retry = Some(retry);
            }
            self.schedule_reconnect(route_id, retry);
        } else {
            self.stop_capture(route_id);
            self.set_notice(None, error);
        }
        self.sync_translation_state();
        self.publish();
    }

    /// The user asking again after the automatic attempts ran out. Clears the budget so a
    /// blip that outlasted them can still recover without restarting the whole session.
    pub(super) fn retry_route(&mut self, route_id: &str) {
        if !self.translation_state.is_active() {
            return;
        }
        let Some(route) = self.routes.get_mut(route_id) else {
            return;
        };
        if route.state != RouteState::Failed {
            return;
        }
        route.reconnect_attempt = 0;
        route.state = RouteState::Connecting;
        self.notice = None;
        self.open_route(route_id);
    }

    pub(super) fn reconnect(&mut self, route_id: &str, retry: u64) {
        if self.routes.get(route_id).and_then(|route| route.retry) == Some(retry) {
            self.open_route(route_id);
        }
    }

    fn schedule_reconnect(&self, route_id: &str, retry: u64) {
        let sender = self.sender.clone();
        let route_id = route_id.to_string();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(RECONNECT_DELAY).await;
            let _ = sender.send(Action::Reconnect { route_id, retry });
        });
    }

    fn refresh_route_state(&mut self, route_id: &str) {
        if let Some(route) = self.routes.get_mut(route_id) {
            route.refresh_state();
        }
    }

    pub(super) fn sync_translation_state(&mut self) {
        let states = self
            .routes
            .values()
            .map(|route| route.state)
            .collect::<Vec<_>>();
        let next = self.translation_state.aggregated(&states);
        match self.translation_state.clock_change(next) {
            ClockChange::Hold => self.session_clock.pause(),
            ClockChange::Continue => self.session_clock.resume(),
            ClockChange::None => {}
        }
        self.translation_state = next;
    }

    fn push_fragment(
        &mut self,
        route_id: &str,
        token: u64,
        kind: FragmentKind,
        text: String,
        final_fragment: bool,
    ) {
        let Some(route) = self.routes.get_mut(route_id) else {
            return;
        };
        route.mark_working();
        let (outputs, flush_version) = route.assembler.push(kind, text, final_fragment);
        if let Some(version) = flush_version {
            let sender = self.sender.clone();
            let route_id = route_id.to_string();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(FLUSH_DELAY).await;
                let _ = sender.send(Action::Flush {
                    route_id,
                    token,
                    version,
                });
            });
        }
        self.record_turns(route_id, outputs);
        self.publish();
    }

    pub(super) fn flush_route(&mut self, route_id: &str, token: u64, version: u64) {
        let outputs = self
            .routes
            .get_mut(route_id)
            .filter(|route| route.owns(token))
            .map(|route| route.assembler.flush(version))
            .unwrap_or_default();
        self.record_turns(route_id, outputs);
        self.publish();
    }

    fn record_turns(&mut self, route_id: &str, outputs: Vec<TranscriptTurn>) {
        for turn in outputs {
            self.turns
                .entry(route_id.into())
                .or_default()
                .push(turn.clone());
            if let Some(journal) = self.journal.as_mut() {
                journal.add(turn, self.session_clock.elapsed());
                if let Err(error) = journal.persist(&self.app) {
                    self.set_notice(None, error);
                }
            }
        }
    }
}
