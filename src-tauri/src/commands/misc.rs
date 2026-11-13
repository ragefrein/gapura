//! Settings, known-hosts management, and misc app commands.

use gapura_core::settings::{save as save_settings_file, Settings};
use gapura_core::ssh::known_hosts::KnownHostEntry;
use gapura_core::GapuraError;
use serde::Serialize;

use crate::error::CommandResult;
use crate::state::AppState;

#[tauri::command]
pub fn get_settings(state: tauri::State<'_, AppState>) -> CommandResult<Settings> {
    Ok(state
        .settings
        .read()
        .map_err(|_| GapuraError::Other("settings lock poisoned".into()))?
        .clone())
}

#[tauri::command]
pub fn save_settings(state: tauri::State<'_, AppState>, settings: Settings) -> CommandResult<()> {
    save_settings_file(&state.config_dir, &settings)?;
    *state
        .settings
        .write()
        .map_err(|_| GapuraError::Other("settings lock poisoned".into()))? = settings;
    Ok(())
}

#[tauri::command]
pub fn list_known_hosts(state: tauri::State<'_, AppState>) -> CommandResult<Vec<KnownHostEntry>> {
    let known = state
        .known_hosts
        .lock()
        .map_err(|_| GapuraError::Other("known-hosts lock poisoned".into()))?;
    Ok(known.entries().to_vec())
}

#[tauri::command]
pub fn remove_known_host(
    state: tauri::State<'_, AppState>,
    host_port: String,
) -> CommandResult<bool> {
    let mut known = state
        .known_hosts
        .lock()
        .map_err(|_| GapuraError::Other("known-hosts lock poisoned".into()))?;
    known.remove(&host_port)
}

#[derive(Debug, Serialize)]
pub struct AppPaths {
    pub data_dir: String,
    pub config_dir: String,
}

#[tauri::command]
pub fn app_paths(state: tauri::State<'_, AppState>) -> AppPaths {
    AppPaths {
        data_dir: state.data_dir.display().to_string(),
        config_dir: state.config_dir.display().to_string(),
    }
}

#[tauri::command]
pub async fn open_data_dir(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> CommandResult<()> {
    let path = state.data_dir.clone();
    tauri_plugin_opener::OpenerExt::opener(&app)
        .open_path(path.display().to_string(), None::<&str>)
        .map_err(|e| GapuraError::Other(format!("cannot open the data folder: {e}")))
}

#[derive(Debug, Serialize)]
pub struct VersionInfo {
    pub version: String,
}

/// v0.1 has no auto-update (PRD Q4): the UI links to GitHub releases.
#[tauri::command]
pub fn version_info() -> VersionInfo {
    VersionInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
    }
}
