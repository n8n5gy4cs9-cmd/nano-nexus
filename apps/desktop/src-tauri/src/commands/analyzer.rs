//! Analyzer commands: folder scans with progress, saved reports, exports, and cleanup plans.

use std::sync::Arc;

use nexus_analyze::{DupMode, ScanProgress};
use nexus_core::error::{IpcError, NexusError};
use tauri::State;
use tauri::ipc::Channel;

use super::CmdResult;
use crate::service::analyzer::{CleanupPlan, ExportFile, ExportFormat, FolderReportView, ReportSummary};
use crate::service;
use crate::state::AppState;

type Ctx<'a> = State<'a, Arc<AppState>>;

fn ipc(e: NexusError) -> IpcError {
    IpcError::from(e)
}

/// Scan a folder (`duplicates`: `off | fast | full`). Progress arrives on `on_progress` (about 10 times a second).
/// Returns the saved report's id.
#[tauri::command]
#[specta::specta]
pub async fn analyze_folder(
    ctx: Ctx<'_>,
    job_id: String,
    root: String,
    duplicates: String,
    on_progress: Channel<ScanProgress>,
) -> CmdResult<String> {
    let mode = match duplicates.as_str() {
        "off" => DupMode::Off,
        "full" => DupMode::Full,
        _ => DupMode::Fast,
    };
    let progress: Arc<dyn Fn(ScanProgress) + Send + Sync> = Arc::new(move |p| {
        let _ = on_progress.send(p);
    });
    service::analyzer::analyze_folder(&ctx, &job_id, &root, mode, progress).await.map_err(ipc)
}

#[tauri::command]
#[specta::specta]
pub async fn analyze_cancel(ctx: Ctx<'_>, job_id: String) -> CmdResult<bool> {
    Ok(service::analyzer::cancel(&ctx, &job_id))
}

#[tauri::command]
#[specta::specta]
pub async fn reports_list(ctx: Ctx<'_>, kind: Option<String>, limit: u32) -> CmdResult<Vec<ReportSummary>> {
    service::analyzer::reports_list(&ctx, kind.as_deref(), limit).map_err(ipc)
}

#[tauri::command]
#[specta::specta]
pub async fn report_folder(ctx: Ctx<'_>, id: String) -> CmdResult<FolderReportView> {
    service::analyzer::report_folder(&ctx, &id).map_err(ipc)
}

/// The JSON body of a file / secrets / logs report.
#[tauri::command]
#[specta::specta]
pub async fn report_body(ctx: Ctx<'_>, id: String) -> CmdResult<String> {
    service::analyzer::report_body(&ctx, &id).map_err(ipc)
}

#[tauri::command]
#[specta::specta]
pub async fn report_delete(ctx: Ctx<'_>, id: String) -> CmdResult<bool> {
    service::analyzer::report_delete(&ctx, &id).map_err(ipc)
}

#[tauri::command]
#[specta::specta]
pub async fn report_narrative_save(ctx: Ctx<'_>, id: String, text: String) -> CmdResult<()> {
    service::analyzer::report_narrative_save(&ctx, &id, &text).map_err(ipc)
}

#[tauri::command]
#[specta::specta]
pub async fn report_export(ctx: Ctx<'_>, id: String, format: ExportFormat) -> CmdResult<ExportFile> {
    service::analyzer::report_export(&ctx, &id, format).map_err(ipc)
}

/// Build a report for one file; returns the report id.
#[tauri::command]
#[specta::specta]
pub async fn file_report(ctx: Ctx<'_>, path: String) -> CmdResult<String> {
    service::analyzer::file_report_for(&ctx, &path).await.map_err(ipc)
}

/// Plan (not apply) a cleanup of duplicates and safe junk; review it on the Changes page.
#[tauri::command]
#[specta::specta]
pub async fn report_cleanup_plan(ctx: Ctx<'_>, id: String, include_duplicates: bool, include_junk: bool) -> CmdResult<CleanupPlan> {
    service::analyzer::cleanup_plan(&ctx, &id, include_duplicates, include_junk).await.map_err(ipc)
}
