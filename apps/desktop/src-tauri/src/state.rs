//! Shared application context. It holds no Tauri types, so `service::*` can run under any caller
//! (the desktop UI now, LAN Share remotes later) and in plain `cargo test`.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use nexus_agent::{TurnEngine, TurnRunner};
use nexus_core::ids::{ConversationId, MessageId};
use nexus_fs::{AuditSink, DenyList, DenyRule, PermissionEngine, builtin_rules};
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

    /// Cancel every running turn (app exit).
    pub fn cancel_all(&self) {
        if let Ok(m) = self.0.lock() {
            for t in m.values() {
                t.cancel();
            }
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
#[derive(Clone, Debug)]
pub enum AppEvent {
    /// A conversation's title or summary changed.
    ConversationUpdated(ConversationId),
    /// A tool needs a folder grant; the UI shows the permission modal.
    PermissionRequest(crate::service::permissions::PermissionRequest),
    /// A change set was planned, applied, undone, or discarded.
    ChangeSetUpdated {
        id: nexus_core::ids::ChangeSetId,
        status: String,
    },
    /// Progress of a model download (also sent on every status change).
    DownloadProgress(crate::service::models::DownloadView),
    /// The active model changed (the runtime is restarting on it).
    ModelChanged(String),
    /// Show an in-app (and maybe OS) notification.
    Notify(nexus_store::NewNotification),
    /// LAN Share started, stopped, or its devices changed.
    ShareStatus(nexus_core::share::ShareStatus),
    /// A job started, progressed, or ended (already throttled by the registry).
    JobUpdate(nexus_core::events::JobSummary),
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
    /// Every file access is decided here (grants + deny list + audit).
    pub perms: Arc<PermissionEngine>,
    pub broker: crate::service::permissions::PermissionBroker,
    /// Hugging Face API client (endpoint and token come from the environment).
    pub hf: Arc<nexus_models::hf::HfClient>,
    /// One download at a time.
    pub downloads: crate::service::models::DownloadManager,
    /// Folder analyses in flight, by job id (for cancel).
    pub analyses: crate::service::analyzer::Analyses,
    /// Rename Forge: loaded project indexes and running jobs.
    pub renames: crate::service::rename::RenameRuntime,
    /// Long-running work shown in the Jobs panel (scan, download, rename proposals).
    pub jobs: nexus_store::JobRegistry,
    /// Folder watches: file watchers and runs in flight.
    pub watches: crate::service::watches::WatchRuntime,
    /// Built-in and user assistants.
    pub registry: RwLock<nexus_agent::registry::Registry>,
    /// The only writer of user files (apply / undo with backups).
    pub journal: Arc<nexus_fs::Journal>,
    /// Queues audit entries and writes them in batches.
    pub audit: Arc<crate::audit::AuditWriter>,
    /// Conversations with a summary job in flight (at most one each).
    pub summarizing: Mutex<HashSet<ConversationId>>,
    pub bus: tokio::sync::broadcast::Sender<AppEvent>,
    pub settings: RwLock<Settings>,
    /// LAN Share server, pairing, and devices (M14).
    pub share: crate::service::share::ShareController,
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

    /// The permission engine: built-in deny list for this OS plus the app's data folder and the user's
    /// own entries, with the persisted grants loaded.
    pub fn build_permissions(
        store: &Store,
        home: &std::path::Path,
        app_data: &std::path::Path,
        audit: Arc<dyn AuditSink>,
    ) -> Arc<PermissionEngine> {
        let mut rules = builtin_rules(home, app_data);
        match store.list_user_deny() {
            Ok(user) => rules.extend(
                user.into_iter()
                    .map(|u| DenyRule::user(PathBuf::from(u.root), "your protected location")),
            ),
            Err(e) => tracing::warn!("could not load protected locations: {e}"),
        }
        let engine = Arc::new(PermissionEngine::new(DenyList::new(rules), audit));
        match store.list_grants() {
            Ok(grants) => engine.load_grants(grants),
            Err(e) => tracing::warn!("could not load folder grants: {e}"),
        }
        engine
    }

    /// The job registry: persisted in `store`, updates go to the event bus.
    pub fn build_jobs(store: &Arc<Store>, bus: &tokio::sync::broadcast::Sender<AppEvent>) -> nexus_store::JobRegistry {
        let bus = bus.clone();
        nexus_store::JobRegistry::new(
            store.clone(),
            Arc::new(move |j| {
                let _ = bus.send(AppEvent::JobUpdate(j));
            }),
        )
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
        Self::for_tests_full(root, engine, None)
    }

    /// Like [`AppState::for_tests_with`], with the Hugging Face API pointed at `hf_endpoint`.
    #[cfg(test)]
    pub fn for_tests_full(
        root: &std::path::Path,
        engine: Option<Arc<dyn TurnRunner>>,
        hf_endpoint: Option<String>,
    ) -> Ctx {
        let hf_client = Arc::new(
            nexus_models::hf::HfClient::new(
                Some(hf_endpoint.unwrap_or_else(|| "http://127.0.0.1:1".into())),
                None,
            )
            .expect("hf"),
        );
        let paths = AppPaths::from_roots(root.join("data"), root.join("logs"));
        paths.ensure_dirs().expect("dirs");
        let store = Arc::new(Store::open(&paths.db_file()).expect("store"));
        let supervisor = Supervisor::new(None);
        let scheduler = Scheduler::new(supervisor.clone());
        let engine = engine.unwrap_or_else(|| Self::default_engine(&scheduler, &supervisor));
        let audit = crate::audit::AuditWriter::start(store.clone());
        let perms = Self::build_permissions(&store, &root.join("home"), &paths.data, audit.clone());
        let journal = Arc::new(nexus_fs::Journal::new(
            paths.journal.clone(),
            Arc::new(nexus_fs::MoveToDirTrash::new(root.join("trash"))),
        ));
        let registry = RwLock::new(crate::service::assistants::load_registry(&store));
        let store_for_share = store.clone();
        let bus = tokio::sync::broadcast::channel(64).0;
        let jobs = Self::build_jobs(&store, &bus);
        Arc::new(AppState {
            paths,
            store,
            supervisor,
            scheduler,
            engine,
            chats: ChatJobs::default(),
            perms,
            broker: Default::default(),
            hf: hf_client,
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
            share: crate::service::share::ShareController::new(store_for_share, &Settings::default()),
            settings: RwLock::new(Settings::default()),
            resource_dir: root.join("resources"),
            exe_dir: root.join("bin"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_core::fs::{Access, GrantLevel, GrantScope};
    use nexus_fs::NewGrant;

    #[test]
    fn persisted_grants_and_user_deny_rules_load_at_startup() {
        let dir = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(dir.path()).unwrap();
        std::fs::create_dir_all(root.join("docs")).unwrap();
        std::fs::create_dir_all(root.join("private")).unwrap();
        let store = Store::open_in_memory().unwrap();
        let first = AppState::build_permissions(
            &store,
            &root.join("home"),
            &root.join("data"),
            Arc::new(nexus_fs::NullAudit),
        );
        let g = first
            .add_grant(NewGrant {
                root: root.join("docs"),
                level: GrantLevel::Read,
                recursive: true,
                scope: GrantScope::Persistent,
                expires_at: None,
                label: None,
            })
            .unwrap();
        store.save_grant(&g).unwrap();
        store
            .add_user_deny(&root.join("private").to_string_lossy())
            .unwrap();

        // "Restart": a fresh engine sees both.
        let second = AppState::build_permissions(
            &store,
            &root.join("home"),
            &root.join("data"),
            Arc::new(nexus_fs::NullAudit),
        );
        assert!(second.check("t", &root.join("docs"), Access::Read).is_ok());
        assert!(second.check("t", &root.join("private"), Access::Read).is_err());
        assert!(second.deny_rules().iter().any(|r| !r.builtin));
    }
}
