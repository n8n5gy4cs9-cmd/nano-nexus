//! Knowledge library commands (M17): thin wrappers over `service::knowledge`.

use std::sync::Arc;

use nexus_core::error::{IpcError, NexusError};
use nexus_knowledge::{Library, Passage};
use tauri::State;
use tauri::ipc::Channel;

use super::CmdResult;
use crate::service;
use crate::service::knowledge::{KbDocumentSource, KbIndexResult, KbLibraryDetail, KbProgress};
use crate::state::AppState;

type Ctx<'a> = State<'a, Arc<AppState>>;

fn ipc(e: NexusError) -> IpcError {
    IpcError::from(e)
}

/// Every library with counts.
#[tauri::command]
#[specta::specta]
pub async fn kb_list(ctx: Ctx<'_>) -> CmdResult<Vec<Library>> {
    service::knowledge::list(&ctx).map_err(ipc)
}

/// One library with its documents (skipped and failed files carry a note).
#[tauri::command]
#[specta::specta]
pub async fn kb_detail(ctx: Ctx<'_>, id: String) -> CmdResult<KbLibraryDetail> {
    service::knowledge::detail(&ctx, &id).map_err(ipc)
}

/// Create a library from folders/files the user picked or dropped. Call `kb_reindex` next.
#[tauri::command]
#[specta::specta]
pub async fn kb_add(
    ctx: Ctx<'_>,
    name: Option<String>,
    paths: Vec<String>,
    reference: bool,
) -> CmdResult<Library> {
    service::knowledge::add(&ctx, name, paths, reference)
        .await
        .map_err(ipc)
}

/// Index or incrementally re-index a library. Progress arrives on `on_event` after every file.
#[tauri::command]
#[specta::specta]
pub async fn kb_reindex(
    ctx: Ctx<'_>,
    job_id: String,
    library_id: String,
    on_event: Channel<KbProgress>,
) -> CmdResult<KbIndexResult> {
    let progress: Arc<dyn Fn(KbProgress) + Send + Sync> = Arc::new(move |p| {
        let _ = on_event.send(p);
    });
    service::knowledge::reindex(&ctx, &library_id, &job_id, progress)
        .await
        .map_err(ipc)
}

/// Stop a running index job. `false` if it is not running.
#[tauri::command]
#[specta::specta]
pub async fn kb_cancel(job_id: String) -> CmdResult<bool> {
    Ok(service::knowledge::cancel(&job_id))
}

#[tauri::command]
#[specta::specta]
pub async fn kb_set_enabled(ctx: Ctx<'_>, id: String, enabled: bool) -> CmdResult<()> {
    service::knowledge::set_enabled(&ctx, &id, enabled).map_err(ipc)
}

/// Remove a library (copied files are deleted; referenced folders are never touched).
#[tauri::command]
#[specta::specta]
pub async fn kb_remove(ctx: Ctx<'_>, id: String) -> CmdResult<()> {
    service::knowledge::remove(&ctx, &id).map_err(ipc)
}

/// Passage search: what the model would see for this question.
#[tauri::command]
#[specta::specta]
pub async fn kb_search(
    ctx: Ctx<'_>,
    query: String,
    library_ids: Option<Vec<String>>,
    k: u32,
) -> CmdResult<Vec<Passage>> {
    service::knowledge::search_passages(&ctx, &query, library_ids, k).map_err(ipc)
}

/// One passage in full (a clicked citation).
#[tauri::command]
#[specta::specta]
pub async fn kb_passage(ctx: Ctx<'_>, chunk_id: i64) -> CmdResult<Passage> {
    service::knowledge::passage(&ctx, chunk_id).map_err(ipc)
}

/// The original text of a passage's document (rendered / raw views). Text formats only.
#[tauri::command]
#[specta::specta]
pub async fn kb_document_source(ctx: Ctx<'_>, chunk_id: i64) -> CmdResult<KbDocumentSource> {
    service::knowledge::document_source(&ctx, chunk_id).map_err(ipc)
}

