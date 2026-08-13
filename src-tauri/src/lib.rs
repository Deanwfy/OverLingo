mod app_config;
mod audio;
mod commands;
mod controller;
mod credentials;
mod diagnostics;
mod overlay_pointer;
mod persistence;
mod translators;
mod windowing;

use app_config::AppConfig;
use commands::audio::AudioState;
use commands::realtime::ProviderState;
use credentials::{CredentialState, CredentialStore};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tauri::Manager;

static EXIT_ALLOWED: AtomicBool = AtomicBool::new(false);

fn install_tls_crypto_provider() {
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        rustls::crypto::ring::default_provider()
            .install_default()
            .expect("failed to install the TLS crypto provider");
    }
}

pub(crate) fn exit_now(app: &tauri::AppHandle) {
    EXIT_ALLOWED.store(true, Ordering::SeqCst);
    app.exit(0);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    install_tls_crypto_provider();
    #[cfg(not(target_os = "macos"))]
    let background_launch = std::env::args_os().any(|arg| arg == "--background");

    let context = tauri::generate_context!();
    let credential_service = context.config().identifier.clone();
    let credential_state = CredentialState(Mutex::new(CredentialStore::load(credential_service)));

    // The identifier keeps the LaunchAgent label reverse-DNS and keeps dev and release entries apart.
    let autostart = tauri_plugin_autostart::Builder::new()
        .arg("--background")
        .app_name(context.config().identifier.clone());
    #[cfg(target_os = "macos")]
    let autostart = autostart.macos_launcher(tauri_plugin_autostart::MacosLauncher::LaunchAgent);

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(autostart.build());
    #[cfg(target_os = "macos")]
    let builder = builder.plugin(tauri_nspanel::init());

    builder
        .manage(AudioState::new())
        .manage(ProviderState::default())
        .manage(credential_state)
        .setup(move |app| {
            let config = AppConfig::load(app.handle());
            let locale = config.locale.clone();
            app.manage(controller::AppController::new(app.handle().clone(), config));
            windowing::install(app, &locale)?;
            #[cfg(not(target_os = "macos"))]
            if !background_launch {
                windowing::show_settings(app.handle()).map_err(std::io::Error::other)?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::credentials::get_credential_status,
            commands::credentials::set_provider_credential,
            commands::session_store::list_sessions,
            commands::session_store::read_session,
            commands::session_store::rename_session,
            commands::session_store::export_session,
            commands::session_store::delete_session,
            windowing::show_settings_window,
            overlay_pointer::set_overlay_interactive_height,
            controller::controller_action,
            controller::subscribe_controller,
        ])
        .build(context)
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            use tauri::Manager;
            match event {
                #[cfg(target_os = "macos")]
                tauri::RunEvent::Reopen { .. } => {
                    let _ = windowing::show_settings(app_handle);
                }
                tauri::RunEvent::ExitRequested { api, .. }
                    if !EXIT_ALLOWED.load(Ordering::SeqCst) =>
                {
                    api.prevent_exit();
                    if let Some(controller) = app_handle.try_state::<controller::AppController>() {
                        let _ = controller.request(controller::ControllerRequest::Exit);
                    } else {
                        exit_now(app_handle);
                    }
                }
                _ => {}
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installs_tls_crypto_provider_once() {
        install_tls_crypto_provider();
        install_tls_crypto_provider();
        assert!(rustls::crypto::CryptoProvider::get_default().is_some());
    }
}
