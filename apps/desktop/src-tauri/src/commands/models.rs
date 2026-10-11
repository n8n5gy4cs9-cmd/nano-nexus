use std::sync::Arc;

use nexus_core::error::{IpcError, NexusError};
use nexus_models::bundled::VerifyResult;
use nexus_models::fit::MemoryInfo;
use nexus_models::hf::SearchHit;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

use super::CmdResult;
use crate::service::models::{ActivateResult, CatalogCard, DownloadView, InstalledModel, RepoFiles};
use crate::service::{self, Caller};
use crate::state::AppState;

type Ctx<'a> = State<'a, Arc<AppState>>;

fn ipc(e: NexusError) -> IpcError {
    IpcError::from(e)
}

/// Global event `download:progress`: sent about 4 times a second while a download runs, and on every status change.
#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
#[tauri_specta(event_name = "download:progress")]
pub struct DownloadProgressEvent(pub DownloadView);

/// Global event `model:changed`: the active model id (the runtime is restarting on it).
#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
#[tauri_specta(event_name = "model:changed")]
pub struct ModelChanged(pub String);

#[tauri::command]
#[specta::specta]
pub async fn models_list(ctx: Ctx<'_>) -> CmdResult<Vec<InstalledModel>> {
    service::models::list_installed(&ctx).map_err(ipc)
}

#[tauri::command]
#[specta::specta]
pub async fn models_catalog(ctx: Ctx<'_>) -> CmdResult<Vec<CatalogCard>> {
    service::models::catalog(&ctx).map_err(ipc)
}

#[tauri::command]
#[specta::specta]
pub async fn models_memory() -> CmdResult<MemoryInfo> {
    Ok(service::models::system_memory())
}

#[tauri::command]
#[specta::specta]
pub async fn models_search(ctx: Ctx<'_>, query: String) -> CmdResult<Vec<SearchHit>> {
    service::models::search(&ctx, &query).await.map_err(ipc)
}

#[tauri::command]
#[specta::specta]
pub async fn models_repo_files(ctx: Ctx<'_>, repo: String) -> CmdResult<RepoFiles> {
    service::models::repo_files(&ctx, &repo).await.map_err(ipc)
}

#[tauri::command]
#[specta::specta]
pub async fn models_downloads(ctx: Ctx<'_>) -> CmdResult<Vec<DownloadView>> {
    service::models::downloads(&ctx).map_err(ipc)
}

#[tauri::command]
#[specta::specta]
pub async fn models_download_catalog(ctx: Ctx<'_>, catalog_id: String) -> CmdResult<DownloadView> {
    service::models::start_catalog(&ctx, &catalog_id)
        .await
        .map_err(ipc)
}

#[tauri::command]
#[specta::specta]
pub async fn models_download_hf(ctx: Ctx<'_>, repo: String, file: String) -> CmdResult<DownloadView> {
    service::models::start_hf(&ctx, &repo, &file).await.map_err(ipc)
}

#[tauri::command]
#[specta::specta]
pub async fn models_download_pause(ctx: Ctx<'_>, id: String) -> CmdResult<()> {
    service::models::pause(&ctx, &Caller::Desktop, &id).map_err(ipc)
}

#[tauri::command]
#[specta::specta]
pub async fn models_download_resume(ctx: Ctx<'_>, id: String) -> CmdResult<()> {
    service::models::resume(&ctx, &Caller::Desktop, &id).map_err(ipc)
}

#[tauri::command]
#[specta::specta]
pub async fn models_download_cancel(ctx: Ctx<'_>, id: String) -> CmdResult<()> {
    service::models::cancel(&ctx, &Caller::Desktop, &id).map_err(ipc)
}

#[tauri::command]
#[specta::specta]
pub async fn models_activate(ctx: Ctx<'_>, id: String) -> CmdResult<ActivateResult> {
    service::models::activate(&ctx, &Caller::Desktop, &id)
        .await
        .map_err(ipc)
}

#[tauri::command]
#[specta::specta]
pub async fn models_delete(ctx: Ctx<'_>, id: String) -> CmdResult<()> {
    service::models::delete(&ctx, &Caller::Desktop, &id).map_err(ipc)
}

/// Native picker for a `.gguf` file; `None` when cancelled.
#[tauri::command]
#[specta::specta]
pub async fn models_pick_file(app: AppHandle) -> CmdResult<Option<String>> {
    let picked = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .add_filter("GGUF model", &["gguf"])
            .blocking_pick_file()
    })
    .await
    .map_err(|e| ipc(NexusError::Internal(e.to_string())))?;
    Ok(picked
        .and_then(|p| p.into_path().ok())
        .map(|p| p.display().to_string()))
}

#[tauri::command]
#[specta::specta]
pub async fn models_import(ctx: Ctx<'_>, path: String, copy: bool) -> CmdResult<InstalledModel> {
    service::models::import_local(&ctx, &Caller::Desktop, &path, copy)
        .await
        .map_err(ipc)
}

/// Re-scan the extra model folders (after changing `models.extra_dirs`). Returns how many models they hold.
#[tauri::command]
#[specta::specta]
pub async fn models_rescan(ctx: Ctx<'_>) -> CmdResult<u32> {
    service::models::rescan(&ctx).map(|n| n as u32).map_err(ipc)
}

/// Re-hash the bundled models.
#[tauri::command]
#[specta::specta]
pub async fn models_verify_bundled(ctx: Ctx<'_>) -> CmdResult<Vec<VerifyResult>> {
    service::models::verify_bundled(&ctx, &Caller::Desktop)
        .await
        .map_err(ipc)
}
