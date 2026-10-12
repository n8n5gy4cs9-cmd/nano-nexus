use std::sync::Arc;

use nexus_core::error::IpcError;
use tauri::State;
use tauri::ipc::Channel;

use super::CmdResult;
use crate::service::editor::{EditorAiEvent, EditorAiRequest, EditorSaveResult};
use crate::service::{self, Caller, ChannelSink};
use crate::state::AppState;

type Ctx<'a> = State<'a, Arc<AppState>>;

fn join_err(e: tauri::Error) -> IpcError {
    IpcError::from(nexus_core::error::NexusError::Internal(e.to_string()))
}

/// Is there a write grant for this file? (No: the editor opens it read-only.)
#[tauri::command]
#[specta::specta]
pub async fn editor_can_write(ctx: Ctx<'_>, path: String) -> CmdResult<bool> {
    Ok(service::editor::can_write(ctx.inner(), &Caller::Desktop, &path))
}

/// Save the buffer through a change set. Fails with `conflict` if the file changed since `base_hash`.
#[tauri::command]
#[specta::specta]
pub async fn editor_save(
    ctx: Ctx<'_>,
    path: String,
    contents: String,
    base_hash: Option<String>,
) -> CmdResult<EditorSaveResult> {
    let ctx = ctx.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        service::editor::save(&ctx, &Caller::Desktop, &path, &contents, base_hash.as_deref())
    })
    .await
    .map_err(join_err)?
    .map_err(IpcError::from)
}

/// Run an AI action on a selection; the text streams over `on_event`.
#[tauri::command]
#[specta::specta]
pub async fn editor_ai(
    ctx: Ctx<'_>,
    job_id: String,
    request: EditorAiRequest,
    on_event: Channel<EditorAiEvent>,
) -> CmdResult<()> {
    service::editor::ai(
        ctx.inner(),
        &Caller::Desktop,
        &job_id,
        request,
        Arc::new(ChannelSink(on_event)),
    )
    .await
    .map_err(IpcError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn editor_ai_cancel(ctx: Ctx<'_>, job_id: String) -> CmdResult<bool> {
    Ok(service::editor::cancel(ctx.inner(), &job_id))
}
