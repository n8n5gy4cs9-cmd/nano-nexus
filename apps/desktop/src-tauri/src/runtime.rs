//! Starting the model runtime in the background.

use std::path::PathBuf;

use nexus_llm::RuntimeConfig;
use tauri::{AppHandle, Manager};

use crate::runtime_paths::{self, ModelSearch};
use crate::state::AppState;

/// Directories the runtime and models are resolved from.
fn dirs(app: &AppHandle) -> Option<(PathBuf, PathBuf)> {
    let resource_dir = app.path().resource_dir().ok()?;
    let exe_dir = std::env::current_exe().ok()?.parent()?.to_owned();
    Some((resource_dir, exe_dir))
}

/// Build a launch config from the current settings, or `None` when no model is installed (the UI
/// then shows "Download a model").
pub fn build_config(app: &AppHandle) -> Option<RuntimeConfig> {
    let state = app.state::<AppState>();
    let settings = state.settings_snapshot();
    let (resource_dir, exe_dir) = dirs(app)?;
    let search = ModelSearch {
        // Developer conveniences exist in debug builds only.
        env_override: cfg!(debug_assertions)
            .then(|| std::env::var_os("NEXUS_MODEL_PATH").map(PathBuf::from))
            .flatten(),
        resource_dir: resource_dir.clone(),
        active_model_id: settings.models_active_model_id.clone(),
        vendor_dir: cfg!(debug_assertions)
            .then(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../vendor")),
    };
    let model = runtime_paths::resolve_model(&search)?;
    let mut cfg = RuntimeConfig::from_settings(
        &settings,
        runtime_paths::server_binary(&resource_dir, &exe_dir),
        model.path,
    );
    cfg.model_id = model.id;
    Some(cfg)
}

/// Start the server without blocking the UI: the window opens first, the pill shows progress.
pub fn start_in_background(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let Some(cfg) = build_config(&app) else {
            tracing::info!("no model found; runtime stays stopped");
            return;
        };
        let supervisor = app.state::<AppState>().supervisor.clone();
        if let Err(e) = supervisor.start(cfg).await {
            tracing::warn!("runtime did not start: {e}");
        }
    });
}
