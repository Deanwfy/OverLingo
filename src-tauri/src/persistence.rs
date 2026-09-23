use serde::de::DeserializeOwned;
use serde::Serialize;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

/// A small JSON document in the app's config directory. Anything missing or unreadable
/// falls back to the default, so a file from an older build still loads.
pub fn read_config<T: DeserializeOwned + Default>(app: &AppHandle, name: &str) -> T {
    config_path(app, name)
        .ok()
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|content| serde_json::from_str(&content).ok())
        .unwrap_or_default()
}

pub fn write_config<T: Serialize>(app: &AppHandle, name: &str, value: &T) -> Result<(), String> {
    let path = config_path(app, name)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Failed to create config directory: {error}"))?;
    }
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|error| format!("Failed to serialize {name}: {error}"))?;
    write_atomic(&path, &bytes)
}

pub fn config_path(app: &AppHandle, name: &str) -> Result<PathBuf, String> {
    app.path()
        .app_config_dir()
        .map(|path| path.join(name))
        .map_err(|error| format!("Failed to resolve config directory: {error}"))
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    let temporary = path.with_extension(format!("{extension}.{}.tmp", rand::random::<u64>()));
    let write_result = (|| {
        let mut file = fs::File::create(&temporary)
            .map_err(|error| format!("Failed to create temporary file: {error}"))?;
        file.write_all(bytes)
            .map_err(|error| format!("Failed to write temporary file: {error}"))?;
        file.sync_all()
            .map_err(|error| format!("Failed to sync temporary file: {error}"))?;
        Ok::<(), String>(())
    })();
    if let Err(error) = write_result {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    replace(&temporary, path)
}

#[cfg(not(target_os = "windows"))]
fn replace(temporary: &Path, destination: &Path) -> Result<(), String> {
    fs::rename(temporary, destination).map_err(|error| {
        let _ = fs::remove_file(temporary);
        format!("Failed to replace file: {error}")
    })
}

#[cfg(target_os = "windows")]
fn replace(temporary: &Path, destination: &Path) -> Result<(), String> {
    let backup = destination.with_extension("backup");
    let had_destination = destination.exists();
    if had_destination {
        let _ = fs::remove_file(&backup);
        fs::rename(destination, &backup)
            .map_err(|error| format!("Failed to prepare file replacement: {error}"))?;
    }
    match fs::rename(temporary, destination) {
        Ok(()) => {
            let _ = fs::remove_file(backup);
            Ok(())
        }
        Err(error) => {
            if had_destination {
                let _ = fs::rename(backup, destination);
            }
            let _ = fs::remove_file(temporary);
            Err(format!("Failed to replace file: {error}"))
        }
    }
}
