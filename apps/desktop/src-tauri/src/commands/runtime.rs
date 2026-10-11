use std::sync::Arc;

use nexus_core::error::IpcError;
use nexus_core::events::RuntimeState;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;

use super::CmdResult;
use crate::service::{self, Caller};
use crate::state::AppState;

/// Global event `runtime:status`: sent on every supervisor state change.
#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
#[tauri_specta(event_name = "runtime:status")]
pub struct RuntimeStatus(pub RuntimeState);

#[tauri::command]
#[specta::specta]
pub async fn runtime_status(ctx: State<'_, Arc<AppState>>) -> CmdResult<RuntimeState> {
    Ok(service::runtime::status(&ctx))
}

#[tauri::command]
#[specta::specta]
pub async fn runtime_restart(ctx: State<'_, Arc<AppState>>) -> CmdResult<()> {
    service::runtime::restart(&ctx, &Caller::Desktop)
        .await
        .map_err(IpcError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn runtime_logs(ctx: State<'_, Arc<AppState>>, lines: u32) -> CmdResult<Vec<String>> {
    Ok(service::runtime::logs(&ctx, lines).await)
}
