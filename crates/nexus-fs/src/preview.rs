//! `ChangeSet` preview (PERMISSIONS §5): per-operation access, conflicts, missing grants, and diffs.
//! The same analysis re-runs inside `Journal::apply`, so what the user approved is what is checked.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use nexus_core::changes::{
    ChangePreview, ChangeSet, Conflict, ConflictKind, FileOp, FilePreview, MissingGrant, TextEdit, Totals,
};
use nexus_core::fs::Access;
use similar::{ChangeTag, TextDiff};

use crate::canon::CanonicalPath;
use crate::engine::{FsError, PermissionEngine};

/// Largest file the preview will diff.
pub const MAX_PREVIEW_BYTES: u64 = 20 * 1024 * 1024;
/// Most operations in one change set.
pub const MAX_OPS: usize = 10_000;

/// blake3 hex of `bytes`.
pub fn hash_bytes(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

/// blake3 hex of a file's content.
pub fn hash_file(path: &Path) -> std::io::Result<String> {
    Ok(hash_bytes(&std::fs::read(path)?))
}

/// An operation after its paths were checked and canonicalized.
#[derive(Clone, Debug)]
pub struct ResolvedOp {
    pub index: usize,
    pub op: FileOp,
    /// Canonical primary path (`from` for moves).
    pub path: PathBuf,
    /// Canonical destination of a move.
    pub to: Option<PathBuf>,
    /// The content to write for edit/write/create, computed from the file as it is now.
    pub new_content: Option<Vec<u8>>,
    /// Original bytes of an edited/replaced file (for the journal and the diff).
    pub old_content: Option<Vec<u8>>,
}

/// Everything the analysis found.
#[derive(Debug, Default)]
pub struct Analysis {
    pub preview: ChangePreview,
    pub resolved: Vec<ResolvedOp>,
}

fn utf8(bytes: &[u8]) -> Option<&str> {
    std::str::from_utf8(bytes).ok()
}

/// Apply byte-range `edits` to `text`. They must be sorted, non-overlapping, in range, and on UTF-8
/// character boundaries.
pub fn apply_edits(text: &str, edits: &[TextEdit]) -> Result<String, String> {
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0usize;
    for (i, e) in edits.iter().enumerate() {
        let (s, t) = (e.start as usize, e.end as usize);
        if s > t {
            return Err(format!("edit {i}: start {s} is after end {t}"));
        }
        if t > text.len() {
            return Err(format!(
                "edit {i}: range {s}..{t} is past the end of the file ({} bytes)",
                text.len()
            ));
        }
        if s < cursor {
            return Err(format!(
                "edit {i}: overlaps or is out of order with the previous edit"
            ));
        }
        if !text.is_char_boundary(s) || !text.is_char_boundary(t) {
            return Err(format!("edit {i}: range {s}..{t} splits a UTF-8 character"));
        }
        out.push_str(&text[cursor..s]);
        out.push_str(&e.new_text);
        cursor = t;
    }
    out.push_str(&text[cursor..]);
    Ok(out)
}

struct Diff {
    text: String,
    insertions: u32,
    deletions: u32,
}

fn diff(old: &str, new: &str, path: &str) -> Diff {
    let d = TextDiff::from_lines(old, new);
    let (mut ins, mut del) = (0u32, 0u32);
    for c in d.iter_all_changes() {
        match c.tag() {
            ChangeTag::Insert => ins += 1,
            ChangeTag::Delete => del += 1,
            ChangeTag::Equal => {}
        }
    }
    let text = d
        .unified_diff()
        .context_radius(3)
        .header(&format!("a/{path}"), &format!("b/{path}"))
        .to_string();
    Diff {
        text,
        insertions: ins,
        deletions: del,
    }
}

fn conflict(i: usize, path: &Path, kind: ConflictKind, message: impl Into<String>) -> Conflict {
    Conflict {
        op_index: i as u32,
        path: path.display().to_string(),
        kind,
        message: message.into(),
    }
}

fn exists(p: &Path) -> bool {
    std::fs::symlink_metadata(p).is_ok()
}

/// Check one path, recording a missing grant or a denial instead of failing the whole analysis.
fn gate(
    engine: &PermissionEngine,
    actor: &str,
    i: usize,
    raw: &str,
    access: Access,
    out: &mut Analysis,
) -> Option<CanonicalPath> {
    match engine.check(actor, Path::new(raw), access) {
        Ok(c) => Some(c),
        Err(FsError::NoGrant { path, needed }) => {
            let needed = match needed {
                "write" => Access::Write,
                "create" => Access::Create,
                "move" => Access::Move,
                "trash" => Access::Trash,
                _ => Access::Read,
            };
            out.preview.missing_grants.push(MissingGrant {
                op_index: i as u32,
                path: path.display().to_string(),
                needed,
            });
            None
        }
        Err(FsError::Denied { path, rule }) => {
            out.preview.conflicts.push(conflict(
                i,
                &path,
                ConflictKind::Denied,
                format!("{} is protected ({rule})", path.display()),
            ));
            None
        }
        Err(FsError::BadPath(e)) => {
            out.preview
                .conflicts
                .push(conflict(i, Path::new(raw), ConflictKind::Invalid, e.to_string()));
            None
        }
    }
}

/// Analyze `cs` against the disk as it is now. Never writes anything.
pub fn analyze(engine: &PermissionEngine, actor: &str, cs: &ChangeSet) -> Analysis {
    let mut out = Analysis::default();
    if cs.ops.is_empty() {
        out.preview.conflicts.push(conflict(
            0,
            Path::new(""),
            ConflictKind::Invalid,
            "the change set has no operations",
        ));
        return out;
    }
    if cs.ops.len() > MAX_OPS {
        out.preview.conflicts.push(conflict(
            0,
            Path::new(""),
            ConflictKind::Invalid,
            format!("too many operations ({} > {MAX_OPS})", cs.ops.len()),
        ));
        return out;
    }

    // The same path in two operations is ambiguous: refuse instead of guessing an order of effects.
    let mut seen: HashMap<PathBuf, usize> = HashMap::new();

    for (i, op) in cs.ops.iter().enumerate() {
        match op {
            FileOp::Edit {
                path,
                base_hash,
                edits,
            } => {
                let Some(c) = gate(engine, actor, i, path, Access::Write, &mut out) else {
                    continue;
                };
                let p = c.into_path_buf();
                if dup(&mut seen, &p, i, &mut out) {
                    continue;
                }
                let Some(bytes) = read_for_preview(&p, i, &mut out) else {
                    continue;
                };
                if &hash_bytes(&bytes) != base_hash {
                    out.preview.conflicts.push(conflict(
                        i,
                        &p,
                        ConflictKind::HashMismatch,
                        "the file changed after the plan was made",
                    ));
                    continue;
                }
                let Some(old) = utf8(&bytes) else {
                    out.preview.conflicts.push(conflict(
                        i,
                        &p,
                        ConflictKind::Invalid,
                        "only text files can be edited",
                    ));
                    continue;
                };
                match apply_edits(old, edits) {
                    Ok(new) => {
                        let d = diff(old, &new, &p.display().to_string());
                        push_text(
                            &mut out,
                            i,
                            op,
                            &p,
                            Access::Write,
                            Some(bytes.len() as u64),
                            new.len() as u64,
                            d,
                        );
                        out.resolved.push(ResolvedOp {
                            index: i,
                            op: op.clone(),
                            path: p,
                            to: None,
                            new_content: Some(new.into_bytes()),
                            old_content: Some(bytes),
                        });
                    }
                    Err(msg) => out
                        .preview
                        .conflicts
                        .push(conflict(i, &p, ConflictKind::Invalid, msg)),
                }
            }
            FileOp::Write {
                path,
                base_hash,
                contents,
            } => {
                let target_exists = exists(Path::new(path));
                let access = if target_exists {
                    Access::Write
                } else {
                    Access::Create
                };
                let Some(c) = gate(engine, actor, i, path, access, &mut out) else {
                    continue;
                };
                let p = c.into_path_buf();
                if dup(&mut seen, &p, i, &mut out) {
                    continue;
                }
                match (std::fs::symlink_metadata(&p).is_ok(), base_hash) {
                    (true, Some(h)) => {
                        let Some(bytes) = read_for_preview(&p, i, &mut out) else {
                            continue;
                        };
                        if &hash_bytes(&bytes) != h {
                            out.preview.conflicts.push(conflict(
                                i,
                                &p,
                                ConflictKind::HashMismatch,
                                "the file changed after the plan was made",
                            ));
                            continue;
                        }
                        let d = diff(utf8(&bytes).unwrap_or(""), contents, &p.display().to_string());
                        push_text(
                            &mut out,
                            i,
                            op,
                            &p,
                            access,
                            Some(bytes.len() as u64),
                            contents.len() as u64,
                            d,
                        );
                        out.resolved.push(ResolvedOp {
                            index: i,
                            op: op.clone(),
                            path: p,
                            to: None,
                            new_content: Some(contents.clone().into_bytes()),
                            old_content: Some(bytes),
                        });
                    }
                    (true, None) => {
                        let Some(bytes) = read_for_preview(&p, i, &mut out) else {
                            continue;
                        };
                        let d = diff(utf8(&bytes).unwrap_or(""), contents, &p.display().to_string());
                        push_text(
                            &mut out,
                            i,
                            op,
                            &p,
                            access,
                            Some(bytes.len() as u64),
                            contents.len() as u64,
                            d,
                        );
                        out.resolved.push(ResolvedOp {
                            index: i,
                            op: op.clone(),
                            path: p,
                            to: None,
                            new_content: Some(contents.clone().into_bytes()),
                            old_content: Some(bytes),
                        });
                    }
                    (false, Some(_)) => out.preview.conflicts.push(conflict(
                        i,
                        &p,
                        ConflictKind::Missing,
                        "the file no longer exists",
                    )),
                    (false, None) => {
                        let d = diff("", contents, &p.display().to_string());
                        push_text(
                            &mut out,
                            i,
                            op,
                            &p,
                            Access::Create,
                            None,
                            contents.len() as u64,
                            d,
                        );
                        out.resolved.push(ResolvedOp {
                            index: i,
                            op: op.clone(),
                            path: p,
                            to: None,
                            new_content: Some(contents.clone().into_bytes()),
                            old_content: None,
                        });
                    }
                }
            }
            FileOp::Create { path, contents } => {
                let Some(c) = gate(engine, actor, i, path, Access::Create, &mut out) else {
                    continue;
                };
                let p = c.into_path_buf();
                if dup(&mut seen, &p, i, &mut out) {
                    continue;
                }
                if exists(&p) {
                    out.preview.conflicts.push(conflict(
                        i,
                        &p,
                        ConflictKind::Exists,
                        "the file already exists",
                    ));
                    continue;
                }
                let d = diff("", contents, &p.display().to_string());
                push_text(
                    &mut out,
                    i,
                    op,
                    &p,
                    Access::Create,
                    None,
                    contents.len() as u64,
                    d,
                );
                out.resolved.push(ResolvedOp {
                    index: i,
                    op: op.clone(),
                    path: p,
                    to: None,
                    new_content: Some(contents.clone().into_bytes()),
                    old_content: None,
                });
            }
            FileOp::Mkdir { path } => {
                let Some(c) = gate(engine, actor, i, path, Access::Create, &mut out) else {
                    continue;
                };
                let p = c.into_path_buf();
                if dup(&mut seen, &p, i, &mut out) {
                    continue;
                }
                if exists(&p) {
                    out.preview.conflicts.push(conflict(
                        i,
                        &p,
                        ConflictKind::Exists,
                        "the folder already exists",
                    ));
                    continue;
                }
                simple(&mut out, i, op, &p, None, Access::Create, None, None);
                out.resolved.push(ResolvedOp {
                    index: i,
                    op: op.clone(),
                    path: p,
                    to: None,
                    new_content: None,
                    old_content: None,
                });
            }
            FileOp::Move { from, to } => {
                let a = gate(engine, actor, i, from, Access::Move, &mut out);
                let b = gate(engine, actor, i, to, Access::Move, &mut out);
                let (Some(a), Some(b)) = (a, b) else { continue };
                let (from_p, to_p) = (a.into_path_buf(), b.into_path_buf());
                if dup(&mut seen, &from_p, i, &mut out) || dup(&mut seen, &to_p, i, &mut out) {
                    continue;
                }
                if !exists(&from_p) {
                    out.preview.conflicts.push(conflict(
                        i,
                        &from_p,
                        ConflictKind::Missing,
                        "the source no longer exists",
                    ));
                    continue;
                }
                if exists(&to_p) {
                    out.preview.conflicts.push(conflict(
                        i,
                        &to_p,
                        ConflictKind::Exists,
                        "the destination already exists",
                    ));
                    continue;
                }
                let size = std::fs::metadata(&from_p).map(|m| m.len()).ok();
                simple(&mut out, i, op, &from_p, Some(&to_p), Access::Move, size, size);
                out.resolved.push(ResolvedOp {
                    index: i,
                    op: op.clone(),
                    path: from_p,
                    to: Some(to_p),
                    new_content: None,
                    old_content: None,
                });
            }
            FileOp::Trash { path } => {
                let Some(c) = gate(engine, actor, i, path, Access::Trash, &mut out) else {
                    continue;
                };
                let p = c.into_path_buf();
                if dup(&mut seen, &p, i, &mut out) {
                    continue;
                }
                if !exists(&p) {
                    out.preview.conflicts.push(conflict(
                        i,
                        &p,
                        ConflictKind::Missing,
                        "the file no longer exists",
                    ));
                    continue;
                }
                let size = std::fs::metadata(&p).map(|m| m.len()).ok();
                simple(&mut out, i, op, &p, None, Access::Trash, size, None);
                out.resolved.push(ResolvedOp {
                    index: i,
                    op: op.clone(),
                    path: p,
                    to: None,
                    new_content: None,
                    old_content: None,
                });
            }
        }
    }
    totals(&mut out.preview);
    out
}

/// Preview only (what the UI asks for).
pub fn preview(engine: &PermissionEngine, actor: &str, cs: &ChangeSet) -> ChangePreview {
    analyze(engine, actor, cs).preview
}

fn dup(seen: &mut HashMap<PathBuf, usize>, p: &Path, i: usize, out: &mut Analysis) -> bool {
    if let Some(prev) = seen.insert(p.to_owned(), i) {
        out.preview.conflicts.push(conflict(
            i,
            p,
            ConflictKind::Invalid,
            format!("this path is also used by operation {}", prev + 1),
        ));
        return true;
    }
    false
}

fn read_for_preview(p: &Path, i: usize, out: &mut Analysis) -> Option<Vec<u8>> {
    let meta = match std::fs::metadata(p) {
        Ok(m) => m,
        Err(_) => {
            out.preview
                .conflicts
                .push(conflict(i, p, ConflictKind::Missing, "the file no longer exists"));
            return None;
        }
    };
    if !meta.is_file() {
        out.preview.conflicts.push(conflict(
            i,
            p,
            ConflictKind::Invalid,
            "this is not a regular file",
        ));
        return None;
    }
    if meta.len() > MAX_PREVIEW_BYTES {
        out.preview.conflicts.push(conflict(
            i,
            p,
            ConflictKind::Invalid,
            "the file is too large to preview and change",
        ));
        return None;
    }
    match std::fs::read(p) {
        Ok(b) => Some(b),
        Err(e) => {
            out.preview.conflicts.push(conflict(
                i,
                p,
                ConflictKind::Invalid,
                format!("cannot read the file: {e}"),
            ));
            None
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn push_text(
    out: &mut Analysis,
    i: usize,
    op: &FileOp,
    p: &Path,
    needs: Access,
    before: Option<u64>,
    after: u64,
    d: Diff,
) {
    out.preview.files.push(FilePreview {
        op_index: i as u32,
        kind: op.kind().to_owned(),
        path: p.display().to_string(),
        to: None,
        needs,
        size_before: before,
        size_after: Some(after),
        insertions: d.insertions,
        deletions: d.deletions,
        diff: Some(d.text),
        binary: false,
    });
}

#[allow(clippy::too_many_arguments)]
fn simple(
    out: &mut Analysis,
    i: usize,
    op: &FileOp,
    p: &Path,
    to: Option<&Path>,
    needs: Access,
    before: Option<u64>,
    after: Option<u64>,
) {
    out.preview.files.push(FilePreview {
        op_index: i as u32,
        kind: op.kind().to_owned(),
        path: p.display().to_string(),
        to: to.map(|t| t.display().to_string()),
        needs,
        size_before: before,
        size_after: after,
        insertions: 0,
        deletions: 0,
        diff: None,
        binary: false,
    });
}

fn totals(p: &mut ChangePreview) {
    let mut t = Totals::default();
    for f in &p.files {
        t.files += 1;
        t.insertions += f.insertions;
        t.deletions += f.deletions;
        match f.kind.as_str() {
            "create" | "mkdir" => t.creates += 1,
            "move" => t.moves += 1,
            "trash" => t.trashes += 1,
            _ => {}
        }
        t.bytes += f.size_after.or(f.size_before).unwrap_or(0);
    }
    p.totals = t;
}
