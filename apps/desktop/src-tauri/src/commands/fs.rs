//! Folder access, protected locations, file browsing, the activity log, and change sets.

use std::sync::Arc;

use nexus_core::changes::ChangePreview;
use nexus_core::error::{IpcError, NexusError};
use nexus_core::fs::{AuditRecord, Grant, GrantLevel};
use nexus_core::ids::ChangeSetId;
use nexus_fs::os::{self, PrivacyPane};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;

use super::CmdResult;
use crate::service::fs::{
    ChangeSetSummary, DenyRuleView, DirEntryInfo, FileStat, OsPermissionsStatus, TextFile, UndoDecision,
    UndoOutcome,
};
use crate::service::{self, Caller};
use crate::state::AppState;

type Ctx<'a> = State<'a, Arc<AppState>>;

/// Global event `changeset:update`: a change set was planned, applied, undone, or discarded.
#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
#[tauri_specta(event_name = "changeset:update")]
pub struct ChangeSetUpdate {
    pub id: ChangeSetId,
    pub status: String,
}

fn ipc(e: NexusError) -> IpcError {
    IpcError::from(e)
}

fn join_err(e: tauri::Error) -> IpcError {
    ipc(NexusError::Internal(e.to_string()))
}

// ---- grants ---------------------------------------------------------------------------------

#[tauri::command]
#[specta::specta]
pub async fn fs_grants_list(ctx: Ctx<'_>) -> CmdResult<Vec<Grant>> {
    Ok(service::fs::grants_list(&ctx))
}

#[tauri::command]
#[specta::specta]
pub async fn fs_grant_add(
    ctx: Ctx<'_>,
    root: String,
    level: GrantLevel,
    recursive: bool,
    label: Option<String>,
    expires_at: Option<i64>,
) -> CmdResult<Grant> {
    service::fs::grant_add(&ctx, &Caller::Desktop, &root, level, recursive, label, expires_at).map_err(ipc)
}

#[tauri::command]
#[specta::specta]
pub async fn fs_grant_update(
    ctx: Ctx<'_>,
    id: String,
    level: Option<GrantLevel>,
    recursive: Option<bool>,
    label: Option<String>,
    expires_at: Option<i64>,
) -> CmdResult<Grant> {
    // `None` keeps the current expiry; the UI sets a new one or revokes and re-adds for "never".
    let expiry = expires_at.map(Some);
    service::fs::grant_update(&ctx, &Caller::Desktop, &id, level, recursive, label, expiry).map_err(ipc)
}

#[tauri::command]
#[specta::specta]
pub async fn fs_grant_revoke(ctx: Ctx<'_>, id: String) -> CmdResult<bool> {
    service::fs::grant_revoke(&ctx, &Caller::Desktop, &id).map_err(ipc)
}

// ---- protected locations --------------------------------------------------------------------

#[tauri::command]
#[specta::specta]
pub async fn fs_deny_list(ctx: Ctx<'_>) -> CmdResult<Vec<DenyRuleView>> {
    Ok(service::fs::deny_list(&ctx))
}

#[tauri::command]
#[specta::specta]
pub async fn fs_deny_add(ctx: Ctx<'_>, root: String) -> CmdResult<()> {
    service::fs::deny_add(&ctx, &Caller::Desktop, &root).map_err(ipc)
}

#[tauri::command]
#[specta::specta]
pub async fn fs_deny_remove(ctx: Ctx<'_>, root: String) -> CmdResult<bool> {
    service::fs::deny_remove(&ctx, &Caller::Desktop, &root).map_err(ipc)
}

/// Native folder picker. `None` when the user cancels.
#[tauri::command]
#[specta::specta]
pub async fn fs_pick_folder(app: AppHandle) -> CmdResult<Option<String>> {
    let picked = tauri::async_runtime::spawn_blocking(move || app.dialog().file().blocking_pick_folder())
        .await
        .map_err(|e| ipc(NexusError::Internal(e.to_string())))?;
    Ok(picked
        .and_then(|p| p.into_path().ok())
        .map(|p| p.display().to_string()))
}

/// Native picker for any one file. `None` when the user cancels.
#[tauri::command]
#[specta::specta]
pub async fn fs_pick_any_file(app: AppHandle) -> CmdResult<Option<String>> {
    let picked = tauri::async_runtime::spawn_blocking(move || app.dialog().file().blocking_pick_file())
        .await
        .map_err(|e| ipc(NexusError::Internal(e.to_string())))?;
    Ok(picked
        .and_then(|p| p.into_path().ok())
        .map(|p| p.display().to_string()))
}

