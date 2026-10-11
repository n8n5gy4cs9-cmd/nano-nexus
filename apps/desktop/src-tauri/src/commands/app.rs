use serde::Serialize;
use specta::Type;
use tauri::State;

use super::CmdResult;
use crate::state::AppState;

/// Static facts about this install.
#[derive(Clone, Debug, Serialize, Type)]
pub struct AppInfo {
    pub version: String,
    pub os: String,
    pub arch: String,
    pub data_dir: String,
    pub log_dir: String,
}

#[tauri::command]
#[specta::specta]
pub async fn app_info(state: State<'_, AppState>) -> CmdResult<AppInfo> {
    Ok(AppInfo {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        os: std::env::consts::OS.to_owned(),
        arch: std::env::consts::ARCH.to_owned(),
        data_dir: state.paths.data.to_string_lossy().into_owned(),
        log_dir: state.paths.logs.to_string_lossy().into_owned(),
    })
}
