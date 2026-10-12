//! NanoNexus desktop shell: thin Tauri commands over the `nexus-*` crates.

pub mod audit;
pub mod bindings;
pub mod commands;
pub mod notify;
pub mod runtime;
pub mod runtime_paths;
pub mod service;
pub mod state;

use std::sync::{Arc, RwLock};

use nexus_core::paths::AppPaths;
use nexus_llm::{Scheduler, Supervisor};
use nexus_store::Store;
use tauri::{Manager, RunEvent};
use tauri_specta::Event as _;

use crate::state::{AppState, Ctx};

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
    let mut rx = app.state::<Ctx>().supervisor.subscribe();
    tauri::async_runtime::spawn(async move {
        while rx.changed().await.is_ok() {
            let state = rx.borrow_and_update().clone();
            if let Err(e) = commands::runtime::RuntimeStatus(state).emit(&app) {
                tracing::warn!("could not emit runtime:status: {e}");
            }
        }
    });
}

/// Forward app-internal events (`AppEvent`) to the webview.
fn bridge_app_events(app: &tauri::AppHandle) {
    let app = app.clone();
    let mut rx = app.state::<Ctx>().bus.subscribe();
    tauri::async_runtime::spawn(async move {
        while let Ok(event) = rx.recv().await {
            match event {
                state::AppEvent::ConversationUpdated(id) => {
                    if let Err(e) = commands::chat::ConversationUpdated(id).emit(&app) {
                        tracing::warn!("could not emit conversation:updated: {e}");
                    }
                }
                state::AppEvent::ChangeSetUpdated { id, status } => {
                    if let Err(e) = (commands::fs::ChangeSetUpdate { id, status }).emit(&app) {
                        tracing::warn!("could not emit changeset:update: {e}");
                    }
                }
                state::AppEvent::DownloadProgress(v) => {
                    if let Err(e) = commands::models::DownloadProgressEvent(v).emit(&app) {
                        tracing::warn!("could not emit download:progress: {e}");
                    }
                }
                state::AppEvent::ModelChanged(id) => {
                    if let Err(e) = commands::models::ModelChanged(id).emit(&app) {
                        tracing::warn!("could not emit model:changed: {e}");
                    }
                }
                state::AppEvent::ModelsListChanged => {
                    if let Err(e) = commands::models::ModelsListChanged.emit(&app) {
                        tracing::warn!("could not emit models:list_changed: {e}");
                    }
                }
                state::AppEvent::ShareStatus(st) => {
                    if let Err(e) = commands::share::ShareStatusChanged(st).emit(&app) {
                        tracing::warn!("could not emit share:status: {e}");
                    }
                }
                state::AppEvent::JobUpdate(j) => {
                    if let Err(e) = commands::jobs::JobUpdate(j).emit(&app) {
                        tracing::warn!("could not emit job:update: {e}");
                    }
                }
                state::AppEvent::Notify(n) => {
                    if let Err(e) = notify::notify(&app, n) {
                        tracing::warn!("could not show a notification: {e}");
                    }
                }
                state::AppEvent::PermissionRequest(req) => {
                    let focused = app
                        .get_webview_window("main")
                        .and_then(|w| w.is_focused().ok())
                        .unwrap_or(false);
                    if !focused {
                        let _ = notify::notify(
                            &app,
                            nexus_store::NewNotification {
                                level: nexus_core::events::NotificationLevel::Info,
                                kind: "permission".into(),
                                title: format!("{} needs access", req.assistant),
                                body: Some(req.path.clone()),
                                action: None,
                            },
                        );
                    }
                    if let Err(e) = commands::permissions::PermissionRequested(req).emit(&app) {
                        tracing::warn!("could not emit permission:request: {e}");
                    }
                }
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
    let resource_dir = app.path().resource_dir()?;
    let exe_dir = std::env::current_exe()?
        .parent()
        .map(|d| d.to_owned())
        .ok_or("the executable has no parent directory")?;
    let interrupted = store.recover_interrupted_messages()?;
    if interrupted > 0 {
        tracing::info!(interrupted, "marked interrupted messages as stopped");
    }
    let engine = AppState::default_engine(&scheduler, &supervisor);
    let home = app.path().home_dir()?;
    // The writer task needs the async runtime, like the scheduler.
    let audit = tauri::async_runtime::block_on(async { audit::AuditWriter::start(store.clone()) });
    let perms = AppState::build_permissions(&store, &home, &paths.data, audit.clone());
    let journal = Arc::new(nexus_fs::Journal::new(
        paths.journal.clone(),
        Arc::new(nexus_fs::SystemTrash),
    ));
    let recovered = journal.recover_interrupted();
    if !recovered.is_empty() {
        tracing::warn!(
            count = recovered.len(),
            "rolled back change sets interrupted by a crash"
        );
    }
    let registry = RwLock::new(service::assistants::load_registry(&store));
    let bus = tokio::sync::broadcast::channel(64).0;
    let jobs = AppState::build_jobs(&store, &bus);
    match jobs.recover() {
        Ok(0) => {}
        Ok(n) => tracing::info!(n, "marked jobs interrupted by a restart as failed"),
        Err(e) => tracing::warn!("could not recover jobs: {e}"),
    }
    let share = service::share::ShareController::new(store.clone(), &settings);
    app.manage(Ctx::new(AppState {
        paths,
        store,
        supervisor,
        scheduler,
        engine,
        chats: Default::default(),
        perms,
        broker: Default::default(),
        hf: Arc::new(nexus_models::hf::HfClient::new(
            None,
            std::env::var("HF_TOKEN").ok().filter(|t| !t.is_empty()),
        )?),
        downloads: Default::default(),
        analyses: Default::default(),
        renames: Default::default(),
        jobs,
        watches: Default::default(),
        registry,
        journal,
        audit,
        summarizing: Default::default(),
        bus,
        settings: RwLock::new(settings),
        share,
        resource_dir,
        exe_dir,
    }));
    bridge_runtime_status(app.handle());
    bridge_app_events(app.handle());
    service::models::init(&app.state::<Ctx>());
    let share_ctx: Ctx = (*app.state::<Ctx>()).clone();
    tauri::async_runtime::spawn(service::share::init(share_ctx));
    // Bundled knowledge libraries: register and (incrementally) index. A no-op without `resources/knowledge`.
    let kb_ctx: Ctx = (*app.state::<Ctx>()).clone();
    tauri::async_runtime::spawn(async move { service::knowledge::sync_bundled(&kb_ctx).await });
    tauri::async_runtime::spawn(service::watches::run_scheduler((*app.state::<Ctx>()).clone()));
    runtime::start_in_background(app.handle());
    exit_on_signals(app.handle());
    Ok(())
}

/// Stop everything this app started: running answers, the LAN share server, `llama-server` (its whole
/// process group), and the audit queue. Safe to call more than once.
async fn shutdown_all(ctx: &Ctx) {
    ctx.chats.cancel_all();
    service::share::stop(ctx).await;
    ctx.supervisor.shutdown().await;
    ctx.audit.flush().await;
}

/// Ctrl-C (`tauri dev`) and `kill` end the app through the normal exit path, so the model server is
/// stopped too instead of being left behind as an orphan.
fn exit_on_signals(app: &tauri::AppHandle) {
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{SignalKind, signal};
            let (Ok(mut term), Ok(mut int), Ok(mut hup)) = (
                signal(SignalKind::terminate()),
                signal(SignalKind::interrupt()),
                signal(SignalKind::hangup()),
            ) else {
                return;
            };
            tokio::select! {
                _ = term.recv() => {}
                _ = int.recv() => {}
                _ = hup.recv() => {}
            }
        }
        #[cfg(not(unix))]
        {
            if tokio::signal::ctrl_c().await.is_err() {
                return;
            }
        }
        tracing::info!("termination signal received; shutting down");
        handle.exit(0);
    });
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
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
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
            handle.try_state::<Ctx>(),
        ) {
            let ctx: Ctx = (*state).clone();
            tauri::async_runtime::block_on(shutdown_all(&ctx));
        }
    });
}
