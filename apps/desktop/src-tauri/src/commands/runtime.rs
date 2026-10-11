use nexus_core::error::{IpcError, NexusError};
use nexus_core::events::RuntimeState;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, State};

use super::CmdResult;
use crate::state::AppState;

/// Global event `runtime:status`: sent on every supervisor state change.
#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
#[tauri_specta(event_name = "runtime:status")]
pub struct RuntimeStatus(pub RuntimeState);

#[tauri::command]
#[specta::specta]
pub async fn runtime_status(state: State<'_, AppState>) -> CmdResult<RuntimeState> {
    Ok(state.supervisor.state())
}

#[tauri::command]
#[specta::specta]
pub async fn runtime_restart(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    let cfg = crate::runtime::build_config(&app).ok_or_else(|| IpcError::from(NexusError::ModelMissing))?;
    state
        .supervisor
        .restart(cfg)
        .await
        .map_err(|e| IpcError::from(NexusError::from(e)))
}

#[tauri::command]
#[specta::specta]
pub async fn runtime_logs(state: State<'_, AppState>, lines: u32) -> CmdResult<Vec<String>> {
    Ok(state.supervisor.log_tail(lines.min(500) as usize).await)
}
