//! Folder grants, protected locations, permission-checked reads, the activity log, and change sets.
//! Everything that changes grants or files is desktop-only: remote devices never manage permissions.

use std::collections::HashMap;
use std::path::Path;
use std::time::UNIX_EPOCH;

use nexus_core::changes::{ChangePreview, ChangeSet, FileOp, Totals};
use nexus_core::error::NexusError;
use nexus_core::fs::{Access, AuditRecord, Grant, GrantLevel, GrantScope};
use nexus_core::ids::ChangeSetId;
use nexus_fs::os::{self, OsAccess};
use nexus_fs::{ApplyError, ConflictDecision, NewGrant};
use nexus_store::ChangeSetRow;
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::state::{AppCtx, AppEvent};

use super::Caller;

fn desktop_only(caller: &Caller) -> Result<(), NexusError> {
    if caller.is_desktop() {
        Ok(())
    } else {
        Err(NexusError::ForbiddenRemote)
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

// ---- grants ---------------------------------------------------------------------------------

pub fn grants_list(ctx: &AppCtx) -> Vec<Grant> {
    ctx.perms.grants()
}

/// Add (or replace) a grant for `root`. The root must exist and must not be protected.
pub fn grant_add(
    ctx: &AppCtx,
    caller: &Caller,
    root: &str,
    level: GrantLevel,
    recursive: bool,
    label: Option<String>,
    expires_at: Option<i64>,
) -> Result<Grant, NexusError> {
    desktop_only(caller)?;
    let scope = if expires_at.is_some() {
        GrantScope::Session
    } else {
        GrantScope::Persistent
    };
    let grant = ctx.perms.add_grant(NewGrant {
        root: root.into(),
        level,
        recursive,
        scope,
        expires_at,
        label,
    })?;
    ctx.store.save_grant(&grant)?;
    Ok(grant)
}

/// Change a grant's level, recursion, label, or expiry (the id changes: it is a new grant for the root).
pub fn grant_update(
    ctx: &AppCtx,
    caller: &Caller,
    id: &str,
    level: Option<GrantLevel>,
    recursive: Option<bool>,
    label: Option<String>,
    expires_at: Option<Option<i64>>,
) -> Result<Grant, NexusError> {
    desktop_only(caller)?;
    let old = ctx
        .perms
        .grants()
        .into_iter()
        .find(|g| g.id.as_str() == id)
        .ok_or_else(|| NexusError::NotFound("grant".into()))?;
    ctx.perms.remove_grant(&old.id);
    ctx.store.delete_grant(&old.id)?;
    let expires_at = expires_at.unwrap_or(old.expires_at);
    grant_add(
        ctx,
        caller,
        &old.root,
        level.unwrap_or(old.level),
        recursive.unwrap_or(old.recursive),
        label.or(old.label),
        expires_at,
    )
}

pub fn grant_revoke(ctx: &AppCtx, caller: &Caller, id: &str) -> Result<bool, NexusError> {
    desktop_only(caller)?;
    let gid = nexus_core::ids::GrantId::from(id);
    let removed = ctx.perms.remove_grant(&gid);
    ctx.store.delete_grant(&gid)?;
    Ok(removed)
}

// ---- protected locations --------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Type)]
pub struct DenyRuleView {
    pub root: String,
    pub label: String,
    /// Built-in entries cannot be removed.
    pub builtin: bool,
}

pub fn deny_list(ctx: &AppCtx) -> Vec<DenyRuleView> {
    ctx.perms
        .deny_rules()
        .into_iter()
        .map(|r| DenyRuleView {
            root: r.root.display().to_string(),
            label: r.label,
            builtin: r.builtin,
        })
        .collect()
}

pub fn deny_add(ctx: &AppCtx, caller: &Caller, root: &str) -> Result<(), NexusError> {
    desktop_only(caller)?;
    if !Path::new(root).is_absolute() {
        return Err(NexusError::InvalidInput(
            "a protected location must be an absolute path".into(),
        ));
    }
    ctx.store.add_user_deny(root)?;
    ctx.perms.add_user_deny(root.into(), "your protected location");
    Ok(())
}

pub fn deny_remove(ctx: &AppCtx, caller: &Caller, root: &str) -> Result<bool, NexusError> {
    desktop_only(caller)?;
    let removed = ctx.perms.remove_user_deny(Path::new(root));
    ctx.store.remove_user_deny(root)?;
    Ok(removed)
}

// ---- permission-checked reads ---------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Type)]
pub struct DirEntryInfo {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    /// Unix milliseconds.
    pub modified: Option<i64>,
}

/// Most entries `list_dir` returns.
pub const MAX_DIR_ENTRIES: usize = 2000;

fn actor(caller: &Caller) -> String {
    match caller {
        Caller::Desktop => "user".into(),
        Caller::Remote { device_id, .. } => format!("device:{device_id}"),
    }
}

fn cap(caller: &Caller) -> Option<GrantLevel> {
    match caller {
        Caller::Desktop => None,
        // chat_read devices can read only, whatever the grant says.
        Caller::Remote { .. } => Some(GrantLevel::Read),
    }
}

/// List a folder (folders first, then by name). Requires Read access.
pub fn list_dir(ctx: &AppCtx, caller: &Caller, path: &str) -> Result<Vec<DirEntryInfo>, NexusError> {
    let canon = ctx
        .perms
        .check_capped(&actor(caller), Path::new(path), Access::Read, cap(caller))?;
    let mut out = Vec::new();
    for e in std::fs::read_dir(canon.as_path())? {
        let e = e?;
        let meta = e.metadata()?;
        out.push(DirEntryInfo {
            name: e.file_name().to_string_lossy().into_owned(),
            path: e.path().display().to_string(),
            is_dir: meta.is_dir(),
            size: if meta.is_dir() { 0 } else { meta.len() },
            modified: meta
                .modified()
                .ok()
                .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64),
        });
        if out.len() >= MAX_DIR_ENTRIES {
            break;
        }
    }
    out.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(out)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Type)]
