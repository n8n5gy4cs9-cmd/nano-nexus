use nexus_core::error::{IpcError, NexusError};
use nexus_core::events::{NotificationAction, NotificationLevel, NotificationPayload};
use nexus_store::NewNotification;
use tauri::{AppHandle, State};

use super::CmdResult;
use crate::state::AppState;

fn ipc(e: nexus_store::StoreError) -> IpcError {
    IpcError::from(NexusError::from(e))
}

/// Newest first.
#[tauri::command]
#[specta::specta]
pub async fn notifications_list(
    state: State<'_, AppState>,
    limit: u32,
) -> CmdResult<Vec<NotificationPayload>> {
    state.store.list_notifications(limit.min(200)).map_err(ipc)
}

#[tauri::command]
#[specta::specta]
pub async fn notifications_unread_count(state: State<'_, AppState>) -> CmdResult<u32> {
    state.store.unread_notification_count().map_err(ipc)
}

/// Mark the given ids read; an empty list marks everything read.
#[tauri::command]
#[specta::specta]
pub async fn notifications_mark_read(state: State<'_, AppState>, ids: Vec<String>) -> CmdResult<()> {
    state.store.mark_notifications_read(&ids).map_err(ipc)
}

#[tauri::command]
#[specta::specta]
pub async fn notifications_clear(state: State<'_, AppState>) -> CmdResult<()> {
    state.store.clear_notifications().map_err(ipc)
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
