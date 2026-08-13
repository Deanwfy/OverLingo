use std::fs;
use std::io::Write;
use std::path::Path;

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