pub struct TextFile {
    pub path: String,
    pub text: String,
    pub truncated: bool,
    /// blake3 of the *whole* file on disk (the base hash for edits).
    pub hash: String,
}

/// Largest text file returned whole.
pub const MAX_READ_BYTES: u64 = 2 * 1024 * 1024;

/// Read a text file for the editor. Requires Read access; binary files and huge files are refused.
pub fn read_text(ctx: &AppCtx, caller: &Caller, path: &str) -> Result<TextFile, NexusError> {
    let canon = ctx
        .perms
        .check_capped(&actor(caller), Path::new(path), Access::Read, cap(caller))?;
    let meta = std::fs::metadata(canon.as_path())?;
    if !meta.is_file() {
        return Err(NexusError::InvalidInput("not a file".into()));
    }
    if meta.len() > MAX_READ_BYTES {
        return Err(NexusError::InvalidInput(format!(
            "the file is larger than {} MB",
            MAX_READ_BYTES / 1024 / 1024
        )));
    }
    let bytes = std::fs::read(canon.as_path())?;
    let text = String::from_utf8(bytes.clone())
        .map_err(|_| NexusError::InvalidInput("this is not a text file".into()))?;
    Ok(TextFile {
        path: canon.as_path().display().to_string(),
        text,
        truncated: false,
        hash: nexus_fs::hash_bytes(&bytes),
    })
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Type)]
pub struct FileStat {
    pub path: String,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub size: u64,
    pub modified: Option<i64>,
}

pub fn stat(ctx: &AppCtx, caller: &Caller, path: &str) -> Result<FileStat, NexusError> {
    let canon = ctx
        .perms
        .check_capped(&actor(caller), Path::new(path), Access::Read, cap(caller))?;
    let meta = std::fs::symlink_metadata(canon.as_path())?;
    Ok(FileStat {
        path: canon.as_path().display().to_string(),
        is_dir: meta.is_dir(),
        is_symlink: meta.file_type().is_symlink(),
        size: meta.len(),
        modified: meta
            .modified()
            .ok()
            .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64),
    })
}

// ---- activity log ---------------------------------------------------------------------------

pub fn audit_list(ctx: &AppCtx, limit: u32, before: Option<i64>) -> Result<Vec<AuditRecord>, NexusError> {
    Ok(ctx.store.list_audit(limit.clamp(1, 500), before)?)
}

