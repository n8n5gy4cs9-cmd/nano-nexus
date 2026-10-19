//! Activity page commands and the `observe:snapshot` event. Desktop only (ADR-017).

use std::sync::Arc;

use nexus_observe::{ObserveView, ActivitySnapshot};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;

use super::CmdResult;
use crate::service;
use crate::state::AppState;

type Ctx<'a> = State<'a, Arc<AppState>>;

/// Global event `observe:snapshot`, about every 2 s while the Activity page is open.
#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
#[tauri_specta(event_name = "observe:snapshot")]
pub struct ObserveSnapshot(pub ObserveView);

/// The latest picture of what is running.
#[tauri::command]
#[specta::specta]
pub async fn observe_snapshot(ctx: Ctx<'_>) -> CmdResult<ObserveView> {
    let c = ctx.inner().clone();
    tokio::task::spawn_blocking(move || service::observe::view(&c))
        .await
        .map_err(|e| nexus_core::error::NexusError::Internal(e.to_string()).into())
}

/// The frame nearest before `at_ms` (replay).
#[tauri::command]
#[specta::specta]
pub async fn observe_replay(ctx: Ctx<'_>, at_ms: f64) -> CmdResult<Option<ActivitySnapshot>> {
    Ok(service::observe::replay(&ctx, at_ms as i64))
}

/// Show synthetic demo data instead of real activity.
#[tauri::command]
#[specta::specta]
pub async fn observe_demo(ctx: Ctx<'_>, on: bool) -> CmdResult<ObserveView> {
    let c = ctx.inner().clone();
    tokio::task::spawn_blocking(move || service::observe::set_demo(&c, on))
        .await
        .map_err(|e| nexus_core::error::NexusError::Internal(e.to_string()).into())
}

/// The page opened (`true`) or closed (`false`): controls live updates.
#[tauri::command]
#[specta::specta]
pub async fn observe_watch(ctx: Ctx<'_>, on: bool) -> CmdResult<()> {
    service::observe::watch(&ctx, on).map_err(Into::into)
}
