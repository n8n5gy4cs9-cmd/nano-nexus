//! The journal: the only code that changes files (ADR-005).
//!
//! `Journal::apply` re-checks every operation, copies originals aside, writes atomically, and rolls
//! everything back if any step fails. Layout: `<data>/journal/<id>/manifest.json` plus `files/<n>.orig`
//! (replaced files) and `files/<n>.trashed` (copies of trashed files under 50 MB).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use nexus_core::changes::{ChangePreview, ChangeSet, FileOp};
use nexus_core::fs::Access;
use serde::{Deserialize, Serialize};

use crate::engine::PermissionEngine;
use crate::preview::{ResolvedOp, analyze, hash_bytes};

/// Trashed files at or below this size are also copied into the journal so undo can restore them.
pub const TRASH_COPY_LIMIT: u64 = 50 * 1024 * 1024;

/// Moves a path to the system Trash / Recycle Bin. Deletion is never permanent.
pub trait Trasher: Send + Sync {
    fn trash(&self, path: &Path) -> Result<(), String>;
}

/// The real thing (the `trash` crate: Finder / Recycle Bin / freedesktop Trash).
pub struct SystemTrash;

impl Trasher for SystemTrash {
    fn trash(&self, path: &Path) -> Result<(), String> {
        trash::delete(path).map_err(|e| e.to_string())
    }
}

/// Moves paths into a folder instead of the system Trash. For tests and environments without a Trash.
pub struct MoveToDirTrash {
    dir: PathBuf,
    counter: std::sync::atomic::AtomicUsize,
}

impl MoveToDirTrash {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into(), counter: std::sync::atomic::AtomicUsize::new(0) }
    }
}

impl Trasher for MoveToDirTrash {
    fn trash(&self, path: &Path) -> Result<(), String> {
        std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        let n = self.counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        std::fs::rename(path, self.dir.join(format!("{n}-{name}"))).map_err(|e| e.to_string())
    }
}

/// What happened to one operation; enough to reverse it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum JournalOp {
    /// An existing file was replaced. `orig` is the journal copy of the original.
    Replaced {
        index: usize,
        path: String,
        orig: String,
        orig_hash: String,
        post_hash: String,
    },
    /// A new file was created.
    Created {
        index: usize,
        path: String,
        post_hash: String,
    },
    MadeDir {
        index: usize,
        path: String,
    },
    Moved {
        index: usize,
        from: String,
        to: String,
    },
    /// Moved to the system Trash. `copy` is the journal copy (files under 50 MB).
    Trashed {
        index: usize,
        path: String,
        is_dir: bool,
        size: u64,
        hash: Option<String>,
        copy: Option<String>,
    },
}

impl JournalOp {
    pub fn index(&self) -> usize {
        match self {
            Self::Replaced { index, .. }
            | Self::Created { index, .. }
            | Self::MadeDir { index, .. }
            | Self::Moved { index, .. }
            | Self::Trashed { index, .. } => *index,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub schema: u32,
    pub id: String,
    pub title: String,
    pub origin: String,
    pub created_at: i64,
    /// `applying | applied | rolled_back | undone | failed`
    pub status: String,
    pub ops: Vec<JournalOp>,
    pub error: Option<String>,
}

/// Result of a successful apply.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppliedChangeSet {
    pub id: String,
    pub ops: Vec<JournalOp>,
}

/// Why an apply did not complete.
#[derive(Debug, thiserror::Error)]
pub enum ApplyError {
    /// The re-check found conflicts or missing grants; nothing was written.
    #[error("the change set cannot be applied")]
    Rejected(Box<ChangePreview>),
    /// An operation failed. Earlier operations were rolled back unless `rollback_problems` says otherwise.
    #[error("operation {op_index} failed: {message}")]
    Failed {
        op_index: usize,
        message: String,
        rollback_problems: Vec<String>,
    },
    #[error("journal error: {0}")]
    Journal(String),
}

pub struct Journal {
    dir: PathBuf,
    trasher: Arc<dyn Trasher>,
}

fn ioe(e: std::io::Error) -> String {
    e.to_string()
}

fn sync_dir(dir: &Path) {
    #[cfg(unix)]
    if let Ok(f) = std::fs::File::open(dir) {
        let _ = f.sync_all();
    }
    #[cfg(not(unix))]
    let _ = dir;
}

/// Write `bytes` next to `target`, fsync, copy permissions from `perms_from`, and rename over `target`.
pub fn write_atomic(target: &Path, bytes: &[u8], perms_from: Option<&Path>) -> std::io::Result<()> {
    let dir = target
        .parent()
        .ok_or_else(|| std::io::Error::other("no parent directory"))?;
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp = dir.join(format!(".{name}.nn-tmp-{}", std::process::id()));
    let result = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        if let Some(src) = perms_from
            && let Ok(meta) = std::fs::metadata(src)
        {
            let _ = std::fs::set_permissions(&tmp, meta.permissions());
        }
        std::fs::rename(&tmp, target)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    } else {
        sync_dir(dir);
    }
    result
}