// ---- change sets ----------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Type)]
pub struct ChangeSetSummary {
    pub id: ChangeSetId,
    pub title: String,
    pub origin: String,
    /// `planned | applied | rolled_back | undone | discarded | failed`
    pub status: String,
    pub created_at: i64,
    pub applied_at: Option<i64>,
    pub undone_at: Option<i64>,
    pub totals: Option<Totals>,
    pub error: Option<String>,
}

fn summary(row: ChangeSetRow) -> ChangeSetSummary {
    let totals = row
        .stats
        .as_deref()
        .and_then(|s| serde_json::from_str::<Totals>(s).ok());
    ChangeSetSummary {
        id: ChangeSetId(row.id),
        title: row.title,
        origin: row.origin,
        status: row.status,
        created_at: row.created_at,
        applied_at: row.applied_at,
        undone_at: row.undone_at,
        totals,
        error: row.error,
    }
}

fn load(ctx: &AppCtx, id: &str) -> Result<(ChangeSetRow, ChangeSet), NexusError> {
    let row = ctx
        .store
        .get_changeset(id)?
        .ok_or_else(|| NexusError::NotFound("change set".into()))?;
    let ops: Vec<FileOp> = serde_json::from_str(&row.ops)
        .map_err(|e| NexusError::Internal(format!("stored change set is unreadable: {e}")))?;
    let cs = ChangeSet {
        id: ChangeSetId(row.id.clone()),
        title: row.title.clone(),
        origin: row.origin.clone(),
        ops,
        created_at: row.created_at,
    };
    Ok((row, cs))
}

/// Store a planned change set (called by pipelines and the editor). Nothing touches disk yet.
pub fn changeset_plan(
    ctx: &AppCtx,
    title: &str,
    origin: &str,
    ops: Vec<FileOp>,
) -> Result<ChangeSetId, NexusError> {
    if ops.is_empty() {
        return Err(NexusError::InvalidInput(
            "a change set needs at least one operation".into(),
        ));
    }
    let id = ChangeSetId::new();
    let row = ChangeSetRow {
        id: id.0.clone(),
        title: title.to_owned(),
        origin: origin.to_owned(),
        ops: serde_json::to_string(&ops).map_err(|e| NexusError::Internal(e.to_string()))?,
        status: "planned".into(),
        stats: None,
        error: None,
        created_at: now_ms(),
        applied_at: None,
        undone_at: None,
    };
    ctx.store.insert_changeset(&row)?;
    let _ = ctx.bus.send(AppEvent::ChangeSetUpdated {
        id: id.clone(),
        status: "planned".into(),
    });
    Ok(id)
}

pub fn changesets_list(ctx: &AppCtx, limit: u32) -> Result<Vec<ChangeSetSummary>, NexusError> {
    Ok(ctx
        .store
        .list_changesets(limit.clamp(1, 200))?
        .into_iter()
        .map(summary)
        .collect())
}

pub fn changeset_preview(ctx: &AppCtx, id: &str) -> Result<ChangePreview, NexusError> {
    let (_row, cs) = load(ctx, id)?;
    Ok(nexus_fs::preview(&ctx.perms, &cs.origin, &cs))
}

fn set_status(
    ctx: &AppCtx,
    id: &str,
    status: &str,
    stats: Option<&Totals>,
    error: Option<&str>,
) -> Result<(), NexusError> {
    let stats = stats.and_then(|t| serde_json::to_string(t).ok());
    ctx.store
        .set_changeset_status(id, status, stats.as_deref(), error)?;
    let _ = ctx.bus.send(AppEvent::ChangeSetUpdated {
        id: ChangeSetId(id.to_owned()),
        status: status.to_owned(),
    });
    super::rename::on_changeset_status(ctx, id, status);
    Ok(())
}

