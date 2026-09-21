use crate::app_config::{AppConfig, RouteConfig};
use crate::audio::CapturableApplication;
use crate::geometry::Rect;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::ipc::Channel;
use tauri::{AppHandle, State};
use tokio::sync::mpsc;

mod clock;
mod journal;
mod model;
mod provider;
mod route;
mod session;
mod settings;
mod snapshot;
mod state;
mod transcript;

use clock::SessionClock;
use journal::Journal;
use model::{ControllerNotice, ControllerSnapshot, OverlaySettingsPatch, TranslationCommand};
use provider::Event as ProviderEvent;
use route::ActiveRoute;
use state::TranslationState;
use transcript::TranscriptTurn;

pub use model::ControllerRequest;

const SYSTEM_ROUTE: &str = "system";
const MICROPHONE_ROUTE: &str = "microphone";
const ROUTE_IDS: [&str; 2] = [SYSTEM_ROUTE, MICROPHONE_ROUTE];

/// Managed before the windows exist, since their pages subscribe as soon as they load,
/// which can be while setup is still busy; the actor is started once setup has what it
/// needs, and anything sent before then waits in the channel.
#[derive(Clone)]
pub struct AppController {
    sender: mpsc::UnboundedSender<Action>,
    /// `None` until the actor's first publish, which reaches every surface subscribed so far.
    snapshot: Arc<Mutex<Option<ControllerSnapshot>>>,
    subscribers: Arc<Mutex<HashMap<String, Channel<ControllerSnapshot>>>>,
}

impl AppController {
    pub fn new() -> (Self, mpsc::UnboundedReceiver<Action>) {
        let (sender, receiver) = mpsc::unbounded_channel();
        let controller = Self {
            sender,
            snapshot: Arc::new(Mutex::new(None)),
            subscribers: Arc::new(Mutex::new(HashMap::new())),
        };
        (controller, receiver)
    }

    pub fn start(
        &self,
        app: AppHandle,
        config: AppConfig,
        receiver: mpsc::UnboundedReceiver<Action>,
    ) {
        let actor = ControllerActor::new(
            app,
            config,
            self.sender.clone(),
            self.snapshot.clone(),
            self.subscribers.clone(),
        );
        tauri::async_runtime::spawn(actor.run(receiver));
    }

    /// The user finished dragging the subtitle box.
    pub fn overlay_frame_changed(&self, frame: Rect) {
        let _ = self.sender.send(Action::OverlayFrame(frame));
    }

    #[cfg(target_os = "macos")]
    pub fn overlay_frame(&self) -> Option<Rect> {
        self.snapshot
            .lock()
            .ok()
            .and_then(|snapshot| snapshot.as_ref()?.config.frame)
    }

    pub fn request(&self, request: ControllerRequest) -> Result<(), String> {
        self.sender
            .send(Action::Request(request))
            .map_err(|_| "Application controller is unavailable".into())
    }

    /// Surfaces pick up the new set of usable translators, and a cleared key frees its routes.
    pub fn credentials_changed(&self, provider: &str, present: bool) {
        let _ = self.sender.send(Action::CredentialsChanged {
            provider: provider.to_owned(),
            present,
        });
    }

    fn subscribe(
        &self,
        surface: String,
        on_event: Channel<ControllerSnapshot>,
    ) -> Result<(), String> {
        let snapshot = self
            .snapshot
            .lock()
            .map_err(|error| error.to_string())?
            .clone();
        if let Some(snapshot) = snapshot {
            on_event.send(snapshot).map_err(|error| error.to_string())?;
        }
        self.subscribers
            .lock()
            .map_err(|error| error.to_string())?
            .insert(surface, on_event);
        Ok(())
    }
}

#[tauri::command]
pub fn controller_action(
    request: ControllerRequest,
    controller: State<'_, AppController>,
) -> Result<(), String> {
    controller.request(request)
}

#[tauri::command]
pub fn subscribe_controller(
    surface: String,
    on_event: Channel<ControllerSnapshot>,
    controller: State<'_, AppController>,
) -> Result<(), String> {
    if !matches!(surface.as_str(), "main" | "overlay" | "toolbar" | "panel") {
        return Err("Invalid controller surface".into());
    }
    controller.subscribe(surface, on_event)
}