impl Journal {
    pub fn new(dir: impl Into<PathBuf>, trasher: Arc<dyn Trasher>) -> Self {
        Self {
            dir: dir.into(),
            trasher,
        }
    }

    pub fn dir_of(&self, id: &str) -> PathBuf {
        self.dir.join(id)
    }

    fn write_manifest(&self, m: &Manifest) -> Result<(), String> {
        let dir = self.dir_of(&m.id);
        std::fs::create_dir_all(dir.join("files")).map_err(ioe)?;
        let json = serde_json::to_vec_pretty(m).map_err(|e| e.to_string())?;
        write_atomic(&dir.join("manifest.json"), &json, None).map_err(ioe)
    }

    pub fn read_manifest(&self, id: &str) -> Result<Manifest, String> {
        let bytes = std::fs::read(self.dir_of(id).join("manifest.json")).map_err(ioe)?;
        serde_json::from_slice(&bytes).map_err(|e| e.to_string())
    }

    /// Apply a change set.
    ///
    /// 1. Re-check every operation (grants, deny list, base hashes). Any problem aborts with nothing written.
    /// 2. Write the manifest, then perform operations in order, copying originals aside first.
    /// 3. If an operation fails, roll back the ones already done, in reverse.
    pub fn apply(
        &self,
        engine: &PermissionEngine,
        actor: &str,
        cs: &ChangeSet,
    ) -> Result<AppliedChangeSet, ApplyError> {
        let analysis = analyze(engine, actor, cs);
        if !analysis.preview.can_apply() {
            return Err(ApplyError::Rejected(Box::new(analysis.preview)));
        }
        let id = cs.id.as_str().to_owned();
        let mut manifest = Manifest {
            schema: 1,
            id: id.clone(),
            title: cs.title.clone(),
            origin: cs.origin.clone(),
            created_at: cs.created_at,
            status: "applying".into(),
            ops: Vec::new(),
            error: None,
        };
        self.write_manifest(&manifest).map_err(ApplyError::Journal)?;

        for r in &analysis.resolved {
            // TOCTOU: the grant, the deny list, and the file's content are checked again right before
            // the write, not just at preview time.
            let step = self.recheck(engine, actor, r).and_then(|()| self.perform(&id, r));
            match step {
                Ok(done) => {
                    manifest.ops.push(done);
                    self.write_manifest(&manifest).map_err(ApplyError::Journal)?;
                }
                Err(message) => {
                    let problems = self.rollback(&mut manifest);
                    manifest.status = if problems.is_empty() {
                        "rolled_back".into()
                    } else {
                        "failed".into()
                    };
                    manifest.error = Some(message.clone());
                    let _ = self.write_manifest(&manifest);
                    return Err(ApplyError::Failed {
                        op_index: r.index,
                        message,
                        rollback_problems: problems,
                    });
                }
            }
        }
        manifest.status = "applied".into();
        self.write_manifest(&manifest).map_err(ApplyError::Journal)?;
        Ok(AppliedChangeSet {
            id,
            ops: manifest.ops,
        })
    }