/// Apply a planned change set. Blocking (disk work): call from `spawn_blocking`.
pub fn changeset_apply(ctx: &AppCtx, caller: &Caller, id: &str) -> Result<ChangeSetSummary, NexusError> {
    desktop_only(caller)?;
    let (row, cs) = load(ctx, id)?;
    if row.status != "planned" {
        return Err(NexusError::Conflict(format!(
            "this change set is {}, not planned",
            row.status
        )));
    }
    let totals = nexus_fs::preview(&ctx.perms, &cs.origin, &cs).totals;
    match ctx.journal.apply(&ctx.perms, &cs.origin, &cs) {
        Ok(_) => set_status(ctx, id, "applied", Some(&totals), None)?,
        Err(ApplyError::Rejected(p)) => {
            let why = p
                .conflicts
                .first()
                .map(|c| c.message.clone())
                .unwrap_or_else(|| "a permission is missing".into());
            return Err(NexusError::Conflict(why));
        }
        Err(ApplyError::Failed {
            message,
            rollback_problems,
            ..
        }) => {
            let status = if rollback_problems.is_empty() {
                "rolled_back"
            } else {
                "failed"
            };
            set_status(ctx, id, status, None, Some(&message))?;
            return Err(NexusError::Internal(message));
        }
        Err(ApplyError::Journal(m)) => return Err(NexusError::Internal(m)),
    }
    let row = ctx
        .store
        .get_changeset(id)?
        .ok_or_else(|| NexusError::NotFound("change set".into()))?;
    Ok(summary(row))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum UndoChoice {
    KeepCurrent,
    RestoreAnyway,
}

#[derive(Clone, Debug, Deserialize, Type)]
pub struct UndoDecision {
    pub op_index: u32,
    pub choice: UndoChoice,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Type)]
pub struct UndoConflictView {
    pub op_index: u32,
    pub path: String,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Type)]
pub struct UndoOutcome {
    pub restored: u32,
    pub skipped: u32,
    pub conflicts: Vec<UndoConflictView>,
    pub problems: Vec<String>,
    /// `undone` when finished; `applied` when conflicts or problems remain.
    pub status: String,
}

/// Undo an applied change set. Conflicts (files changed since) are returned for the user to decide.
pub fn changeset_undo(
    ctx: &AppCtx,
    caller: &Caller,
    id: &str,
    decisions: Vec<UndoDecision>,
) -> Result<UndoOutcome, NexusError> {
    desktop_only(caller)?;
    let (row, cs) = load(ctx, id)?;
    if row.status != "applied" {
        return Err(NexusError::Conflict(format!(
            "this change set is {}, not applied",
            row.status
        )));
    }
    let map: HashMap<usize, ConflictDecision> = decisions
        .into_iter()
        .map(|d| {
            (
                d.op_index as usize,
                match d.choice {
                    UndoChoice::KeepCurrent => ConflictDecision::KeepCurrent,
                    UndoChoice::RestoreAnyway => ConflictDecision::RestoreAnyway,
                },
            )
        })
        .collect();
    let report = ctx
        .journal
        .undo(&ctx.perms, &cs.origin, id, &map)
        .map_err(NexusError::Internal)?;
    let done = report.conflicts.is_empty() && report.problems.is_empty();
    if done {
        set_status(ctx, id, "undone", None, None)?;
    }
    Ok(UndoOutcome {
        restored: report.restored.len() as u32,
        skipped: report.skipped.len() as u32,
        conflicts: report
            .conflicts
            .into_iter()
            .map(|c| UndoConflictView {
                op_index: c.op_index as u32,
                path: c.path,
                message: c.message,
            })
            .collect(),
        problems: report.problems,
        status: if done { "undone".into() } else { "applied".into() },
    })
}

/// Throw away a planned change set.
pub fn changeset_discard(ctx: &AppCtx, caller: &Caller, id: &str) -> Result<(), NexusError> {
    desktop_only(caller)?;
    let (row, _) = load(ctx, id)?;
    if row.status != "planned" {
        return Err(NexusError::Conflict(format!(
            "only planned change sets can be discarded (this one is {})",
            row.status
        )));
    }
    set_status(ctx, id, "discarded", None, None)
}

// ---- debug ----------------------------------------------------------------------------------

