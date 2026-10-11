//! Jobs center commands and the `job:update` event.

use std::sync::Arc;

use nexus_core::events::JobSummary;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;

use super::CmdResult;
use crate::service;
use crate::state::AppState;

type Ctx<'a> = State<'a, Arc<AppState>>;

/// Global event `job:update`: a job started, progressed (at most 4 Hz), or ended.
#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
#[tauri_specta(event_name = "job:update")]
pub struct JobUpdate(pub JobSummary);

/// Running jobs first, then recent finished ones.
#[tauri::command]
#[specta::specta]
pub async fn jobs_list(ctx: Ctx<'_>, limit: u32) -> CmdResult<Vec<JobSummary>> {
    service::jobs::list(&ctx, limit).map_err(Into::into)
}

/// Cancel a running job; `false` when it already ended.
#[tauri::command]
#[specta::specta]
pub async fn job_cancel(ctx: Ctx<'_>, id: String) -> CmdResult<bool> {
    Ok(service::jobs::cancel(&ctx, &id))
}