    fn recheck(&self, engine: &PermissionEngine, actor: &str, r: &ResolvedOp) -> Result<(), String> {
        let (access, second) = match &r.op {
            FileOp::Edit { .. } => (Access::Write, None),
            FileOp::Write { .. } => (
                if r.old_content.is_some() {
                    Access::Write
                } else {
                    Access::Create
                },
                None,
            ),
            FileOp::Create { .. } | FileOp::Mkdir { .. } => (Access::Create, None),
            FileOp::Move { .. } => (Access::Move, r.to.as_deref()),
            FileOp::Trash { .. } => (Access::Trash, None),
        };
        engine.check(actor, &r.path, access).map_err(|e| e.to_string())?;
        if let Some(to) = second {
            engine.check(actor, to, access).map_err(|e| e.to_string())?;
        }
        // The file must still be what the preview was computed from.
        if let Some(old) = &r.old_content {
            let now = std::fs::read(&r.path).map_err(|e| format!("cannot read {}: {e}", r.path.display()))?;
            if hash_bytes(&now) != hash_bytes(old) {
                return Err(format!("{} changed while applying", r.path.display()));
            }
        }
        Ok(())
    }

    fn perform(&self, id: &str, r: &ResolvedOp) -> Result<JournalOp, String> {
        let files = self.dir_of(id).join("files");
        let path = r.path.to_string_lossy().into_owned();
        match &r.op {
            FileOp::Edit { .. } | FileOp::Write { .. } if r.old_content.is_some() => {
                let (old, new) = (
                    r.old_content.as_deref().unwrap_or_default(),
                    r.new_content.as_deref().unwrap_or_default(),
                );
                let orig_name = format!("{}.orig", r.index);
                std::fs::write(files.join(&orig_name), old)
                    .map_err(|e| format!("cannot back up {path}: {e}"))?;
                write_atomic(&r.path, new, Some(&r.path)).map_err(|e| format!("cannot write {path}: {e}"))?;
                Ok(JournalOp::Replaced {
                    index: r.index,
                    path,
                    orig: format!("files/{orig_name}"),
                    orig_hash: hash_bytes(old),
                    post_hash: hash_bytes(new),
                })
            }
            FileOp::Write { .. } | FileOp::Create { .. } => {
                let new = r.new_content.as_deref().unwrap_or_default();
                let mut f = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&r.path)
                    .map_err(|e| format!("cannot create {path}: {e}"))?;
                let wrote = f.write_all(new).and_then(|()| f.sync_all());
                drop(f);
                if let Err(e) = wrote {
                    let _ = std::fs::remove_file(&r.path);
                    return Err(format!("cannot write {path}: {e}"));
                }
                Ok(JournalOp::Created {
                    index: r.index,
                    path,
                    post_hash: hash_bytes(new),
                })
            }
            FileOp::Edit { .. } => Err(format!("{path}: edit without original content")),
            FileOp::Mkdir { .. } => {
                std::fs::create_dir(&r.path).map_err(|e| format!("cannot create folder {path}: {e}"))?;
                Ok(JournalOp::MadeDir { index: r.index, path })
            }
            FileOp::Move { .. } => {
                let to = r.to.as_ref().ok_or("move without destination")?;
                move_path(&r.path, to, self.trasher.as_ref())?;
                Ok(JournalOp::Moved {
                    index: r.index,
                    from: path,
                    to: to.to_string_lossy().into_owned(),
                })
            }
            FileOp::Trash { .. } => {
                let meta =
                    std::fs::symlink_metadata(&r.path).map_err(|e| format!("cannot read {path}: {e}"))?;
                let is_dir = meta.is_dir();
                let (mut hash, mut copy) = (None, None);
                if !is_dir && meta.len() <= TRASH_COPY_LIMIT {
                    let bytes = std::fs::read(&r.path).map_err(|e| format!("cannot read {path}: {e}"))?;
                    let name = format!("{}.trashed", r.index);
                    std::fs::write(files.join(&name), &bytes)
                        .map_err(|e| format!("cannot copy {path} into the journal: {e}"))?;
                    hash = Some(hash_bytes(&bytes));
                    copy = Some(format!("files/{name}"));
                }
                self.trasher
                    .trash(&r.path)
                    .map_err(|e| format!("cannot move {path} to the Trash: {e}"))?;
                Ok(JournalOp::Trashed {
                    index: r.index,
                    path,
                    is_dir,
                    size: meta.len(),
                    hash,
                    copy,
                })
            }
        }
    }

    /// Reverse `manifest.ops` in reverse order (used when an apply fails midway). Returns problems that
    /// could not be reverted; an empty list means everything is back as it was.
    fn rollback(&self, manifest: &mut Manifest) -> Vec<String> {
        let dir = self.dir_of(&manifest.id);
        let mut problems = Vec::new();
        for op in manifest.ops.iter().rev() {
            if let Err(p) = self.revert(&dir, op, Revert::Rollback) {
                problems.push(p);
            }
        }
        problems
    }

    /// What an undo would run into, without changing anything.
    pub fn undo_conflicts(&self, id: &str) -> Result<Vec<UndoConflict>, String> {
        let m = self.read_manifest(id)?;
        let mut out = Vec::new();
        for op in &m.ops {
            if let Some(message) = conflict_of(op) {
                out.push(UndoConflict {
                    op_index: op.index(),
                    path: op_path(op),
                    message,
                });
            }
        }
        Ok(out)
    }

    /// Undo an applied change set, newest operation first. Every restore is re-checked against the
    /// grants. An operation whose file changed since is a conflict: it is skipped unless `decisions`
    /// says `RestoreAnyway` for its index.
    pub fn undo(
        &self,
        engine: &PermissionEngine,
        actor: &str,
        id: &str,
        decisions: &std::collections::HashMap<usize, ConflictDecision>,
    ) -> Result<UndoReport, String> {
        let mut m = self.read_manifest(id)?;
        if m.status != "applied" {
            return Err(format!("this change set is {}, not applied", m.status));
        }
        let dir = self.dir_of(id);
        let mut report = UndoReport::default();
        for op in m.ops.clone().iter().rev() {
            let idx = op.index();
            let (path, access) = undo_access(op);
            if let Err(e) = engine.check(actor, Path::new(&path), access) {
                report.problems.push(format!("{path}: {e}"));
                continue;
            }
            let force = decisions.get(&idx) == Some(&ConflictDecision::RestoreAnyway);
            let conflict = conflict_of(op);
            if conflict.is_some() && !force && decisions.get(&idx) != Some(&ConflictDecision::KeepCurrent) {
                report.conflicts.push(UndoConflict {
                    op_index: idx,
                    path: path.clone(),
                    message: conflict.unwrap_or_default(),
                });
                continue;
            }
            if conflict.is_some() && decisions.get(&idx) == Some(&ConflictDecision::KeepCurrent) {
                report.skipped.push(idx);
                continue;
            }
            let mode = if force { Revert::UndoForce } else { Revert::Undo };
            match self.revert(&dir, op, mode) {
                Ok(()) => report.restored.push(idx),
                Err(p) => report.problems.push(p),
            }
        }
        if report.conflicts.is_empty() && report.problems.is_empty() {
            m.status = "undone".into();
            self.write_manifest(&m)?;
        }
        Ok(report)
    }

    /// Startup recovery: any change set still `applying` was interrupted (crash, power loss). Roll it back.
    /// Returns the ids that were recovered.
    pub fn recover_interrupted(&self) -> Vec<String> {
        let mut recovered = Vec::new();
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return recovered;
        };
        for e in entries.flatten() {
            let id = e.file_name().to_string_lossy().into_owned();
            let Ok(mut m) = self.read_manifest(&id) else {
                continue;
            };
            if m.status != "applying" {
                continue;
            }
            let problems = self.rollback(&mut m);
            m.status = if problems.is_empty() {
                "rolled_back".into()
            } else {
                "failed".into()
            };
            m.error = Some("interrupted; rolled back on the next start".into());
            let _ = self.write_manifest(&m);
            recovered.push(id);
        }
        recovered
    }

    /// Delete journals older than `max_age_days` or beyond `max_bytes` in total, oldest first.
    /// Journals of change sets that are still `applying` are never pruned. Returns the ids removed.
    pub fn prune(&self, now_ms: i64, max_age_days: u32, max_bytes: u64) -> Vec<String> {
        let mut items: Vec<(i64, String, u64)> = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&self.dir) {
            for e in entries.flatten() {
                let id = e.file_name().to_string_lossy().into_owned();
                let Ok(m) = self.read_manifest(&id) else { continue };
                if m.status == "applying" {
                    continue;
                }
                items.push((m.created_at, id.clone(), dir_size(&self.dir_of(&id))));
            }
        }
        items.sort();
        let cutoff = now_ms - i64::from(max_age_days) * 86_400_000;
        let mut total: u64 = items.iter().map(|i| i.2).sum();
        let mut removed = Vec::new();
        for (created, id, size) in items {
            if created < cutoff || total > max_bytes {
                if std::fs::remove_dir_all(self.dir_of(&id)).is_ok() {
                    total = total.saturating_sub(size);
                    removed.push(id);
                }
            } else {
                break;
            }
        }
        removed
    }

    /// Revert one operation. `Revert::Rollback` is used for our own just-applied work (no conflicts are
    /// expected); `Undo` is used later and handles a changed file by reporting a conflict.
    pub(crate) fn revert(&self, dir: &Path, op: &JournalOp, mode: Revert) -> Result<(), String> {
        match op {
            JournalOp::Replaced {
                path,
                orig,
                orig_hash,
                post_hash,
                ..
            } => {
                let p = Path::new(path);
                let current = std::fs::read(p).map(|b| hash_bytes(&b)).ok();
                if current.as_deref() == Some(orig_hash.as_str()) {
                    return Ok(()); // already original
                }
                if mode == Revert::Undo && current.as_deref() != Some(post_hash.as_str()) {
                    return Err(format!("{path} changed after the change set was applied"));
                }
                let bytes = std::fs::read(dir.join(orig))
                    .map_err(|e| format!("cannot read the saved original of {path}: {e}"))?;
                write_atomic(p, &bytes, Some(p)).map_err(|e| format!("cannot restore {path}: {e}"))
            }
            JournalOp::Created { path, post_hash, .. } => {
                let p = Path::new(path);
                match std::fs::read(p) {
                    Err(_) => Ok(()), // already gone
                    Ok(b) if hash_bytes(&b) == *post_hash || mode == Revert::Rollback => self
                        .trasher
                        .trash(p)
                        .map_err(|e| format!("cannot remove {path}: {e}")),
                    Ok(_) => Err(format!("{path} changed after it was created")),
                }
            }
            JournalOp::MadeDir { path, .. } => {
                let p = Path::new(path);
                if !p.exists() {
                    return Ok(());
                }
                match std::fs::remove_dir(p) {
                    Ok(()) => Ok(()),
                    Err(_) => Err(format!("{path} is not empty, so it was left in place")),
                }
            }
            JournalOp::Moved { from, to, .. } => {
                let (f, t) = (Path::new(from), Path::new(to));
                if !t.exists() {
                    return Ok(());
                }
                if f.exists() {
                    return Err(format!("{from} exists again, so {to} was not moved back"));
                }
                move_path(t, f, self.trasher.as_ref())
            }
            JournalOp::Trashed {
                path,
                copy,
                hash,
                is_dir,
                ..
            } => {
                let p = Path::new(path);
                if p.exists() {
                    match mode {
                        Revert::Undo => {
                            return Err(format!(
                                "something new exists at {path}, so the original was not restored"
                            ));
                        }
                        Revert::UndoForce => {
                            // Never overwrite: the newer file goes to the Trash first.
                            self.trasher
                                .trash(p)
                                .map_err(|e| format!("cannot move the newer {path} to the Trash: {e}"))?;
                        }
                        Revert::Rollback => return Ok(()),
                    }
                }
                if *is_dir || copy.is_none() {
                    return Err(format!("{path} is in the system Trash; restore it from there"));
                }
                let bytes = std::fs::read(dir.join(copy.as_deref().unwrap_or_default()))
                    .map_err(|e| format!("cannot read the saved copy of {path}: {e}"))?;
                if let Some(h) = hash
                    && hash_bytes(&bytes) != *h
                {
                    return Err(format!("the saved copy of {path} is damaged"));
                }
                write_atomic(p, &bytes, None).map_err(|e| format!("cannot restore {path}: {e}"))
            }
        }
    }
}

