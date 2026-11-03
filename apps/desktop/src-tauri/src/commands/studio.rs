//! Thin desktop-only Studio model commands.
use super::CmdResult;
use crate::{
    service::{
        self, Caller,
        studio::{MigrationResult, ModelInput, ModelView, OrphanPreview, ReclaimResult},
    },
    state::AppState,
};
use nexus_core::error::{IpcError, NexusError};
use nexus_models::download::DownloadProgress;
use nexus_store::studio::{ModelKind, StudioModel};
use std::sync::Arc;
use tauri::{AppHandle, State, ipc::Channel};
use tauri_plugin_dialog::DialogExt;
type Ctx<'a> = State<'a, Arc<AppState>>;

#[tauri::command]
#[specta::specta]
pub async fn studio_models(ctx: Ctx<'_>) -> CmdResult<Vec<ModelView>> {
    service::studio::list(&ctx, &Caller::Desktop).map_err(IpcError::from)
}
#[tauri::command]
#[specta::specta]
pub async fn studio_download(
    ctx: Ctx<'_>,
    job_id: String,
    url: String,
    input: ModelInput,
    on_event: Channel<DownloadProgress>,
) -> CmdResult<StudioModel> {
    service::studio::download(
        &ctx,
        &Caller::Desktop,
        &job_id,
        &url,
        input,
        Arc::new(move |p| {
            let _ = on_event.send(p);
        }),
    )
    .await
    .map_err(IpcError::from)
}
#[tauri::command]
#[specta::specta]
pub async fn studio_import(
    ctx: Ctx<'_>,
    job_id: String,
    path: String,
    input: ModelInput,
    on_event: Channel<DownloadProgress>,
) -> CmdResult<StudioModel> {
    service::studio::import(
        &ctx,
        &Caller::Desktop,
        &job_id,
        &path,
        input,
        Arc::new(move |p| {
            let _ = on_event.send(p);
        }),
    )
    .await
    .map_err(IpcError::from)
}
#[tauri::command]
#[specta::specta]
pub async fn studio_transfer_cancel(job_id: String, discard: bool) -> CmdResult<bool> {
    service::studio::cancel(&Caller::Desktop, &job_id, discard).map_err(IpcError::from)
}
#[tauri::command]
#[specta::specta]
pub async fn studio_pick_asset(app: AppHandle) -> CmdResult<Option<String>> {
    let picked = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .add_filter(
                "Model assets",
                &[
                    "gguf",
                    "safetensors",
                    "bin",
                    "onnx",
                    "json",
                    "txt",
                    "npy",
                    "npz",
                    "model",
                ],
            )
            .blocking_pick_file()
    })
    .await
    .map_err(|e| IpcError::from(NexusError::Internal(e.to_string())))?;
    Ok(picked
        .and_then(|p| p.into_path().ok())
        .map(|p| p.display().to_string()))
}

#[tauri::command]
#[specta::specta]
pub async fn studio_select(ctx: Ctx<'_>, id: String) -> CmdResult<()> {
    service::studio::select(&ctx, &Caller::Desktop, &id)
        .await
        .map_err(IpcError::from)
}
#[tauri::command]
#[specta::specta]
pub async fn studio_unload(ctx: Ctx<'_>, kind: ModelKind) -> CmdResult<()> {
    service::studio::unload(&ctx, &Caller::Desktop, kind)
        .await
        .map_err(IpcError::from)
}
#[tauri::command]
#[specta::specta]
pub async fn studio_remove(ctx: Ctx<'_>, id: String) -> CmdResult<()> {
    service::studio::remove(&ctx, &Caller::Desktop, &id).map_err(IpcError::from)
}
#[tauri::command]
#[specta::specta]
pub async fn studio_reclassify(ctx: Ctx<'_>, id: String, kind: ModelKind) -> CmdResult<()> {
    service::studio::reclassify(&ctx, &Caller::Desktop, &id, kind).map_err(IpcError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn studio_adopt_legacy(
    ctx: Ctx<'_>,
    job_id: String,
    on_event: Channel<DownloadProgress>,
) -> CmdResult<MigrationResult> {
    service::studio::adopt_legacy(
        &ctx,
        &Caller::Desktop,
        &job_id,
        Arc::new(move |p| {
            let _ = on_event.send(p);
        }),
    )
    .await
    .map_err(IpcError::from)
}
#[tauri::command]
#[specta::specta]
pub async fn studio_pack_import(
    ctx: Ctx<'_>,
    job_id: String,
    path: String,
    on_event: Channel<DownloadProgress>,
) -> CmdResult<MigrationResult> {
    service::studio::import_pack(
        &ctx,
        &Caller::Desktop,
        &job_id,
        &path,
        Arc::new(move |p| {
            let _ = on_event.send(p);
        }),
    )
    .await
    .map_err(IpcError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn studio_orphans(ctx: Ctx<'_>) -> CmdResult<OrphanPreview> {
    service::studio::orphan_preview(&ctx, &Caller::Desktop).map_err(IpcError::from)
}
#[tauri::command]
#[specta::specta]
pub async fn studio_reclaim(ctx: Ctx<'_>, sha256s: Vec<String>) -> CmdResult<ReclaimResult> {
    service::studio::reclaim_orphans(&ctx, &Caller::Desktop, sha256s).map_err(IpcError::from)
}
