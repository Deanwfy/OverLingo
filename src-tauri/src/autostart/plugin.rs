use super::AutostartStatus;
use tauri::{AppHandle, Runtime};
use tauri_plugin_autostart::ManagerExt;

fn status_of(enabled: bool) -> AutostartStatus {
    if enabled {
        AutostartStatus::Enabled
    } else {
        AutostartStatus::Disabled
    }
}

pub fn status<R: Runtime>(app: &AppHandle<R>) -> Result<AutostartStatus, String> {
    let enabled = app
        .autolaunch()
        .is_enabled()
        .map_err(|error| error.to_string())?;
    Ok(status_of(enabled))
}

pub fn set_enabled<R: Runtime>(
    app: &AppHandle<R>,
    enabled: bool,
) -> Result<AutostartStatus, String> {
    let manager = app.autolaunch();
    let result = if enabled {
        manager.enable()
    } else {
        manager.disable()
    };
    result.map_err(|error| error.to_string())?;
    status(app)
}

pub fn open_settings() {}