/// Which situation a revert runs in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Revert {
    /// Our own just-applied work: no conflicts are expected.
    Rollback,
    /// A user undo: a file that changed since is reported as a conflict.
    Undo,
    /// A user undo where the user chose "restore anyway" for this operation.
    UndoForce,
}

/// Rename, falling back to copy + verify + trash the source when the destination is on another volume.
pub fn move_path(from: &Path, to: &Path, trasher: &dyn Trasher) -> Result<(), String> {
    match std::fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(e) if is_cross_device(&e) => {
            let meta = std::fs::symlink_metadata(from).map_err(ioe)?;
            if meta.is_dir() {
                return Err(format!(
                    "cannot move the folder {} to another volume",
                    from.display()
                ));
            }
            let bytes = std::fs::read(from).map_err(ioe)?;
            write_atomic(to, &bytes, Some(from)).map_err(ioe)?;
            let copied = std::fs::read(to).map_err(ioe)?;
            if hash_bytes(&copied) != hash_bytes(&bytes) {
                let _ = std::fs::remove_file(to);
                return Err("the copy on the other volume did not match the original".into());
            }
            trasher.trash(from)
        }
        Err(e) => Err(format!("cannot move {} to {}: {e}", from.display(), to.display())),
    }
}

fn is_cross_device(e: &std::io::Error) -> bool {
    #[cfg(unix)]
    {
        e.raw_os_error() == Some(18) // EXDEV
    }
    #[cfg(windows)]
    {
        e.raw_os_error() == Some(17) // ERROR_NOT_SAME_DEVICE
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = e;
        false
    }
}

