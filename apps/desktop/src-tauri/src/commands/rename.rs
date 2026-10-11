//! Rename Forge commands (M09): thin wrappers over `service::rename`.

use std::sync::Arc;

use nexus_analyze::code::find::{Found, Usage};
use nexus_analyze::code::verify::{Checker, VerifyResult};
use nexus_core::error::{IpcError, NexusError};
use tauri::State;
use tauri::ipc::Channel;

use super::CmdResult;
use crate::service;
use crate::service::rename::{RenameItemView, RenamePlanOptions, RenamePlanView, RenameProgress, RenameSessionView};
use crate::state::AppState;

type Ctx<'a> = State<'a, Arc<AppState>>;

fn ipc(e: NexusError) -> IpcError {
    IpcError::from(e)
}

fn progress_to(ch: Channel<RenameProgress>) -> Arc<dyn Fn(RenameProgress) + Send + Sync> {
    Arc::new(move |p| {
        let _ = ch.send(p);
    })
}

/// Index a project folder and list its naming issues. `job_id` is the handle for `rename_cancel`.
#[tauri::command]
#[specta::specta]
pub async fn rename_session_create(
    ctx: Ctx<'_>,
    job_id: String,
    root: String,
    on_event: Channel<RenameProgress>,
) -> CmdResult<RenameSessionView> {
    service::rename::session_create(&ctx, &job_id, &root, progress_to(on_event)).await.map_err(ipc)
}

#[tauri::command]
#[specta::specta]
pub async fn rename_sessions(ctx: Ctx<'_>, limit: u32) -> CmdResult<Vec<RenameSessionView>> {
    service::rename::sessions_list(&ctx, limit).map_err(ipc)
}

#[tauri::command]
#[specta::specta]
pub async fn rename_session_delete(ctx: Ctx<'_>, session_id: String) -> CmdResult<bool> {
    service::rename::session_delete(&ctx, &session_id).map_err(ipc)
}

#[tauri::command]
#[specta::specta]
pub async fn rename_items(ctx: Ctx<'_>, session_id: String, status: Option<String>) -> CmdResult<Vec<RenameItemView>> {
    service::rename::items(&ctx, &session_id, status.as_deref()).map_err(ipc)
}

/// Ask the model for names; returns how many proposals were made.
#[tauri::command]
#[specta::specta]
pub async fn rename_propose(
    ctx: Ctx<'_>,
    job_id: String,
    session_id: String,
    symbol_ids: Option<Vec<String>>,
    try_again: bool,
    on_event: Channel<RenameProgress>,
) -> CmdResult<u32> {
    service::rename::propose_names(&ctx, &job_id, &session_id, symbol_ids, try_again, progress_to(on_event))
        .await
        .map_err(ipc)
}

#[tauri::command]
#[specta::specta]
pub async fn rename_item_update(
    ctx: Ctx<'_>,
    session_id: String,
    symbol_id: String,
    final_name: Option<String>,
    status: Option<String>,
    include_strings: Option<bool>,
) -> CmdResult<RenameItemView> {
    service::rename::item_update(&ctx, &session_id, &symbol_id, final_name, status, include_strings).map_err(ipc)
}

/// Plan the accepted renames as a change set to review on the Changes page.
#[tauri::command]
#[specta::specta]
pub async fn rename_plan(ctx: Ctx<'_>, session_id: String, options: RenamePlanOptions) -> CmdResult<RenamePlanView> {
    service::rename::plan(&ctx, &session_id, options).await.map_err(ipc)
}

#[tauri::command]
#[specta::specta]
pub async fn rename_checkers(ctx: Ctx<'_>, session_id: String) -> CmdResult<Vec<Checker>> {
    service::rename::verify_checkers(&ctx, &session_id).map_err(ipc)
}

/// Run the project's own checker; output lines arrive on `on_output`.
#[tauri::command]
#[specta::specta]
pub async fn rename_verify(
    ctx: Ctx<'_>,
    job_id: String,
    session_id: String,
    checker_id: String,
    on_event: Channel<String>,
) -> CmdResult<VerifyResult> {
    let sink: Arc<dyn Fn(String) + Send + Sync> = Arc::new(move |l| {
        let _ = on_event.send(l);
    });
    service::rename::verify(&ctx, &job_id, &session_id, &checker_id, sink).await.map_err(ipc)
}

#[tauri::command]
#[specta::specta]
pub async fn rename_usages(ctx: Ctx<'_>, session_id: String, symbol_id: String) -> CmdResult<Vec<Usage>> {
    service::rename::usages(&ctx, &session_id, &symbol_id).map_err(ipc)
}

#[tauri::command]
#[specta::specta]
pub async fn rename_find(ctx: Ctx<'_>, session_id: String, query: String) -> CmdResult<Vec<Found>> {
    service::rename::find(&ctx, &session_id, &query).map_err(ipc)
}

#[tauri::command]
#[specta::specta]
pub async fn rename_cancel(ctx: Ctx<'_>, job_id: String) -> CmdResult<bool> {
    Ok(service::rename::cancel(&ctx, &job_id))
}
