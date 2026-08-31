use super::provider::{self, Handle as ProviderHandle};
use super::state::RouteState;
use super::transcript::TurnAssembler;
use crate::app_config::{AppConfig, RouteConfig};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use tauri::AppHandle;

/// 10 s of 16 kHz mono PCM16 — enough to bridge a reconnect without unbounded growth.
const BACKLOG_LIMIT_BYTES: usize = 320_000;

/// Everything that forces a provider session to be rebuilt. Anything else can be
/// applied without touching the websocket.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ProviderKey {
    engine: String,
    model: String,
    source_language: String,
    target_language: String,
    /// The provider's own settings, opaque here so adding a provider never widens this key.
    settings: String,
}

impl ProviderKey {
    pub(super) fn new(config: &AppConfig, route: &RouteConfig) -> Self {
        Self {
            engine: route.engine.clone(),
            model: route.model.clone(),
            source_language: route.source_language.clone(),
            target_language: route.target_language.clone(),
            settings: crate::translators::settings_fingerprint(&route.engine, config),
        }
    }
}

/// Everything that forces the OS capture stream to be rebuilt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct CaptureKey {
    input: String,
    application: Option<String>,
    microphone_device: Option<String>,
}

impl CaptureKey {
    pub(super) fn new(config: &AppConfig, route: &RouteConfig) -> Self {
        let application = (route.input != "microphone"
            && config.audio.system.scope == "application")
            .then(|| {
                config
                    .audio
                    .system
                    .application
                    .as_ref()
                    .map(|application| application.bundle_id.clone())
            })
            .flatten();
        let microphone_device = (route.input == "microphone")
            .then(|| config.audio.microphone.device.clone())
            .flatten();
        Self {
            input: route.input.clone(),
            application,
            microphone_device,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct Session {
    pub(super) handle: ProviderHandle,
    pub(super) token: u64,
    pub(super) ready: bool,
}

struct BridgeState {
    provider: Option<ProviderHandle>,
    backlog: VecDeque<Vec<u8>>,
    backlog_bytes: usize,
}

impl BridgeState {
    fn buffer(&mut self, audio: Vec<u8>) {
        self.backlog_bytes += audio.len();
        self.backlog.push_back(audio);
        while self.backlog_bytes > BACKLOG_LIMIT_BYTES {
            let Some(dropped) = self.backlog.pop_front() else {
                break;
            };
            self.backlog_bytes -= dropped.len();
        }
    }
}

/// Decouples the capture stream from the provider session: audio captured while no
/// provider is attached is buffered and replayed once one is, so swapping translators
/// or reconnecting never restarts the OS capture.
#[derive(Clone)]
pub(super) struct AudioBridge(Arc<Mutex<BridgeState>>);

impl AudioBridge {
    fn new() -> Self {
        Self(Arc::new(Mutex::new(BridgeState {
            provider: None,
            backlog: VecDeque::new(),
            backlog_bytes: 0,
        })))
    }

    pub(super) fn push(&self, app: &AppHandle, audio: Vec<u8>) -> Result<(), String> {
        let mut state = self.0.lock().map_err(|error| error.to_string())?;
        if let Some(provider) = state.provider {
            return provider::send_audio(app, provider, audio);
        }
        state.buffer(audio);
        Ok(())
    }

    pub(super) fn attach(&self, app: &AppHandle, provider: ProviderHandle) {
        let Ok(mut state) = self.0.lock() else {
            return;
        };
        state.provider = Some(provider);
        state.backlog_bytes = 0;
        for audio in std::mem::take(&mut state.backlog) {
            if provider::send_audio(app, provider, audio).is_err() {
                break;
            }
        }
    }

    pub(super) fn detach(&self) {
        if let Ok(mut state) = self.0.lock() {
            state.provider = None;
        }
    }
}

pub(super) struct ActiveRoute {
    pub(super) config: RouteConfig,
    pub(super) provider_key: ProviderKey,
    pub(super) capture_key: CaptureKey,
    pub(super) state: RouteState,
    pub(super) error: String,
    /// The session that owns the transcript stream.
    pub(super) provider: Option<Session>,
    /// A session being dialled while the current one keeps running.
    pub(super) pending: Option<Session>,
    pub(super) capture: Option<u64>,
    pub(super) capture_ready: bool,
    pub(super) reconnect_attempt: u8,
    pub(super) retry: Option<u64>,
    pub(super) reconfiguring: bool,
    pub(super) bridge: AudioBridge,
    pub(super) assembler: TurnAssembler,
}

impl ActiveRoute {
    pub(super) fn new(
        route_id: &str,
        config: RouteConfig,
        provider_key: ProviderKey,
        capture_key: CaptureKey,
    ) -> Self {
        let assembler = TurnAssembler::new(route_id, &config);
        Self {
            config,
            provider_key,
            capture_key,
            state: RouteState::Connecting,
            error: String::new(),
            provider: None,
            pending: None,
            capture: None,
            capture_ready: false,
            reconnect_attempt: 0,
            retry: None,
            reconfiguring: false,
            bridge: AudioBridge::new(),
            assembler,
        }
    }

    pub(super) fn owns(&self, token: u64) -> bool {
        self.provider.is_some_and(|session| session.token == token)
    }

    pub(super) fn owns_pending(&self, token: u64) -> bool {
        self.pending.is_some_and(|session| session.token == token)
    }

    pub(super) fn is_ready(&self) -> bool {
        self.provider.is_some_and(|session| session.ready)
    }

    pub(super) fn handles(&self) -> impl Iterator<Item = ProviderHandle> {
        [self.provider, self.pending]
            .into_iter()
            .flatten()
            .map(|session| session.handle)
    }

    /// A transcript arrived, so this session is genuinely working and has earned a fresh
    /// retry budget. Deliberately not tied to Live: reaching Live only proves the socket
    /// opened, which a provider that rejects every session also manages.
    pub(super) fn mark_working(&mut self) {
        self.reconnect_attempt = 0;
    }

    /// Live requires both halves: a ready websocket with no audio reaching it is not a
    /// working route. Reaching Live does not refill the retry budget — a provider that
    /// accepts the socket and then drops it would otherwise reconnect forever.
    pub(super) fn refresh_state(&mut self) {
        // A route waiting on a scheduled retry keeps its own label.
        if matches!(self.state, RouteState::Failed | RouteState::Reconnecting)
            && self.provider.is_none()
        {
            return;
        }
        self.state = if self.is_ready() && self.capture_ready {
            self.reconfiguring = false;
            RouteState::Live
        } else if self.reconfiguring {
            RouteState::Reconfiguring
        } else if self.reconnect_attempt > 0 {
            RouteState::Reconnecting
        } else {
            RouteState::Connecting
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn qwen_config() -> AppConfig {
        let mut config = AppConfig::default();
        config.routes.system.engine = "qwen".into();
        config.routes.microphone.engine = "qwen".into();
        config
    }

    fn capturing_zoom(config: &AppConfig) -> AppConfig {
        let mut changed = config.clone();
        changed.audio.system.scope = "application".into();
        changed.audio.system.application = Some(crate::app_config::ApplicationReference {
            bundle_id: "us.zoom.xos".into(),
            name: "Zoom".into(),
        });
        changed
    }

    #[test]
    fn capture_target_does_not_rebuild_the_provider() {
        let config = qwen_config();
        let changed = capturing_zoom(&config);
        assert_eq!(
            ProviderKey::new(&config, &config.routes.system),
            ProviderKey::new(&changed, &changed.routes.system)
        );
        assert_ne!(
            CaptureKey::new(&config, &config.routes.system),
            CaptureKey::new(&changed, &changed.routes.system)
        );
    }

    #[test]
    fn microphone_device_only_rekeys_the_microphone_route() {
        let config = AppConfig::default();
        let mut changed = config.clone();
        changed.audio.microphone.device = Some("USB Microphone".into());

        assert_ne!(
            CaptureKey::new(&config, &config.routes.microphone),
            CaptureKey::new(&changed, &changed.routes.microphone)
        );
        assert_eq!(
            CaptureKey::new(&config, &config.routes.system),
            CaptureKey::new(&changed, &changed.routes.system)
        );
    }

    #[test]
    fn capture_target_never_touches_the_microphone_route() {
        let config = qwen_config();
        let changed = capturing_zoom(&config);
        assert_eq!(
            CaptureKey::new(&config, &config.routes.microphone),
            CaptureKey::new(&changed, &changed.routes.microphone)
        );
    }

    #[test]
    fn language_change_rebuilds_the_provider_only() {
        let config = qwen_config();
        let mut changed = config.clone();
        changed.routes.system.target_language = "ja".into();
        assert_ne!(
            ProviderKey::new(&config, &config.routes.system),
            ProviderKey::new(&changed, &changed.routes.system)
        );
        assert_eq!(
            CaptureKey::new(&config, &config.routes.system),
            CaptureKey::new(&changed, &changed.routes.system)
        );
    }

    #[test]
    fn qwen_settings_leave_openai_routes_alone() {
        let mut config = qwen_config();
        config.routes.microphone.engine = "openai".into();
        let mut changed = config.clone();
        changed.qwen.workspace_id = "ws-123".into();
        assert_ne!(
            ProviderKey::new(&config, &config.routes.system),
            ProviderKey::new(&changed, &changed.routes.system)
        );
        assert_eq!(
            ProviderKey::new(&config, &config.routes.microphone),
            ProviderKey::new(&changed, &changed.routes.microphone)
        );
    }

    fn active_route() -> ActiveRoute {
        let config = qwen_config();
        let route = config.routes.system.clone();
        ActiveRoute::new(
            "system",
            route.clone(),
            ProviderKey::new(&config, &route),
            CaptureKey::new(&config, &route),
        )
    }

    fn session(token: u64, ready: bool) -> Session {
        Session {
            handle: token,
            token,
            ready,
        }
    }

    #[test]
    fn a_pending_session_never_answers_for_the_live_one() {
        let mut route = active_route();
        route.provider = Some(session(1, true));
        route.pending = Some(session(2, false));

        assert!(route.owns(1));
        assert!(!route.owns_pending(1));
        assert!(route.owns_pending(2));
        assert!(!route.owns(2));
        // A make-before-break handover must stop both sockets, not just the live one.
        assert_eq!(route.handles().count(), 2);
    }

    #[test]
    fn a_route_is_live_only_with_both_halves() {
        let mut route = active_route();
        route.provider = Some(session(1, true));
        route.refresh_state();
        assert_eq!(route.state, RouteState::Connecting);

        route.capture_ready = true;
        route.refresh_state();
        assert_eq!(route.state, RouteState::Live);
    }

    /// A key that is rejected still opens a socket, so refilling the budget on Live would
    /// let it reconnect for ever. Only a delivered transcript proves the session works.
    #[test]
    fn only_a_delivered_fragment_refills_the_retry_budget() {
        let mut route = active_route();
        route.reconnect_attempt = 2;
        route.reconfiguring = true;
        route.provider = Some(session(1, true));
        route.capture_ready = true;
        route.refresh_state();

        assert_eq!(route.state, RouteState::Live);
        assert!(!route.reconfiguring);
        assert_eq!(route.reconnect_attempt, 2);

        route.mark_working();
        assert_eq!(route.reconnect_attempt, 0);
    }

    /// A route between backoff attempts owns no session; recomputing must not relabel it
    /// as merely connecting and hide the failure.
    #[test]
    fn a_route_awaiting_retry_keeps_its_label() {
        for label in [RouteState::Failed, RouteState::Reconnecting] {
            let mut route = active_route();
            route.state = label;
            route.refresh_state();
            assert_eq!(route.state, label);
        }
    }

    #[test]
    fn a_reconfiguring_route_is_not_reported_as_reconnecting() {
        let mut route = active_route();
        route.reconfiguring = true;
        route.reconnect_attempt = 1;
        route.provider = Some(session(1, false));
        route.refresh_state();
        assert_eq!(route.state, RouteState::Reconfiguring);
    }

    #[test]
    fn backlog_keeps_the_newest_audio_within_the_limit() {
        let bridge = AudioBridge::new();
        let mut state = bridge.0.lock().unwrap();
        for marker in 0..100_u8 {
            state.buffer(vec![marker; 12_800]);
        }
        assert!(state.backlog_bytes <= BACKLOG_LIMIT_BYTES);
        assert_eq!(
            state.backlog_bytes,
            state.backlog.iter().map(Vec::len).sum::<usize>()
        );
        assert_eq!(state.backlog.back().unwrap()[0], 99);
    }
}