/// An operation that cannot be undone cleanly because the file changed since.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UndoConflict {
    pub op_index: usize,
    pub path: String,
    pub message: String,
}

/// The user's answer to an [`UndoConflict`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConflictDecision {
    /// Leave the current file as it is.
    KeepCurrent,
    /// Restore the original over it (a newer file at that path goes to the Trash first).
    RestoreAnyway,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UndoReport {
    pub restored: Vec<usize>,
    pub conflicts: Vec<UndoConflict>,
    pub skipped: Vec<usize>,
    pub problems: Vec<String>,
}

fn op_path(op: &JournalOp) -> String {
    match op {
        JournalOp::Replaced { path, .. }
        | JournalOp::Created { path, .. }
        | JournalOp::MadeDir { path, .. }
        | JournalOp::Trashed { path, .. } => path.clone(),
        JournalOp::Moved { from, .. } => from.clone(),
    }
}

/// Path and access an undo of `op` needs.
fn undo_access(op: &JournalOp) -> (String, Access) {
    match op {
        JournalOp::Replaced { path, .. } => (path.clone(), Access::Write),
        JournalOp::Created { path, .. } | JournalOp::MadeDir { path, .. } => (path.clone(), Access::Trash),
        JournalOp::Moved { from, .. } => (from.clone(), Access::Move),
        JournalOp::Trashed { path, .. } => (path.clone(), Access::Create),
    }
}