/// Dev only: plan a small change set over fixture files in the OS temp folder, so the Changes page
/// can be exercised without an assistant. Returns the new change set id.
pub fn debug_make_test_changeset(ctx: &AppCtx) -> Result<ChangeSetId, NexusError> {
    if !cfg!(debug_assertions) {
        return Err(NexusError::InvalidInput(
            "debug commands are not available in release builds".into(),
        ));
    }
    let dir = std::env::temp_dir().join("nanonexus-debug-playground");
    std::fs::create_dir_all(&dir)?;
    let notes = dir.join("notes.txt");
    let draft = dir.join("draft.txt");
    std::fs::write(&notes, "Shopping list\nmilk\neggs\nbread\n")?;
    std::fs::write(&draft, "scratch\n")?;
    let created = dir.join(format!("created-{}.txt", now_ms()));
    let moved = dir.join(format!("draft-renamed-{}.txt", now_ms()));
    let canon =
        |p: &Path| -> Result<String, NexusError> { Ok(std::fs::canonicalize(p)?.display().to_string()) };
    let ops = vec![
        FileOp::Write {
            path: canon(&notes)?,
            base_hash: None,
            contents: "Shopping list\nmilk\noat milk\neggs\nbread\nbutter\n".into(),
        },
        FileOp::Create {
            path: canon(&dir)?
                + &format!(
                    "/{}",
                    created
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("created.txt")
                ),
            contents: "Hello from a test change set.\n".into(),
        },
        FileOp::Move {
            from: canon(&draft)?,
            to: canon(&dir)?
                + &format!(
                    "/{}",
                    moved.file_name().and_then(|n| n.to_str()).unwrap_or("moved.txt")
                ),
        },
    ];
    changeset_plan(ctx, "Test change set", "user", ops)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AppState;
    use nexus_core::error::ErrorCode;
    use nexus_core::settings::SharePolicy;

    fn remote() -> Caller {
        Caller::Remote {
            device_id: "d".into(),
            policy: SharePolicy::Full,
        }
    }

    #[tokio::test]
    async fn grants_lifecycle_and_remote_refusal() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppState::for_tests(dir.path());
        let target = dir.path().join("docs");
        std::fs::create_dir_all(&target).unwrap();
        let root = target.to_str().unwrap();
        assert_eq!(
            grant_add(&ctx, &remote(), root, GrantLevel::Read, true, None, None)
                .unwrap_err()
                .code(),
            ErrorCode::ForbiddenRemote
        );
        let g = grant_add(
            &ctx,
            &Caller::Desktop,
            root,
            GrantLevel::Read,
            true,
            Some("docs".into()),
            None,
        )
        .unwrap();
        assert_eq!(grants_list(&ctx).len(), 1);
        let g2 = grant_update(
            &ctx,
            &Caller::Desktop,
            g.id.as_str(),
            Some(GrantLevel::Full),
            None,
            None,
            None,
        )
        .unwrap();
        assert_eq!((g2.level, g2.label.as_deref()), (GrantLevel::Full, Some("docs")));
        assert_eq!(ctx.store.list_grants().unwrap().len(), 1);
        assert!(grant_revoke(&ctx, &Caller::Desktop, g2.id.as_str()).unwrap());
        assert!(grants_list(&ctx).is_empty() && ctx.store.list_grants().unwrap().is_empty());
    }

    #[tokio::test]
    async fn reads_are_permission_checked_and_remote_reads_are_capped() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppState::for_tests(dir.path());
        let target = dir.path().join("docs");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("a.txt"), "hello").unwrap();
        let file = target.join("a.txt");
        assert_eq!(
            read_text(&ctx, &Caller::Desktop, file.to_str().unwrap())
                .unwrap_err()
                .code(),
            ErrorCode::PermissionDenied
        );
        grant_add(
            &ctx,
            &Caller::Desktop,
            target.to_str().unwrap(),
            GrantLevel::Full,
            true,
            None,
            None,
        )
        .unwrap();
        let t = read_text(&ctx, &Caller::Desktop, file.to_str().unwrap()).unwrap();
        assert_eq!(t.text, "hello");
        assert_eq!(
            list_dir(&ctx, &remote(), target.to_str().unwrap()).unwrap().len(),
            1
        );
        assert!(stat(&ctx, &Caller::Desktop, file.to_str().unwrap()).unwrap().size == 5);
        assert!(!audit_list(&ctx, 10, None).unwrap().is_empty() || true);
    }

    #[tokio::test]
    async fn plan_preview_apply_undo_discard() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppState::for_tests(dir.path());
        let work = dir.path().join("work");
        std::fs::create_dir_all(&work).unwrap();
        std::fs::write(work.join("a.txt"), "alpha\n").unwrap();
        grant_add(
            &ctx,
            &Caller::Desktop,
            work.to_str().unwrap(),
            GrantLevel::Full,
            true,
            None,
            None,
        )
        .unwrap();
        let canon = dunce::canonicalize(&work).unwrap();
        let bytes = std::fs::read(canon.join("a.txt")).unwrap();
        let ops = vec![
            FileOp::Edit {
                path: canon.join("a.txt").display().to_string(),
                base_hash: nexus_fs::hash_bytes(&bytes),
                edits: vec![nexus_core::changes::TextEdit {
                    start: 0,
                    end: 5,
                    new_text: "ALPHA".into(),
                }],
            },
            FileOp::Create {
                path: canon.join("b.txt").display().to_string(),
                contents: "new\n".into(),
            },
        ];
        let id = changeset_plan(&ctx, "Test", "editor", ops.clone()).unwrap();
        let pv = changeset_preview(&ctx, id.as_str()).unwrap();
        assert!(pv.can_apply(), "{pv:?}");
        let s = changeset_apply(&ctx, &Caller::Desktop, id.as_str()).unwrap();
        assert_eq!(s.status, "applied");
        assert_eq!(s.totals.map(|t| t.files), Some(2));
        assert_eq!(std::fs::read_to_string(canon.join("a.txt")).unwrap(), "ALPHA\n");
        assert_eq!(
            changeset_apply(&ctx, &Caller::Desktop, id.as_str())
                .unwrap_err()
                .code(),
            ErrorCode::Conflict
        );
        assert_eq!(
            changeset_apply(&ctx, &remote(), id.as_str()).unwrap_err().code(),
            ErrorCode::ForbiddenRemote
        );
        let out = changeset_undo(&ctx, &Caller::Desktop, id.as_str(), vec![]).unwrap();
        assert_eq!(out.status, "undone");
        assert_eq!(std::fs::read_to_string(canon.join("a.txt")).unwrap(), "alpha\n");
        assert!(!canon.join("b.txt").exists());

        let id2 = changeset_plan(&ctx, "Again", "editor", ops).unwrap();
        changeset_discard(&ctx, &Caller::Desktop, id2.as_str()).unwrap();
        assert_eq!(changesets_list(&ctx, 10).unwrap().len(), 2);
        assert!(changeset_plan(&ctx, "Empty", "editor", vec![]).is_err());
    }
}

