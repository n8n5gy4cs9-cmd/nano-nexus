use std::sync::Arc;

use nexus_core::error::IpcError;
use nexus_core::events::{NotificationAction, NotificationLevel, NotificationPayload};
use nexus_store::NewNotification;
use tauri::{AppHandle, State};

use super::CmdResult;
use crate::service;
use crate::state::AppState;

/// Newest first.
#[tauri::command]
#[specta::specta]
pub async fn notifications_list(
    ctx: State<'_, Arc<AppState>>,
    limit: u32,
) -> CmdResult<Vec<NotificationPayload>> {
    service::notifications::list(&ctx, limit).map_err(IpcError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn notifications_unread_count(ctx: State<'_, Arc<AppState>>) -> CmdResult<u32> {
    service::notifications::unread_count(&ctx).map_err(IpcError::from)
}

/// Mark the given ids read; an empty list marks everything read.
#[tauri::command]
#[specta::specta]
pub async fn notifications_mark_read(ctx: State<'_, Arc<AppState>>, ids: Vec<String>) -> CmdResult<()> {
    service::notifications::mark_read(&ctx, &ids).map_err(IpcError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn notifications_clear(ctx: State<'_, Arc<AppState>>) -> CmdResult<()> {
    service::notifications::clear(&ctx).map_err(IpcError::from)
}

/// "Send test notification" (shown in dev builds).
#[tauri::command]
#[specta::specta]
pub async fn notifications_send_test(app: AppHandle) -> CmdResult<NotificationPayload> {
    crate::notify::notify(
        &app,
        NewNotification {
            level: NotificationLevel::Info,
            kind: "test".into(),
            title: "NanoNexus test notification".into(),
            body: Some("If you can read this, notifications work.".into()),
            action: Some(NotificationAction {
                label: "Open Models".into(),
                route: "/models".into(),
            }),
        },
    )
    .map_err(IpcError::from)
}
