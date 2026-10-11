use std::sync::Arc;

use nexus_core::error::IpcError;
use nexus_core::settings::Settings;
use tauri::State;

use super::CmdResult;
use crate::service::settings::SettingValue;
use crate::service::{self, Caller};
use crate::state::AppState;

#[tauri::command]
#[specta::specta]
pub async fn settings_get(ctx: State<'_, Arc<AppState>>) -> CmdResult<Settings> {
    Ok(service::settings::get(&ctx))
}

/// Set one key (dotted name, e.g. `"ui.theme"`). Unknown keys and wrongly typed values are rejected.
#[tauri::command]
#[specta::specta]
pub async fn settings_set(
    ctx: State<'_, Arc<AppState>>,
    key: String,
    value: SettingValue,
) -> CmdResult<Settings> {
    service::settings::set(&ctx, &Caller::Desktop, &key, value).map_err(IpcError::from)
}
