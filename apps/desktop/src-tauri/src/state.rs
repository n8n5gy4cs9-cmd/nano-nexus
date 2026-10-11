//! Shared application state, managed by Tauri and read by commands.

use std::sync::{Arc, RwLock};

use nexus_core::paths::AppPaths;
use nexus_core::settings::Settings;
use nexus_llm::{Scheduler, Supervisor};
use nexus_store::Store;

/// Everything commands need. Later milestones add jobs, permissions, and the assistant registry.
pub struct AppState {
    pub paths: AppPaths,
    pub store: Arc<Store>,
    pub supervisor: Supervisor,
    pub scheduler: Scheduler,
    pub settings: RwLock<Settings>,
}

impl AppState {
    /// A copy of the current settings.
    pub fn settings_snapshot(&self) -> Settings {
        self.settings.read().map(|s| s.clone()).unwrap_or_default()
    }
}