pub(crate) enum Action {
    Request(ControllerRequest),
    Provider {
        route_id: String,
        token: u64,
        event: ProviderEvent,
    },
    AudioFailed {
        route_id: String,
        token: u64,
        error: String,
    },
    CaptureStarted {
        route_id: String,
        token: u64,
        result: Result<(), String>,
    },
    Reconnect {
        route_id: String,
        retry: u64,
    },
    Flush {
        route_id: String,
        token: u64,
        version: u64,
    },
    CaptureOptionsLoaded(Result<(Vec<CapturableApplication>, Vec<String>), String>),
    CredentialsChanged {
        provider: String,
        present: bool,
    },
    Tick(u64),
    SaveConfig(u64),
    OverlayFrame(Rect),
}

/// Owns all mutable session state. Every mutation arrives as an `Action` on one channel,
/// so nothing here needs locking and the ordering of route events is deterministic.
struct ControllerActor {
    app: AppHandle,
    sender: mpsc::UnboundedSender<Action>,
    snapshot: Arc<Mutex<Option<ControllerSnapshot>>>,
    subscribers: Arc<Mutex<HashMap<String, Channel<ControllerSnapshot>>>>,
    config: AppConfig,
    translation_state: TranslationState,
    overlay_visible: bool,
    routes: HashMap<String, ActiveRoute>,
    turns: HashMap<String, Vec<TranscriptTurn>>,
    capture_applications: Vec<CapturableApplication>,
    capture_microphones: Vec<String>,
    capture_loading: bool,
    session_clock: SessionClock,
    journal: Option<Journal>,
    /// Hands out the tokens that let stale provider, capture and retry callbacks be ignored.
    sequence: u64,
    session_generation: u64,
    save_generation: u64,
    notice: Option<ControllerNotice>,
}

impl ControllerActor {
    fn new(
        app: AppHandle,
        config: AppConfig,
        sender: mpsc::UnboundedSender<Action>,
        snapshot: Arc<Mutex<Option<ControllerSnapshot>>>,
        subscribers: Arc<Mutex<HashMap<String, Channel<ControllerSnapshot>>>>,
    ) -> Self {
        Self {
            app,
            sender,
            snapshot,
            subscribers,
            config,
            translation_state: TranslationState::Stopped,
            overlay_visible: false,
            routes: HashMap::new(),
            turns: HashMap::new(),
            capture_applications: Vec::new(),
            capture_microphones: Vec::new(),
            capture_loading: false,
            session_clock: SessionClock::default(),
            journal: None,
            sequence: 0,
            session_generation: 0,
            save_generation: 0,
            notice: None,
        }
    }

    async fn run(mut self, mut receiver: mpsc::UnboundedReceiver<Action>) {
        // The box comes back if it was showing at the last quit.
        if self.config.overlay.enabled {
            self.set_overlay_visible(true, false);
        }
        self.publish();
        while let Some(action) = receiver.recv().await {
            match action {
                Action::Request(request) => self.handle_request(request),
                Action::Provider {
                    route_id,
                    token,
                    event,
                } => self.handle_provider_event(&route_id, token, event),
                Action::AudioFailed {
                    route_id,
                    token,
                    error,
                } => self.fail_capture(&route_id, token, error),
                Action::CaptureStarted {
                    route_id,
                    token,
                    result,
                } => self.capture_started(&route_id, token, result),
                Action::Reconnect { route_id, retry } => self.reconnect(&route_id, retry),
                Action::Flush {
                    route_id,
                    token,
                    version,
                } => self.flush_route(&route_id, token, version),
                Action::CaptureOptionsLoaded(result) => {
                    self.capture_loading = false;
                    match result {
                        Ok((applications, microphones)) => {
                            self.capture_applications = applications;
                            self.capture_microphones = microphones;
                        }
                        Err(error) => self.set_notice(None, error),
                    }
                    self.publish();
                }
                Action::CredentialsChanged { provider, present } => {
                    self.credentials_changed(&provider, present)
                }
                Action::Tick(generation) => self.tick(generation),
                Action::SaveConfig(generation) => {
                    if generation == self.save_generation {
                        self.save_config();
                    }
                }
                Action::OverlayFrame(frame) => {
                    self.update_overlay_settings(OverlaySettingsPatch::frame(frame))
                }
            }
        }
    }

