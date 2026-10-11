//! Starting the model runtime in the background.

use std::sync::Arc;

use tauri::{AppHandle, Manager};

use crate::service;
use crate::state::AppState;

/// Start the server without blocking the UI: the window opens first, the pill shows progress.
pub fn start_in_background(app: &AppHandle) {
    let ctx = Arc::clone(&app.state::<Arc<AppState>>());
    tauri::async_runtime::spawn(async move {
        let Some(cfg) = service::runtime::build_config(&ctx) else {
            tracing::info!("no model found; runtime stays stopped");
            return;
        };
        if let Err(e) = ctx.supervisor.start(cfg).await {
            tracing::warn!("runtime did not start: {e}");
        }
    });
}
