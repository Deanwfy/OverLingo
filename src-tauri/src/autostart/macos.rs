use super::AutostartStatus;
use objc2_service_management::{SMAppService, SMAppServiceStatus};
use std::path::Path;
use tauri::{AppHandle, Runtime};

fn service() -> objc2::rc::Retained<SMAppService> {
    unsafe { SMAppService::mainAppService() }
}

/// `SMAppService.mainApp` happily registers whatever directory holds a bare executable, so
/// `tauri dev` would turn `target/debug` into a login item. Only a real bundle qualifies.
fn packaged_app() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    if is_bundled(&exe) {
        Ok(())
    } else {
        Err("Launch at login is only available from the packaged app bundle".to_string())
    }
}

fn is_bundled(exe: &Path) -> bool {
    exe.ancestors()
        .nth(3)
        .and_then(|bundle| bundle.extension())
        .is_some_and(|extension| extension == "app")
}

fn current_status() -> AutostartStatus {
    match unsafe { service().status() } {
        SMAppServiceStatus::Enabled => AutostartStatus::Enabled,
        SMAppServiceStatus::RequiresApproval => AutostartStatus::RequiresApproval,
        _ => AutostartStatus::Disabled,
    }
}

pub fn status<R: Runtime>(_app: &AppHandle<R>) -> Result<AutostartStatus, String> {
    if packaged_app().is_err() {
        return Ok(AutostartStatus::Disabled);
    }
    Ok(current_status())
}

pub fn set_enabled<R: Runtime>(
    _app: &AppHandle<R>,
    enabled: bool,
) -> Result<AutostartStatus, String> {
    packaged_app()?;
    let service = service();
    let result = unsafe {
        if enabled {
            service.registerAndReturnError()
        } else {
            service.unregisterAndReturnError()
        }
    };
    let status = current_status();
    match result {
        Ok(()) => Ok(status),
        // Registering while the user has the item switched off fails; the status carries
        // the real answer and the UI points at System Settings.
        Err(_) if status == AutostartStatus::RequiresApproval => Ok(status),
        Err(error) => Err(error.localizedDescription().to_string()),
    }
}

pub fn open_settings() {
    unsafe { SMAppService::openSystemSettingsLoginItems() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_app_bundles_count_as_packaged() {
        assert!(is_bundled(Path::new(
            "/Applications/OverLingo.app/Contents/MacOS/overlingo"
        )));
        assert!(!is_bundled(Path::new(
            "/Users/me/project/target/debug/overlingo"
        )));
        assert!(!is_bundled(Path::new("/overlingo")));
    }
}