    fn handle_request(&mut self, request: ControllerRequest) {
        match request {
            ControllerRequest::Translation { command } => match command {
                TranslationCommand::Start => self.start_translation(),
                TranslationCommand::Stop => self.stop_translation(),
                TranslationCommand::Pause => self.pause_translation(),
                TranslationCommand::Resume => self.resume_translation(),
            },
            ControllerRequest::ToggleTranslation => {
                if self.translation_state.is_active() {
                    self.stop_translation();
                } else {
                    self.start_translation();
                }
            }
            ControllerRequest::ToggleOverlay => {
                self.set_overlay_visible(!self.overlay_visible, true)
            }
            ControllerRequest::ShowOverlay { visible } => self.set_overlay_visible(visible, true),
            ControllerRequest::Hide => self.set_overlay_visible(false, true),
            ControllerRequest::Settings { patch } => self.update_overlay_settings(patch),
            ControllerRequest::Route { route_id, enabled } => {
                self.update_route_enabled(&route_id, enabled)
            }
            ControllerRequest::RetryRoute { route_id } => self.retry_route(&route_id),
            ControllerRequest::RouteSettings { route_id, patch } => {
                self.update_route_settings(&route_id, patch)
            }
            ControllerRequest::QwenSettings { patch } => self.update_qwen_settings(patch),
            ControllerRequest::Locale { locale } => self.update_locale(&locale),
            ControllerRequest::Capture { bundle_id } => self.update_capture(bundle_id),
            ControllerRequest::MicrophoneDevice { device } => self.update_microphone_device(device),
            ControllerRequest::RequestCaptureOptions => self.load_capture_options(),
            ControllerRequest::Exit => {
                self.stop_translation();
                self.save_config();
                crate::exit_now(&self.app);
            }
        }
    }

    fn start_translation(&mut self) {
        if self.translation_state.is_active() {
            return;
        }
        if let Some(code) = self.validate_start() {
            self.set_notice(Some(code), String::new());
            let _ = crate::shell::show_settings(&self.app);
            self.publish();
            return;
        }

        // Whatever ran before ends here, failed or not, and its record is closed out on the
        // old clock before the new session resets it.
        self.stop_routes();
        let save_error = self.close_journal();
        self.turns.clear();
        self.notice = None;
        self.translation_state = TranslationState::Starting;
        self.session_clock.start();
        self.journal = Some(Journal::new(&self.config));
        self.open_enabled_routes();
        self.set_overlay_visible(true, true);
        if let Some(error) = save_error {
            self.set_notice(None, error);
        }
        self.publish();
    }

    /// The only way a journal leaves the actor: whoever drops it stamps the duration first,
    /// so a record's total can never disagree with the timestamps inside it. Returns the
    /// error that kept it from being written, if any.
    fn close_journal(&mut self) -> Option<String> {
        let elapsed = self.session_clock.elapsed();
        let mut journal = self.journal.take()?;
        journal.finish(elapsed);
        journal.persist(&self.app).err()
    }

    fn stop_translation(&mut self) {
        if self.translation_state == TranslationState::Stopped && self.routes.is_empty() {
            return;
        }
        self.stop_routes();
        let save_error = self.close_journal();
        self.translation_state = TranslationState::Stopped;
        self.session_clock.reset();
        self.session_generation = self.session_generation.wrapping_add(1);
        self.turns.clear();
        if let Some(error) = save_error {
            self.set_notice(None, error);
        }
        self.publish();
    }

    fn pause_translation(&mut self) {
        if self.translation_state != TranslationState::Running {
            return;
        }
        self.session_clock.pause();
        self.session_generation = self.session_generation.wrapping_add(1);
        self.stop_routes();
        self.translation_state = TranslationState::Paused;
        self.publish();
    }

    fn resume_translation(&mut self) {
        if self.translation_state != TranslationState::Paused {
            return;
        }
        self.session_clock.resume();
        self.translation_state = TranslationState::Starting;
        self.open_enabled_routes();
        self.publish();
    }

    fn open_enabled_routes(&mut self) {
        self.session_generation = self.session_generation.wrapping_add(1);
        for route_id in self.enabled_route_ids() {
            self.open_route(route_id);
        }
        self.schedule_tick();
    }

