//! Shared application context. It holds no Tauri types, so `service::*` can run under any caller
//! (the desktop UI now, LAN Share remotes later) and in plain `cargo test`.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use nexus_core::paths::AppPaths;
use nexus_core::settings::Settings;
use nexus_llm::{Scheduler, Supervisor};
use nexus_store::Store;

/// Everything services need. Later milestones add jobs, permissions, and the assistant registry.
pub struct AppState {
    pub paths: AppPaths,
    pub store: Arc<Store>,
    pub supervisor: Supervisor,
    pub scheduler: Scheduler,
    pub settings: RwLock<Settings>,
    /// Tauri's `resource_dir()` (bundled models, Windows runtime).
    pub resource_dir: PathBuf,
    /// Directory of the running executable (macOS sidecar, portable data, `models-extra`).
    pub exe_dir: PathBuf,
}

/// The name services use for the shared context.
pub type AppCtx = AppState;

/// What Tauri manages and commands receive: a shared handle that spawned tasks can keep.
pub type Ctx = Arc<AppCtx>;

impl AppState {
    /// A copy of the current settings.
    pub fn settings_snapshot(&self) -> Settings {
        self.settings.read().map(|s| s.clone()).unwrap_or_default()
    }

    /// A context over `root` with an in-memory-free, on-disk store, for tests. Needs a tokio runtime.
    #[cfg(test)]
    pub fn for_tests(root: &std::path::Path) -> Ctx {
        let paths = AppPaths::from_roots(root.join("data"), root.join("logs"));
        paths.ensure_dirs().expect("dirs");
        let store = Arc::new(Store::open(&paths.db_file()).expect("store"));
        let supervisor = Supervisor::new(None);
        let scheduler = Scheduler::new(supervisor.clone());
        Arc::new(AppState {
            paths,
            store,
            supervisor,
            scheduler,
            settings: RwLock::new(Settings::default()),
            resource_dir: root.join("resources"),
            exe_dir: root.join("bin"),
        })
    }
}
