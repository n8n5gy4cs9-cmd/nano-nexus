use std::sync::Arc;

use tauri::State;

use super::CmdResult;
use crate::service::{self, Caller};
use crate::state::AppState;

pub use crate::service::app::AppInfo;

#[tauri::command]
#[specta::specta]
pub async fn app_info(ctx: State<'_, Arc<AppState>>) -> CmdResult<AppInfo> {
    Ok(service::app::info(&ctx, &Caller::Desktop))
}

/// Open a web link in the OS browser (https/http only).
#[tauri::command]
#[specta::specta]
pub async fn open_external(app: tauri::AppHandle, url: String) -> CmdResult<()> {
    use nexus_core::error::IpcError;
    use tauri_plugin_opener::OpenerExt;
    service::app::validate_external_url(&url).map_err(IpcError::from)?;
    app.opener()
        .open_url(&url, None::<&str>)
        .map_err(|e| IpcError::from(nexus_core::error::NexusError::Internal(e.to_string())))
}
