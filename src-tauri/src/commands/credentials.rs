use crate::controller::AppController;
use crate::credentials::CredentialState;
use std::collections::HashMap;
use tauri::State;

#[tauri::command]
pub fn get_credential_status(
    state: State<'_, CredentialState>,
) -> Result<HashMap<String, bool>, String> {
    let store = state.0.lock().map_err(|error| error.to_string())?;
    Ok(store.status())
}

#[tauri::command]
pub fn set_provider_credential(
    provider: String,
    secret: String,
    state: State<'_, CredentialState>,
    controller: State<'_, AppController>,
) -> Result<HashMap<String, bool>, String> {
    let status = {
        let mut store = state.0.lock().map_err(|error| error.to_string())?;
        store.set(&provider, secret.trim())?;
        store.status()
    };
    controller.credentials_changed();
    Ok(status)
}