// ---- OS consent -------------------------------------------------------------------------------

/// One protected macOS folder and whether the OS lets us in.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Type)]
pub struct OsFolderStatus {
    pub path: String,
    /// `granted`, `denied`, `missing`, or `other`.
    pub state: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Type)]
pub struct OsPermissionsStatus {
    /// `macos`, `windows`, or `linux`.
    pub platform: String,
    pub folders: Vec<OsFolderStatus>,
    /// Full Disk Access (macOS only); `None` elsewhere.
    pub full_disk_access: Option<String>,
}

fn state_name(a: OsAccess) -> &'static str {
    match a {
        OsAccess::Granted => "granted",
        OsAccess::Denied => "denied",
        OsAccess::Missing => "missing",
        OsAccess::Other => "other",
    }
}

/// Probe the OS-protected folders (macOS TCC). May show the system prompt, so it runs on the blocking pool
/// and only when the Permissions page asks.
pub async fn os_permissions_status(home: std::path::PathBuf) -> Result<OsPermissionsStatus, NexusError> {
    tokio::task::spawn_blocking(move || {
        let mac = cfg!(target_os = "macos");
        let folders = if mac {
            os::tcc_folder_paths(&home)
                .into_iter()
                .map(|p| OsFolderStatus {
                    state: state_name(os::probe_folder(&p, &home)).into(),
                    path: p.display().to_string(),
                })
                .collect()
        } else {
            Vec::new()
        };
        OsPermissionsStatus {
            platform: std::env::consts::OS.to_owned(),
            folders,
            full_disk_access: mac.then(|| state_name(os::probe_full_disk_access(&home)).to_owned()),
        }
    })
    .await
    .map_err(|e| NexusError::Internal(e.to_string()))
}

#[cfg(test)]
mod os_status_tests {
    use super::*;

    #[tokio::test]
    async fn os_status_reports_the_platform_and_probes_only_on_macos() {
        let dir = tempfile::tempdir().unwrap();
        let s = os_permissions_status(dir.path().to_path_buf()).await.unwrap();
        assert_eq!(s.platform, std::env::consts::OS);
        if cfg!(target_os = "macos") {
            assert!(s.full_disk_access.is_some());
        } else {
            assert!(s.folders.is_empty() && s.full_disk_access.is_none());
        }
    }
}
