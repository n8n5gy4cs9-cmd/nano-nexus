//! NanoNexus desktop shell: thin Tauri commands over the `nexus-*` crates.

pub mod bindings;
pub mod commands;
pub mod notify;
pub mod runtime;
pub mod runtime_paths;
pub mod state;

use std::sync::{Arc, RwLock};

use nexus_core::paths::AppPaths;
use nexus_llm::{Scheduler, Supervisor};
use nexus_store::Store;
use tauri::{Manager, RunEvent};
use tauri_specta::Event as _;

use crate::state::AppState;

/// Resolve the data and log folders: portable mode (`portable.flag` next to the exe) or the OS defaults.
fn resolve_paths(app: &tauri::App) -> Result<AppPaths, Box<dyn std::error::Error>> {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_owned()));
    if let Some(dir) = exe_dir.filter(|d| AppPaths::is_portable(d)) {
        return Ok(AppPaths::portable(&dir));
    }
    Ok(AppPaths::from_roots(
        app.path().app_data_dir()?,
        app.path().app_log_dir()?,
    ))
}

/// Forward every supervisor state change to the webview as `runtime:status`.
fn bridge_runtime_status(app: &tauri::AppHandle) {
    let app = app.clone();
    let mut rx = app.state::<AppState>().supervisor.subscribe();
    tauri::async_runtime::spawn(async move {
        while rx.changed().await.is_ok() {
            let state = rx.borrow_and_update().clone();
            if let Err(e) = commands::runtime::RuntimeStatus(state).emit(&app) {
                tracing::warn!("could not emit runtime:status: {e}");
            }
        }
    });
}

fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let paths = resolve_paths(app)?;
    paths.ensure_dirs()?;
    let store = Arc::new(Store::open(&paths.db_file())?);
    let settings = store.load_settings()?;
    let supervisor = Supervisor::with_files(
        Some(paths.logs.join("llama-server.log")),
        Some(paths.server_json()),
    );
    // The scheduler spawns its worker task, so it needs the async runtime.
    let scheduler = tauri::async_runtime::block_on(async { Scheduler::new(supervisor.clone()) });
    app.manage(AppState {
        paths,
        store,
        supervisor,
        scheduler,
        settings: RwLock::new(settings),
    });
    bridge_runtime_status(app.handle());
    runtime::start_in_background(app.handle());
    Ok(())
}

/// Build and run the Tauri application.
pub fn run() {
    let specta = bindings::builder();
    // Debug builds regenerate the TypeScript bindings on every launch.
    #[cfg(debug_assertions)]
    if let Err(e) =
        bindings::export(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/bindings.ts"))
    {
        eprintln!("could not export bindings.ts: {e}");
    }
    let built = tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .invoke_handler(specta.invoke_handler())
        .setup(move |app| {
            specta.mount_events(app);
            setup(app)
        })
        .build(tauri::generate_context!());
    let app = match built {
        Ok(app) => app,
        Err(e) => {
            eprintln!("error while building NanoNexus: {e}");
            std::process::exit(1);
        }
    };
    app.run(|handle, event| {
        // `kill_on_drop` is not enough: process::exit skips destructors, so stop the server explicitly.
        if let (true, Some(state)) = (
            matches!(event, RunEvent::ExitRequested { .. } | RunEvent::Exit),
            handle.try_state::<AppState>(),
        ) {
            tauri::async_runtime::block_on(state.supervisor.shutdown());
        }
    });
}