// ---- permission-checked reads ---------------------------------------------------------------

#[tauri::command]
#[specta::specta]
pub async fn fs_list_dir(ctx: Ctx<'_>, path: String) -> CmdResult<Vec<DirEntryInfo>> {
    service::fs::list_dir(&ctx, &Caller::Desktop, &path).map_err(ipc)
}

#[tauri::command]
#[specta::specta]
pub async fn fs_read_text(ctx: Ctx<'_>, path: String) -> CmdResult<TextFile> {
    service::fs::read_text(&ctx, &Caller::Desktop, &path).map_err(ipc)
}

#[tauri::command]
#[specta::specta]
pub async fn fs_stat(ctx: Ctx<'_>, path: String) -> CmdResult<FileStat> {
    service::fs::stat(&ctx, &Caller::Desktop, &path).map_err(ipc)
}

// ---- OS consent -----------------------------------------------------------------------------

/// Probe the OS-protected folders. Only runs when the Permissions page asks (macOS may prompt).
#[tauri::command]
#[specta::specta]
pub async fn os_permissions_status(app: AppHandle) -> CmdResult<OsPermissionsStatus> {
    let home = app.path().home_dir().map_err(join_err)?;
    service::fs::os_permissions_status(home).await.map_err(ipc)
}

/// Open System Settings on the right Privacy pane (`files` or `full_disk`). No-op off macOS.
#[tauri::command]
#[specta::specta]
pub async fn os_open_privacy_settings(app: AppHandle, pane: String) -> CmdResult<()> {
    use tauri_plugin_opener::OpenerExt;
    if !cfg!(target_os = "macos") {
        return Ok(());
    }
    let pane = match pane.as_str() {
        "full_disk" => PrivacyPane::FullDiskAccess,
        _ => PrivacyPane::FilesAndFolders,
    };
    app.opener()
        .open_url(os::privacy_deep_link(pane), None::<&str>)
        .map_err(|e| ipc(NexusError::Internal(e.to_string())))
}

// ---- activity log and change sets -----------------------------------------------------------

#[tauri::command]
#[specta::specta]
pub async fn audit_list(ctx: Ctx<'_>, limit: u32, before: Option<i64>) -> CmdResult<Vec<AuditRecord>> {
    service::fs::audit_list(&ctx, limit, before).map_err(ipc)
}

#[tauri::command]
#[specta::specta]
pub async fn changesets_list(ctx: Ctx<'_>, limit: u32) -> CmdResult<Vec<ChangeSetSummary>> {
    service::fs::changesets_list(&ctx, limit).map_err(ipc)
}

#[tauri::command]
#[specta::specta]
pub async fn changeset_preview(ctx: Ctx<'_>, id: String) -> CmdResult<ChangePreview> {
    let ctx = ctx.inner().clone();
    tauri::async_runtime::spawn_blocking(move || service::fs::changeset_preview(&ctx, &id))
        .await
        .map_err(join_err)?
        .map_err(ipc)
}

#[tauri::command]
#[specta::specta]
pub async fn changeset_apply(ctx: Ctx<'_>, id: String) -> CmdResult<ChangeSetSummary> {
    let ctx = ctx.inner().clone();
    tauri::async_runtime::spawn_blocking(move || service::fs::changeset_apply(&ctx, &Caller::Desktop, &id))
        .await
        .map_err(join_err)?
        .map_err(ipc)
}

#[tauri::command]
#[specta::specta]
pub async fn changeset_undo(
    ctx: Ctx<'_>,
    id: String,
    decisions: Vec<UndoDecision>,
) -> CmdResult<UndoOutcome> {
    let ctx = ctx.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        service::fs::changeset_undo(&ctx, &Caller::Desktop, &id, decisions)
    })
    .await
    .map_err(join_err)?
    .map_err(ipc)
}

#[tauri::command]
#[specta::specta]
pub async fn changeset_discard(ctx: Ctx<'_>, id: String) -> CmdResult<()> {
    service::fs::changeset_discard(&ctx, &Caller::Desktop, &id).map_err(ipc)
}

/// Dev only (T05.14): plan a fixture change set to try the Changes page.
#[tauri::command]
#[specta::specta]
pub async fn debug_make_test_changeset(ctx: Ctx<'_>) -> CmdResult<ChangeSetId> {
    service::fs::debug_make_test_changeset(&ctx).map_err(ipc)
}