/// Describe how the current disk differs from what the apply left, if it does.
fn conflict_of(op: &JournalOp) -> Option<String> {
    match op {
        JournalOp::Replaced {
            path,
            orig_hash,
            post_hash,
            ..
        } => {
            let cur = std::fs::read(path).map(|b| hash_bytes(&b)).ok();
            match cur {
                Some(h) if h == *post_hash || h == *orig_hash => None,
                Some(_) => Some(format!("{path} changed after the change set was applied")),
                None => None,
            }
        }
        JournalOp::Created { path, post_hash, .. } => match std::fs::read(path).map(|b| hash_bytes(&b)) {
            Ok(h) if h != *post_hash => Some(format!("{path} changed after it was created")),
            _ => None,
        },
        JournalOp::MadeDir { path, .. } => {
            let p = Path::new(path);
            let non_empty = p.is_dir()
                && std::fs::read_dir(p)
                    .map(|mut r| r.next().is_some())
                    .unwrap_or(false);
            non_empty.then(|| format!("{path} is not empty now"))
        }
        JournalOp::Moved { from, to, .. } => (Path::new(from).exists() && Path::new(to).exists())
            .then(|| format!("{from} exists again, so {to} cannot be moved back")),
        JournalOp::Trashed { path, .. } => Path::new(path)
            .exists()
            .then(|| format!("something new exists at {path}")),
    }
}

fn dir_size(dir: &Path) -> u64 {
    let mut total = 0;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            match e.metadata() {
                Ok(m) if m.is_dir() => total += dir_size(&e.path()),
                Ok(m) => total += m.len(),
                Err(_) => {}
            }
        }
    }
    total
}
