//! Shared application context. It holds no Tauri types, so `service::*` can run under any caller
//! (the desktop UI now, LAN Share remotes later) and in plain `cargo test`.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use nexus_agent::{TurnEngine, TurnRunner};
use nexus_core::ids::{ConversationId, MessageId};
use tokio_util::sync::CancellationToken;

use nexus_core::paths::AppPaths;
use nexus_core::settings::Settings;
use nexus_llm::{Scheduler, Supervisor};
use nexus_store::Store;

/// Cancellation tokens of running chat turns, keyed by the assistant message they fill.
#[derive(Default)]
pub struct ChatJobs(Mutex<HashMap<MessageId, CancellationToken>>);

impl ChatJobs {
    /// Register a turn; the returned token is cancelled by [`ChatJobs::cancel`].
    pub fn register(&self, id: &MessageId) -> CancellationToken {
        let token = CancellationToken::new();
        if let Ok(mut m) = self.0.lock() {
            m.insert(id.clone(), token.clone());
        }
        token
    }

    /// Cancel a running turn. `false` if it is not running.
    pub fn cancel(&self, id: &MessageId) -> bool {
        match self.0.lock().ok().and_then(|m| m.get(id).cloned()) {
            Some(t) => {
                t.cancel();
                true
            }
            None => false,
        }
    }

    /// Forget a finished turn.
    pub fn finish(&self, id: &MessageId) {
        if let Ok(mut m) = self.0.lock() {
            m.remove(id);
        }
    }

    pub fn is_running(&self, id: &MessageId) -> bool {
        self.0.lock().map(|m| m.contains_key(id)).unwrap_or(false)
    }
}

/// App-internal events that the shell forwards to the webview.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AppEvent {
    /// A conversation's title or summary changed.
    ConversationUpdated(ConversationId),
}

/// Everything services need. Later milestones add jobs, permissions, and the assistant registry.
pub struct AppState {
    pub paths: AppPaths,
    pub store: Arc<Store>,
    pub supervisor: Supervisor,
    pub scheduler: Scheduler,
    /// Runs chat turns (a scripted fake in tests).
    pub engine: Arc<dyn TurnRunner>,
    pub chats: ChatJobs,
    /// Conversations with a summary job in flight (at most one each).
    pub summarizing: Mutex<HashSet<ConversationId>>,
    pub bus: tokio::sync::broadcast::Sender<AppEvent>,
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

    /// The production turn engine over a scheduler and supervisor.
    pub fn default_engine(scheduler: &Scheduler, supervisor: &Supervisor) -> Arc<dyn TurnRunner> {
        Arc::new(TurnEngine::new(scheduler.clone(), supervisor.clone()))
    }

    /// A context over `root` with an on-disk store, for tests. Needs a tokio runtime.
    #[cfg(test)]
    pub fn for_tests(root: &std::path::Path) -> Ctx {
        Self::for_tests_with(root, None)
    }

    /// Like [`AppState::for_tests`], with a scripted turn runner.
    #[cfg(test)]
    pub fn for_tests_with(root: &std::path::Path, engine: Option<Arc<dyn TurnRunner>>) -> Ctx {
        let paths = AppPaths::from_roots(root.join("data"), root.join("logs"));
        paths.ensure_dirs().expect("dirs");
        let store = Arc::new(Store::open(&paths.db_file()).expect("store"));
        let supervisor = Supervisor::new(None);
        let scheduler = Scheduler::new(supervisor.clone());
        let engine = engine.unwrap_or_else(|| Self::default_engine(&scheduler, &supervisor));
        Arc::new(AppState {
            paths,
            store,
            supervisor,
            scheduler,
            engine,
            chats: ChatJobs::default(),
            summarizing: Default::default(),
            bus: tokio::sync::broadcast::channel(64).0,
            settings: RwLock::new(Settings::default()),
            resource_dir: root.join("resources"),
            exe_dir: root.join("bin"),
        })
    }
}
