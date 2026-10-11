//! Permission prompts (PERMISSIONS §4): a tool that needs a folder it has no grant for asks the user
//! and waits. "Allow once" is a session grant for exactly that path; "Allow folder" is persistent.
//! Requests time out after five minutes and count as Deny.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use nexus_core::error::NexusError;
use nexus_core::fs::{Access, GrantLevel, GrantScope};
use nexus_fs::NewGrant;
use serde::{Deserialize, Serialize};
use specta::Type;
use tokio::sync::oneshot;

use crate::state::{AppCtx, AppEvent};

pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(300);

/// Shown in the permission modal ("Folder Scout wants to read ~/Downloads").
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct PermissionRequest {
    pub id: String,
    pub assistant: String,
    pub path: String,
    pub needed: Access,
    /// Why ("to build a size and clutter report").
    pub reason: Option<String>,
    /// Set when a paired LAN device asked.
    pub requested_by: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum Decision {
    Deny,
    AllowOnce,
    /// Persistent grant on the folder at this level.
    AllowFolder { level: GrantLevel },
}

struct Pending {
    req: PermissionRequest,
    tx: oneshot::Sender<Decision>,
}

/// Tracks open prompts.
#[derive(Default)]
pub struct PermissionBroker {
    pending: Mutex<HashMap<String, Pending>>,
}

impl PermissionBroker {
    /// Register a prompt and return the receiver the tool awaits.
    pub fn open(&self, req: PermissionRequest) -> oneshot::Receiver<Decision> {
        let (tx, rx) = oneshot::channel();
        if let Ok(mut m) = self.pending.lock() {
            m.insert(req.id.clone(), Pending { req, tx });
        }
        rx
    }

    fn take(&self, id: &str) -> Option<Pending> {
        self.pending.lock().ok().and_then(|mut m| m.remove(id))
    }

    /// Prompts still waiting (for the UI to re-show after a reload).
    pub fn open_requests(&self) -> Vec<PermissionRequest> {
        self.pending.lock().map(|m| m.values().map(|p| p.req.clone()).collect()).unwrap_or_default()
    }
}

/// The level an access needs when the user says "allow".
fn level_for(access: Access) -> GrantLevel {
    match access {
        Access::Read => GrantLevel::Read,
        Access::Write => GrantLevel::ReadWrite,
        Access::Create | Access::Move | Access::Trash => GrantLevel::Full,
    }
}

/// Ask the user and wait for the answer (or the timeout, which is a Deny). A granted decision has
/// already been turned into a grant when this returns.
pub async fn ask(ctx: &AppCtx, req: PermissionRequest) -> Decision {
    let id = req.id.clone();
    let rx = ctx.broker.open(req.clone());
    let _ = ctx.bus.send(AppEvent::PermissionRequest(req));
    match tokio::time::timeout(REQUEST_TIMEOUT, rx).await {
        Ok(Ok(d)) => d,
        _ => {
            ctx.broker.take(&id);
            Decision::Deny
        }
    }
}

/// The UI's answer. Creates the grant (if any) and wakes the waiting tool. `false` if the prompt was
/// already answered or timed out.
pub fn respond(ctx: &AppCtx, id: &str, decision: Decision) -> Result<bool, NexusError> {
    let Some(p) = ctx.broker.take(id) else { return Ok(false) };
    match decision {
        Decision::Deny => {}
        Decision::AllowOnce => {
            let path = Path::new(&p.req.path);
            let is_file = path.is_file();
            ctx.perms.add_grant(NewGrant {
                root: path.to_owned(),
                level: level_for(p.req.needed),
                recursive: !is_file,
                scope: GrantScope::Session,
                expires_at: None,
                label: Some("allowed once".into()),
            })?;
        }
        Decision::AllowFolder { level } => {
            let path = Path::new(&p.req.path);
            let root = if path.is_dir() { path.to_owned() } else { path.parent().unwrap_or(path).to_owned() };
            let grant = ctx.perms.add_grant(NewGrant {
                root,
                level: level.max(level_for(p.req.needed)),
                recursive: true,
                scope: GrantScope::Persistent,
                expires_at: None,
                label: Some(p.req.assistant.clone()),
            })?;
            ctx.store.save_grant(&grant)?;
        }
    }
    let _ = p.tx.send(decision);
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AppState;

    fn req(path: &Path, needed: Access) -> PermissionRequest {
        PermissionRequest { id: "r1".into(), assistant: "folder-scout".into(), path: path.display().to_string(), needed, reason: None, requested_by: None }
    }

    #[tokio::test]
    async fn allow_folder_creates_a_persistent_grant_and_wakes_the_waiter() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppState::for_tests(dir.path());
        let target = dir.path().join("Downloads");
        std::fs::create_dir_all(&target).unwrap();
        let mut events = ctx.bus.subscribe();
        let c2 = ctx.clone();
        let t = target.clone();
        let waiter = tokio::spawn(async move { ask(&c2, req(&t, Access::Read)).await });
        let ev = events.recv().await.unwrap();
        assert!(matches!(ev, AppEvent::PermissionRequest(r) if r.id == "r1"));
        assert!(respond(&ctx, "r1", Decision::AllowFolder { level: GrantLevel::Read }).unwrap());
        assert_eq!(waiter.await.unwrap(), Decision::AllowFolder { level: GrantLevel::Read });
        assert!(ctx.perms.check("t", &target, Access::Read).is_ok());
        assert_eq!(ctx.store.list_grants().unwrap().len(), 1);
        assert!(!respond(&ctx, "r1", Decision::Deny).unwrap(), "already answered");
    }

    #[tokio::test]
    async fn allow_once_is_a_session_grant_that_is_not_stored_and_deny_grants_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppState::for_tests(dir.path());
        let f = dir.path().join("note.txt");
        std::fs::write(&f, "x").unwrap();
        let _rx = ctx.broker.open(req(&f, Access::Read));
        respond(&ctx, "r1", Decision::AllowOnce).unwrap();
        assert!(ctx.perms.check("t", &f, Access::Read).is_ok());
        assert!(ctx.perms.check("t", &dir.path().join("other.txt"), Access::Read).is_err(), "only that file");
        assert!(ctx.store.list_grants().unwrap().is_empty());

        let g = dir.path().join("secret.txt");
        std::fs::write(&g, "x").unwrap();
        let mut r = req(&g, Access::Read);
        r.id = "r2".into();
        let _rx2 = ctx.broker.open(r);
        respond(&ctx, "r2", Decision::Deny).unwrap();
        assert!(ctx.perms.check("t", &g, Access::Read).is_err());
    }
}
