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
