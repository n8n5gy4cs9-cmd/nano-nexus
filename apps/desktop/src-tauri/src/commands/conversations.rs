use std::sync::Arc;

use nexus_core::chat::ConversationSummary;
use nexus_core::error::IpcError;
use nexus_core::ids::ConversationId;
use tauri::State;

use super::CmdResult;
use crate::service::conversations::{ConversationUpdate, ConversationWithMessages};
use crate::service::{self};
use crate::state::AppState;

/// Pinned first, then newest; `query` searches titles and message text.
#[tauri::command]
#[specta::specta]
pub async fn conversations_list(
    ctx: State<'_, Arc<AppState>>,
    query: Option<String>,
    limit: u32,
    before: Option<i64>,
) -> CmdResult<Vec<ConversationSummary>> {
    service::conversations::list(&ctx, query.as_deref(), limit, before).map_err(IpcError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn conversation_get(
    ctx: State<'_, Arc<AppState>>,
    id: ConversationId,
) -> CmdResult<ConversationWithMessages> {
    service::conversations::get(&ctx, &id).map_err(IpcError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn conversation_update(
    ctx: State<'_, Arc<AppState>>,
    id: ConversationId,
    update: ConversationUpdate,
) -> CmdResult<()> {
    service::conversations::update(&ctx, &id, update).map_err(IpcError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn conversation_delete(ctx: State<'_, Arc<AppState>>, id: ConversationId) -> CmdResult<()> {
    service::conversations::delete(&ctx, &id).map_err(IpcError::from)
}

/// The conversation as Markdown text (the UI copies or saves it).
#[tauri::command]
#[specta::specta]
pub async fn conversation_export_md(ctx: State<'_, Arc<AppState>>, id: ConversationId) -> CmdResult<String> {
    service::conversations::export_markdown(&ctx, &id).map_err(IpcError::from)
}
