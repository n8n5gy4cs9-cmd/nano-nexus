use std::sync::Arc;

use nexus_core::error::IpcError;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;

use super::CmdResult;
use crate::service::permissions::{Decision, PermissionRequest};
use crate::service::{self};
use crate::state::AppState;

/// Global event `permission:request`: a tool needs a folder the user has not granted.
#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
#[tauri_specta(event_name = "permission:request")]
pub struct PermissionRequested(pub PermissionRequest);

/// The user's answer to a permission prompt. `false` if it was already answered or timed out.
#[tauri::command]
#[specta::specta]
pub async fn permission_respond(ctx: State<'_, Arc<AppState>>, request_id: String, decision: Decision) -> CmdResult<bool> {
    service::permissions::respond(&ctx, &request_id, decision).map_err(IpcError::from)
}

/// Prompts still open (after a UI reload).
#[tauri::command]
#[specta::specta]
pub async fn permission_requests(ctx: State<'_, Arc<AppState>>) -> CmdResult<Vec<PermissionRequest>> {
    Ok(ctx.broker.open_requests())
}
