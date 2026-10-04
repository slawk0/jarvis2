//! Commands over local data: profiles, per-profile documents, settings, secrets.

use serde::Serialize;
use serde_json::Value;
use specta::Type;
use tauri::State;

use crate::error::{AppError, AppResult};
use crate::shell::validate;
use crate::state::AppState;
use crate::store::profile_data::DataKey;
use crate::store::profiles::{resolve_key_path, AuthType, Profile, ProfileInput, SECRET_PASSPHRASE, SECRET_PASSWORD};
use crate::store::secrets::GLOBAL;
use crate::store::settings::AppSettings;

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProfileView {
    pub profile: Profile,
    pub has_password: bool,
    pub has_passphrase: bool,
}

fn view(state: &AppState, profile: Profile) -> ProfileView {
    state.secrets.import_legacy_profile(&profile.id);
    ProfileView {
        has_password: state.secrets.exists(&profile.id, SECRET_PASSWORD),
        has_passphrase: state.secrets.exists(&profile.id, SECRET_PASSPHRASE),
        profile,
    }
}

#[tauri::command]
#[specta::specta]
pub fn profiles_list(state: State<'_, AppState>) -> Vec<ProfileView> {
    state.profiles.list().into_iter().map(|p| view(&state, p)).collect()
}

#[tauri::command]
#[specta::specta]
pub fn profile_save(state: State<'_, AppState>, input: ProfileInput) -> AppResult<ProfileView> {
    if input.id.is_none() && input.auth_type == AuthType::Password && input.password.as_deref().unwrap_or("").is_empty() {
        return Err(AppError::invalid("Password is required"));
    }
    let profile = state.profiles.upsert(&input)?;
    let secrets = &state.secrets;
    if input.clear_secrets {
        secrets.delete(&profile.id, SECRET_PASSWORD)?;
        secrets.delete(&profile.id, SECRET_PASSPHRASE)?;
    }
    match input.auth_type {
        AuthType::Password => {
            if let Some(password) = input.password.filter(|p| !p.is_empty()) {
                secrets.set(&profile.id, SECRET_PASSWORD, &password)?;
            }
            secrets.delete(&profile.id, SECRET_PASSPHRASE)?;
        }
        AuthType::Key => {
            if let Some(passphrase) = input.passphrase.filter(|p| !p.is_empty()) {
                secrets.set(&profile.id, SECRET_PASSPHRASE, &passphrase)?;
            }
            secrets.delete(&profile.id, SECRET_PASSWORD)?;
        }
    }
    Ok(view(&state, profile))
}

/// Delete a profile together with all of its data and secrets.
#[tauri::command]
#[specta::specta]
pub async fn profile_delete(state: State<'_, AppState>, profile_id: String) -> AppResult<()> {
    if state.session_opt().is_some_and(|s| s.profile.id == profile_id) {
        state.teardown().await;
    }
    state.profiles.delete(&profile_id)?;
    state.secrets.delete_all(&profile_id)?;
    state.data.delete_profile(&profile_id)
}

#[tauri::command]
#[specta::specta]
pub fn profile_set_default(state: State<'_, AppState>, profile_id: Option<String>) -> AppResult<()> {
    state.profiles.set_default(profile_id.as_deref())
}

/// Validate a picked key file; a `.pub` resolves to its private sibling.
#[tauri::command]
#[specta::specta]
pub fn profile_resolve_key(path: String) -> AppResult<String> {
    resolve_key_path(std::path::Path::new(path.trim()))
}

/// Folder the key picker should open in.
#[tauri::command]
#[specta::specta]
pub fn default_key_dir() -> Option<String> {
    let home = dirs::home_dir()?;
    let ssh = home.join(".ssh");
    let dir = if ssh.is_dir() { ssh } else { home };
    Some(dir.to_string_lossy().into_owned())
}

/// One per-profile document as JSON text (`null` when nothing is stored).
/// Documents travel as text because their shape belongs to the feature that
/// owns them; the frontend wraps this in a typed accessor per [`DataKey`].
#[tauri::command]
#[specta::specta]
pub fn profile_data_get(state: State<'_, AppState>, profile_id: String, key: DataKey) -> AppResult<String> {
    Ok(state.data.get(&profile_id, key)?.to_string())
}

#[tauri::command]
#[specta::specta]
pub fn profile_data_set(state: State<'_, AppState>, profile_id: String, key: DataKey, json: String) -> AppResult<()> {
    let value: Value = serde_json::from_str(&json)?;
    state.data.set(&profile_id, key, &value)
}

#[tauri::command]
#[specta::specta]
pub fn settings_get(state: State<'_, AppState>) -> AppSettings {
    state.settings.get()
}

#[tauri::command]
#[specta::specta]
pub fn settings_set(state: State<'_, AppState>, settings: AppSettings) -> AppResult<()> {
    state.settings.set(settings)
}

#[tauri::command]
#[specta::specta]
pub fn default_download_dir(state: State<'_, AppState>) -> String {
    state.settings.download_dir().to_string_lossy().into_owned()
}

/// Problems found while loading local files (e.g. a corrupt store that was
/// set aside). Returned once.
#[tauri::command]
#[specta::specta]
pub fn startup_notices(state: State<'_, AppState>) -> Vec<AppError> {
    std::mem::take(&mut *state.notices.lock())
}

fn secret_owner(owner: &Option<String>) -> AppResult<&str> {
    match owner {
        Some(id) => validate::slug("profile id", id),
        None => Ok(GLOBAL),
    }
}

fn secret_name(name: &str) -> AppResult<&str> {
    let ok = !name.is_empty() && name.len() <= 200 && name.bytes().all(|b| b.is_ascii_alphanumeric() || b"/_.-".contains(&b));
    if ok {
        Ok(name)
    } else {
        Err(AppError::invalid("Invalid secret name"))
    }
}

/// Store a secret in the OS keyring. `profile_id = null` is the global scope.
/// Secrets are write-only for the UI: there is no command that reads one back.
#[tauri::command]
#[specta::specta]
pub fn secret_set(state: State<'_, AppState>, profile_id: Option<String>, name: String, value: String) -> AppResult<()> {
    state.secrets.set(secret_owner(&profile_id)?, secret_name(&name)?, &value)
}

#[tauri::command]
#[specta::specta]
pub fn secret_exists(state: State<'_, AppState>, profile_id: Option<String>, name: String) -> AppResult<bool> {
    Ok(state.secrets.exists(secret_owner(&profile_id)?, secret_name(&name)?))
}

/// Clear one secret, or every secret under a `prefix/`.
#[tauri::command]
#[specta::specta]
pub fn secret_clear(state: State<'_, AppState>, profile_id: Option<String>, name: String) -> AppResult<()> {
    let owner = secret_owner(&profile_id)?;
    let name = secret_name(&name)?;
    if name.ends_with('/') {
        state.secrets.delete_prefixed(owner, name)
    } else {
        state.secrets.delete(owner, name)
    }
}

/// Write text to a local file the user picked in a save dialog
/// (log downloads, query exports).
#[tauri::command]
#[specta::specta]
pub async fn save_text_file(path: String, content: String) -> AppResult<()> {
    tokio::fs::write(&path, content).await?;
    Ok(())
}