    /// Slider drags patch the config many times a second; the file is written once they settle.
    fn schedule_save(&mut self) {
        self.save_generation = self.save_generation.wrapping_add(1);
        let sender = self.sender.clone();
        let generation = self.save_generation;
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            let _ = sender.send(Action::SaveConfig(generation));
        });
    }

    /// Drives the elapsed-time readout while a session is open.
    fn schedule_tick(&self) {
        let sender = self.sender.clone();
        let generation = self.session_generation;
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let _ = sender.send(Action::Tick(generation));
        });
    }

    fn tick(&mut self, generation: u64) {
        if generation != self.session_generation || !self.translation_state.is_timed() {
            return;
        }
        self.publish();
        self.schedule_tick();
    }

    fn next_sequence(&mut self) -> u64 {
        self.sequence = self.sequence.wrapping_add(1);
        self.sequence
    }

    fn set_notice(&mut self, code: Option<&str>, message: String) {
        let id = self.next_sequence();
        self.notice = Some(ControllerNotice {
            id,
            code: code.map(str::to_owned),
            message,
        });
    }
}

fn route_config<'a>(config: &'a AppConfig, route_id: &str) -> Option<&'a RouteConfig> {
    match route_id {
        SYSTEM_ROUTE => Some(&config.routes.system),
        MICROPHONE_ROUTE => Some(&config.routes.microphone),
        _ => None,
    }
}

fn route_config_mut<'a>(config: &'a mut AppConfig, route_id: &str) -> Option<&'a mut RouteConfig> {
    match route_id {
        SYSTEM_ROUTE => Some(&mut config.routes.system),
        MICROPHONE_ROUTE => Some(&mut config.routes.microphone),
        _ => None,
    }
}

fn enabled_route_ids(config: &AppConfig) -> Vec<&'static str> {
    ROUTE_IDS
        .into_iter()
        .filter(|route_id| route_config(config, route_id).is_some_and(|route| route.enabled))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::provider::FragmentKind;
    use super::transcript::TurnAssembler;
    use super::*;

    #[test]
    fn pairs_final_fragments_in_order() {
        let route = AppConfig::default().routes.system;
        let mut assembler = TurnAssembler::new(SYSTEM_ROUTE, &route);
        assembler.push(FragmentKind::Original, "one".into(), true);
        let (outputs, _) = assembler.push(FragmentKind::Translation, "一".into(), true);
        assert_eq!(outputs.first().unwrap().translation, "一");
    }

    /// Fragments are snapshots, so the newest one replaces the draft whatever the provider.
    #[test]
    fn draft_tracks_the_latest_snapshot() {
        let base = AppConfig::default().routes.system;
        for engine in ["qwen", "openai"] {
            let route = RouteConfig {
                engine: engine.into(),
                ..base.clone()
            };
            let mut assembler = TurnAssembler::new(SYSTEM_ROUTE, &route);
            assembler.push(FragmentKind::Translation, "hel".into(), false);
            assembler.push(FragmentKind::Translation, "hello".into(), false);
            assert_eq!(assembler.draft.translation, "hello", "{engine}");
        }
    }

    #[test]
    fn accepts_frontend_route_settings_contract() {
        let request: ControllerRequest = serde_json::from_value(serde_json::json!({
            "type": "routeSettings",
            "routeId": "system",
            "patch": { "sourceLanguage": "ja", "targetLanguage": "ko" }
        }))
        .unwrap();
        let ControllerRequest::RouteSettings { route_id, patch } = request else {
            panic!("unexpected request");
        };
        assert_eq!(route_id, "system");
        assert_eq!(patch.source_language.as_deref(), Some("ja"));
        assert_eq!(patch.target_language.as_deref(), Some("ko"));
    }

    #[test]
    fn accepts_frontend_capture_contract() {
        let request: ControllerRequest = serde_json::from_value(serde_json::json!({
            "type": "capture",
            "bundleId": "us.zoom.xos"
        }))
        .unwrap();
        let ControllerRequest::Capture { bundle_id } = request else {
            panic!("unexpected request");
        };
        assert_eq!(bundle_id, "us.zoom.xos");
    }
}
