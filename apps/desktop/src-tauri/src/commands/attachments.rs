use nexus_core::chat::AttachmentInput;
use nexus_core::error::{IpcError, NexusError};
use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;

use super::CmdResult;
use crate::service::{self, Caller};

/// Read files the user dropped onto the window (paths come from the OS drop event).
#[tauri::command]
#[specta::specta]
pub async fn attachments_read(paths: Vec<String>) -> CmdResult<Vec<AttachmentInput>> {
    paths
        .iter()
        .take(service::attachments::MAX_ATTACHMENTS)
        .map(|p| service::attachments::read_text_file(&Caller::Desktop, p))
        .collect::<Result<_, _>>()
        .map_err(IpcError::from)
}

/// Open the native file picker and read the chosen files. Empty when the user cancels.
#[tauri::command]
#[specta::specta]
pub async fn attachments_pick(app: AppHandle) -> CmdResult<Vec<AttachmentInput>> {
    let picked = tauri::async_runtime::spawn_blocking(move || app.dialog().file().blocking_pick_files())
        .await
        .map_err(|e| IpcError::from(NexusError::Internal(e.to_string())))?;
    let Some(files) = picked else {
        return Ok(Vec::new());
    };
    files
        .into_iter()
        .take(service::attachments::MAX_ATTACHMENTS)
        .filter_map(|f| f.into_path().ok())
        .map(|p| service::attachments::read_text_file(&Caller::Desktop, &p.to_string_lossy()))
        .collect::<Result<_, _>>()
        .map_err(IpcError::from)
}
