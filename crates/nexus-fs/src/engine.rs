//! `PermissionEngine::check` (PERMISSIONS §3): canonicalize, deny list first, then the best matching grant.

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use nexus_core::error::NexusError;
use nexus_core::fs::{Access, Grant, GrantLevel, GrantScope};
use nexus_core::ids::GrantId;

use crate::audit::{AuditEntry, AuditSink};
use crate::canon::{CanonError, CanonicalPath, canonicalize, is_direct_child, is_within, same_path};
use crate::deny::{DenyList, DenyRule};

/// Why an access was refused.
#[derive(Debug, thiserror::Error)]
pub enum FsError {
    #[error(transparent)]
    BadPath(#[from] CanonError),
    #[error("{path} is protected ({rule})")]
    Denied { path: PathBuf, rule: String },
    #[error("no {needed} access to {path}")]
    NoGrant { path: PathBuf, needed: &'static str },
}

impl FsError {
    /// The access that was missing, for permission prompts.
    pub fn needed(&self) -> Option<&'static str> {
        match self {
            FsError::NoGrant { needed, .. } => Some(needed),
            _ => None,
        }
    }
}

impl From<FsError> for NexusError {
    fn from(e: FsError) -> Self {
        match e {
            FsError::BadPath(c) => NexusError::InvalidInput(c.to_string()),
            FsError::Denied { path, .. } => NexusError::PathDenied(path.display().to_string()),
            FsError::NoGrant { path, .. } => NexusError::PermissionDenied(path.display().to_string()),
        }
    }
}

/// A grant to create. The root is canonicalized (it must exist).
#[derive(Clone, Debug)]
pub struct NewGrant {
    pub root: PathBuf,
    pub level: GrantLevel,
    pub recursive: bool,
    pub scope: GrantScope,
    pub expires_at: Option<i64>,
    pub label: Option<String>,
}

type Clock = Box<dyn Fn() -> i64 + Send + Sync>;

fn system_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Decides every file access. Cheap to share (`Arc`) and safe to call from any thread.
pub struct PermissionEngine {
    grants: RwLock<Vec<Grant>>,
    deny: RwLock<DenyList>,
    audit: Arc<dyn AuditSink>,
    now: Clock,
}

impl PermissionEngine {
    pub fn new(deny: DenyList, audit: Arc<dyn AuditSink>) -> Self {
        Self {
            grants: RwLock::new(Vec::new()),
            deny: RwLock::new(deny),
            audit,
            now: Box::new(system_now),
        }
    }

    /// Replace the clock (tests).
    pub fn with_clock(mut self, now: impl Fn() -> i64 + Send + Sync + 'static) -> Self {
        self.now = Box::new(now);
        self
    }

    /// Load persisted grants at startup (their roots were canonical when stored).
    pub fn load_grants(&self, grants: Vec<Grant>) {
        if let Ok(mut g) = self.grants.write() {
            *g = grants;
        }
    }

    /// Create a grant. Refused when the root is itself protected: a grant can never open a deny rule.
    pub fn add_grant(&self, new: NewGrant) -> Result<Grant, FsError> {
        let root = canonicalize(&new.root)?;
        let denied = self
            .deny
            .read()
            .ok()
            .and_then(|d| d.find(root.as_path()).cloned());
        if let Some(rule) = denied {
            return Err(FsError::Denied {
                path: root.into_path_buf(),
                rule: rule.label,
            });
        }
        let grant = Grant {
            id: GrantId::new(),
            root: root.as_path().to_string_lossy().into_owned(),
            level: new.level,
            recursive: new.recursive,
            scope: new.scope,
            expires_at: new.expires_at,
            label: new.label,
            created_at: (self.now)(),
        };
        if let Ok(mut g) = self.grants.write() {
            // One grant per root: replace (a new choice supersedes the old one).
            g.retain(|x| !same_path(Path::new(&x.root), root.as_path()));
            g.push(grant.clone());
        }
        Ok(grant)
    }

    pub fn remove_grant(&self, id: &GrantId) -> bool {
        match self.grants.write() {
            Ok(mut g) => {
                let before = g.len();
                g.retain(|x| &x.id != id);
                g.len() != before
            }
            Err(_) => false,
        }
    }

    /// Current grants (including expired ones; they simply never match).
    pub fn grants(&self) -> Vec<Grant> {
        self.grants.read().map(|g| g.clone()).unwrap_or_default()
    }

    pub fn deny_rules(&self) -> Vec<DenyRule> {
        self.deny.read().map(|d| d.rules().to_vec()).unwrap_or_default()
    }

    pub fn add_user_deny(&self, root: PathBuf, label: &str) {
        if let Ok(mut d) = self.deny.write() {
            d.add_user(root, label);
        }
    }

    pub fn remove_user_deny(&self, root: &Path) -> bool {
        self.deny
            .write()
            .map(|mut d| d.remove_user(root))
            .unwrap_or(false)
    }

    /// The highest level any live grant gives `canonical`.
    fn best_level(&self, canonical: &Path) -> Option<GrantLevel> {
        let now = (self.now)();
        let grants = self.grants.read().ok()?;
        grants
            .iter()
            .filter(|g| !g.is_expired(now))
            .filter(|g| {
                let root = Path::new(&g.root);
                if g.recursive {
                    is_within(canonical, root)
                } else {
                    same_path(canonical, root) || is_direct_child(canonical, root)
                }
            })
            .map(|g| g.level)
            .max()
    }

    /// May `actor` perform `access` on `path`? On success returns the canonical path to use. Every call
    /// (allowed or denied) is audited.
    pub fn check(&self, actor: &str, path: &Path, access: Access) -> Result<CanonicalPath, FsError> {
        self.check_capped(actor, path, access, None)
    }

    /// Like [`PermissionEngine::check`], but no grant counts for more than `cap` (remote devices with the
    /// `chat_read` policy are capped at `Read`).
    pub fn check_capped(
        &self,
        actor: &str,
        path: &Path,
        access: Access,
        cap: Option<GrantLevel>,
    ) -> Result<CanonicalPath, FsError> {
        let result = self.decide(path, access, cap);
        let (shown, allowed, detail) = match &result {
            Ok(c) => (c.as_path().display().to_string(), true, None),
            Err(FsError::Denied { path, rule }) => (
                path.display().to_string(),
                false,
                Some(format!("protected: {rule}")),
            ),
            Err(FsError::NoGrant { path, needed }) => (
                path.display().to_string(),
                false,
                Some(format!("no {needed} grant")),
            ),
            Err(FsError::BadPath(e)) => (path.display().to_string(), false, Some(e.to_string())),
        };
        self.audit.record(AuditEntry {
            ts: (self.now)(),
            actor: actor.to_owned(),
            action: access.as_str().to_owned(),
            path: shown,
            allowed,
            detail,
        });
        result
    }

    fn decide(&self, path: &Path, access: Access, cap: Option<GrantLevel>) -> Result<CanonicalPath, FsError> {
        let canon = canonicalize(path)?;
        // Deny ALWAYS wins, before any grant is looked at.
        if let Some(rule) = self
            .deny
            .read()
            .ok()
            .and_then(|d| d.find(canon.as_path()).cloned())
        {
            return Err(FsError::Denied {
                path: canon.into_path_buf(),
                rule: rule.label,
            });
        }
        let level = self
            .best_level(canon.as_path())
            .map(|l| cap.map_or(l, |c| l.min(c)));
        match level {
            Some(l) if l.allows(access) => Ok(canon),
            _ => Err(FsError::NoGrant {
                path: canon.into_path_buf(),
                needed: access.as_str(),
            }),
        }
    }
}
