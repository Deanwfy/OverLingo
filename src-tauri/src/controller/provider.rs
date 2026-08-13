use crate::app_config::{AppConfig, RouteConfig};
use crate::commands::realtime::{Events, ProviderState};
use crate::credentials::CredentialState;
use tauri::{AppHandle, Manager};

pub(super) use crate::commands::realtime::{Event, FragmentKind};

/// Identifies a live session. Opaque so the controller can neither tell nor care which
/// translator opened it.
pub(super) type Handle = u64;

pub(super) fn start(
    app: &AppHandle,
    config: &AppConfig,
    route_id: &str,
    route: &RouteConfig,
    on_event: impl Fn(Event) + Send + Sync + 'static,
) -> Result<Handle, String> {
    let credentials = app
        .state::<CredentialState>()
        .0
        .lock()
        .map_err(|error| error.to_string())?
        .clone();

    crate::translators::open_session(
        config,
        route,
        route_id,
        &credentials,
        Events::callback(on_event),
        &app.state::<ProviderState>(),
    )
}

pub(super) fn send_audio(app: &AppHandle, handle: Handle, audio: Vec<u8>) -> Result<(), String> {
    app.state::<ProviderState>().send_audio(handle, audio)
}

pub(super) fn stop(app: &AppHandle, handle: Handle) {
    app.state::<ProviderState>().stop(handle);
}
