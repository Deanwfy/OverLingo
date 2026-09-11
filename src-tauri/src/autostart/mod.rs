//! Launch-at-login behind one interface. macOS registers the app itself through
//! `SMAppService`, so it lands under "Open at Login" with its own name and icon; other
//! platforms keep the autostart plugin.

use serde::Serialize;
use tauri::{AppHandle, Runtime};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(target_os = "macos"))]
mod plugin;

#[cfg(target_os = "macos")]
use macos as platform;
#[cfg(not(target_os = "macos"))]
use plugin as platform;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AutostartStatus {
    Enabled,
    Disabled,
    /// The user switched the item off in System Settings; only they can turn it back on.
    /// Only macOS reports it, but the frontend handles it everywhere.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    RequiresApproval,
}

#[tauri::command]
pub fn get_autostart_status<R: Runtime>(app: AppHandle<R>) -> Result<AutostartStatus, String> {
    platform::status(&app)
}

#[tauri::command]
pub fn set_autostart_enabled<R: Runtime>(
    app: AppHandle<R>,
    enabled: bool,
) -> Result<AutostartStatus, String> {
    platform::set_enabled(&app, enabled)
}

#[tauri::command]
pub fn open_autostart_settings() {
    platform::open_settings();
}
