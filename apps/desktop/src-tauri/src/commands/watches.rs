//! Watch commands: list, create, change, delete, and "Run now".

use std::sync::Arc;

use nexus_analyze::watch::ReportDelta;
use tauri::State;

use super::CmdResult;
use crate::service;
use crate::service::watches::WatchView;
use crate::state::AppState;

type Ctx<'a> = State<'a, Arc<AppState>>;

#[tauri::command]
#[specta::specta]
pub async fn watches_list(ctx: Ctx<'_>) -> CmdResult<Vec<WatchView>> {
    service::watches::list(&ctx).map_err(Into::into)
}

/// `schedule`: `daily | weekly | on_change`.
#[tauri::command]
#[specta::specta]
pub async fn watch_create(ctx: Ctx<'_>, root: String, schedule: String) -> CmdResult<WatchView> {
    service::watches::create(&ctx, &root, &schedule).map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub async fn watch_set_enabled(ctx: Ctx<'_>, id: String, enabled: bool) -> CmdResult<()> {
    service::watches::set_enabled(&ctx, &id, enabled).map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub async fn watch_set_schedule(ctx: Ctx<'_>, id: String, schedule: String) -> CmdResult<()> {
    service::watches::set_schedule(&ctx, &id, &schedule).map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub async fn watch_delete(ctx: Ctx<'_>, id: String) -> CmdResult<bool> {
    service::watches::delete(&ctx, &id).map_err(Into::into)
}

/// Scan now and compare with the last report. `None` for the first run (it saves the baseline).
#[tauri::command]
#[specta::specta]
pub async fn watch_run_now(ctx: Ctx<'_>, id: String) -> CmdResult<Option<ReportDelta>> {
    service::watches::run(&ctx, &id, true).await.map_err(Into::into)
}
