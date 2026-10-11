//! Rename planner (M09, T09.10): accepted renames → one [`ChangeSet`] of byte-range edits plus
//! file and folder moves. Moves come first (files, then folders deepest-first, all from their old
//! paths); edits then target the final paths. Case-only renames go through a temporary name.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use nexus_core::changes::{ChangeSet, FileOp, TextEdit};
use nexus_core::ids::ChangeSetId;
use serde::{Deserialize, Serialize};

use super::index::{CodeIndex, Flags, RefKind, SymbolId};
use super::model::SymbolKind;
use super::propose::Validator;

/// One accepted rename.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct RenameItem {
    pub symbol: SymbolId,
    pub new_name: String,
}

/// What the plan includes beyond certain code references. Everything is off by default.
#[derive(Clone, Debug, Default)]
pub struct PlanOptions {
    /// Rename occurrences in documentation files.
    pub docs: bool,
    /// Rename occurrences in comments.
    pub comments: bool,
    /// Include references that could also mean another symbol.
    pub ambiguous: bool,
    /// Rename symbols that look like public API.
    pub public_api: bool,
}

/// An item (or a single occurrence) the planner left out, and why.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct Skipped {
    pub symbol: Option<SymbolId>,
    pub reason: String,
}

/// The result of planning.
#[derive(Clone, Debug)]
pub struct RenamePlan {
    pub change_set: ChangeSet,
    pub skipped: Vec<Skipped>,
    /// Project-relative files the plan touches that have uncommitted git changes.
    pub dirty: Vec<String>,
    pub edit_count: u32,
}

#[derive(Debug, thiserror::Error)]
pub enum PlanError {
    #[error("`{0}` changed since the project was indexed; re-scan first")]
    Stale(String),
    #[error("could not read `{0}`: {1}")]
    Read(String, String),
    #[error("nothing to rename")]
    Empty,
}

fn lower(s: &str) -> String {
    s.to_lowercase()
}

fn ext_of(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.').map(|(_, e)| e).unwrap_or("")
}

fn join(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_owned()
    } else {
        format!("{parent}/{name}")
    }
}

fn abs(root: &str, rel: &str) -> String {
    let mut p = PathBuf::from(root);
    for part in rel.split('/').filter(|s| !s.is_empty()) {
        p.push(part);
    }
    p.to_string_lossy().into_owned()
}

/// Plan the accepted renames. `now_ms` stamps the change set.
pub fn plan_renames(
    index: &CodeIndex,
    items: &[RenameItem],
    opts: &PlanOptions,
    now_ms: i64,
) -> Result<RenamePlan, PlanError> {
    let validator = Validator::new(index);
    let root = Path::new(&index.root);
    let mut skipped: Vec<Skipped> = Vec::new();
    let mut taken: HashMap<SymbolId, String> = HashMap::new();
    let mut accepted: Vec<&RenameItem> = Vec::new();

    for it in items {
        let Some(s) = index.symbol(it.symbol) else {
            skipped.push(Skipped {
                symbol: None,
                reason: "unknown symbol".into(),
            });
            continue;
        };
        if taken.contains_key(&it.symbol) {
            skipped.push(Skipped {
                symbol: Some(it.symbol),
                reason: "renamed twice".into(),
            });
            continue;
        }
        if s.flags.has(Flags::PUBLIC_API) && !opts.public_api {
            skipped.push(Skipped {
                symbol: Some(it.symbol),
                reason: "looks like public API (not included)".into(),
            });
            continue;
        }
        if s.flags.has(Flags::RESERVED) {
            skipped.push(Skipped {
                symbol: Some(it.symbol),
                reason: "reserved name".into(),
            });
            continue;
        }
        if let Err(e) = validator.check(it.symbol, &it.new_name, &taken) {
            skipped.push(Skipped {
                symbol: Some(it.symbol),
                reason: e,
            });
            continue;
        }
        taken.insert(it.symbol, it.new_name.clone());
        accepted.push(it);
    }

    // Path renames.
    let mut file_new: HashMap<u32, String> = HashMap::new();
    let mut folder_new: HashMap<String, String> = HashMap::new();
    for it in &accepted {
        let Some(s) = index.symbol(it.symbol) else {
            continue;
        };
        match s.kind {
            SymbolKind::File => {
                if let Some(fi) = index.files.iter().position(|f| f.path == s.signature) {
                    file_new.insert(fi as u32, it.new_name.clone());
                }
            }
            SymbolKind::Folder => {
                folder_new.insert(s.signature.clone(), it.new_name.clone());
            }
            _ => {}
        }
    }

    // Targets that already exist on disk block the rename (a case-only change of itself is fine).
    let mut blocked_files: HashSet<u32> = HashSet::new();
    for (fi, stem) in file_new.clone() {
        let Some(f) = index.file(fi) else { continue };
        let (dir, _) = f.path.rsplit_once('/').unwrap_or(("", &f.path));
        let ext = ext_of(&f.path);
        let to = join(
            dir,
            &if ext.is_empty() {
                stem
            } else {
                format!("{stem}.{ext}")
            },
        );
        if lower(&to) != lower(&f.path) && root.join(&to).exists() {
            skipped.push(Skipped {
                symbol: None,
                reason: format!("`{to}` already exists; `{}` was not renamed", f.path),
            });
            blocked_files.insert(fi);
            file_new.remove(&fi);
        }
    }
    for (d, name) in folder_new.clone() {
        let (parent, _) = d.rsplit_once('/').unwrap_or(("", &d));
        let to = join(parent, &name);
        if lower(&to) != lower(&d) && root.join(&to).exists() {
            skipped.push(Skipped {
                symbol: None,
                reason: format!("`{to}` already exists; folder `{d}` was not renamed"),
            });
            folder_new.remove(&d);
        }
    }

    // Final location of a directory / file after all moves.
    let final_dir = |dir: &str| -> String {
        let mut out = String::new();
        let mut old = String::new();
        for comp in dir.split('/').filter(|c| !c.is_empty()) {
            old = join(&old, comp);
            let name = folder_new.get(&old).map(String::as_str).unwrap_or(comp);
            out = join(&out, name);
        }
        out
    };
    let final_file = |fi: u32, rel: &str| -> String {
        let (dir, name) = rel.rsplit_once('/').unwrap_or(("", rel));
        let name = match file_new.get(&fi) {
            Some(stem) => {
                let ext = ext_of(rel);
                if ext.is_empty() {
                    stem.clone()
                } else {
                    format!("{stem}.{ext}")
                }
            }
            None => name.to_owned(),
        };
        join(&final_dir(dir), &name)
    };

    let mut ops: Vec<FileOp> = Vec::new();
    let mut tmp_n = 0u32;
    let mut tmp_name = |parent: &str| {
        tmp_n += 1;
        join(parent, &format!(".nexus-rename-tmp-{tmp_n}"))
    };
    let mut push_move = |ops: &mut Vec<FileOp>, from_rel: &str, to_rel: &str, parent: &str| {
        if lower(from_rel) == lower(to_rel) {
            let tmp = tmp_name(parent);
            ops.push(FileOp::Move {
                from: abs(&index.root, from_rel),
                to: abs(&index.root, &tmp),
            });
            ops.push(FileOp::Move {
                from: abs(&index.root, &tmp),
                to: abs(&index.root, to_rel),
            });
        } else {
            ops.push(FileOp::Move {
                from: abs(&index.root, from_rel),
                to: abs(&index.root, to_rel),
            });
        }
    };

    let mut touched: Vec<String> = Vec::new();
    // Files first, in place (their directories still have the old names).
    let mut file_ids: Vec<u32> = file_new.keys().copied().collect();
    file_ids.sort_unstable();
    for fi in file_ids {
        let Some(f) = index.file(fi) else { continue };
        let (dir, _) = f.path.rsplit_once('/').unwrap_or(("", &f.path));
        let stem = &file_new[&fi];
        let ext = ext_of(&f.path);
        let to = join(
            dir,
            &if ext.is_empty() {
                stem.clone()
            } else {
                format!("{stem}.{ext}")
            },
        );
        touched.push(f.path.clone());
        push_move(&mut ops, &f.path, &to, dir);
    }
    // Folders deepest first, always from their old path.
    let mut dirs: Vec<&String> = folder_new.keys().collect();
    dirs.sort_by(|a, b| {
        b.matches('/')
            .count()
            .cmp(&a.matches('/').count())
            .then_with(|| a.cmp(b))
    });
    for d in dirs {
        let (parent, _) = d.rsplit_once('/').unwrap_or(("", d));
        let to = join(parent, &folder_new[d]);
        touched.push(d.clone());
        push_move(&mut ops, d, &to, parent);
    }
    // Edits.
    let mut by_file: BTreeMap<u32, Vec<(u32, u32, String, SymbolId)>> = BTreeMap::new();
    let mut edit_skips: Vec<Skipped> = Vec::new();
    for it in &accepted {
        let Some(s) = index.symbol(it.symbol) else {
            continue;
        };
        if s.kind == SymbolKind::File
            && let Some(fi) = index.files.iter().position(|f| f.path == s.signature)
            && blocked_files.contains(&(fi as u32))
        {
            continue;
        }
        for r in &s.refs {
            let wanted = match r.kind {
                RefKind::Code | RefKind::Import | RefKind::StringLiteral | RefKind::ConfigFile => true,
                RefKind::Doc => opts.docs,
                RefKind::Comment => opts.comments,
            };
            if !wanted || (r.ambiguous && !r.is_def && !opts.ambiguous) {
                continue;
            }
            by_file.entry(r.loc.file).or_default().push((
                r.loc.start,
                r.loc.end,
                it.new_name.clone(),
                it.symbol,
            ));
        }
    }

    let mut edit_count = 0u32;
    let mut edit_ops: Vec<FileOp> = Vec::new();
    for (file, mut list) in by_file {
        let Some(rel) = index.path_of(file) else { continue };
        let rel = rel.to_owned();
        let bytes =
            std::fs::read(root.join(&rel)).map_err(|e| PlanError::Read(rel.clone(), e.to_string()))?;
        if let Some(f) = index.file(file)
            && blake3::hash(&bytes).to_hex().to_string() != f.hash
        {
            return Err(PlanError::Stale(rel));
        }
        list.sort_by_key(|e| (e.0, e.1));
        list.dedup();
        let mut edits: Vec<TextEdit> = Vec::new();
        let mut last_end = 0u32;
        for (start, end, new_text, sym) in list {
            let (a, b) = (start as usize, end as usize);
            let old = index.symbol(sym).map(|s| s.name.as_str()).unwrap_or("");
            if b > bytes.len() || a >= b || bytes[a..b] != *old.as_bytes() {
                edit_skips.push(Skipped {
                    symbol: Some(sym),
                    reason: format!("the text at {rel}:{start} is not `{old}`; left alone"),
                });
                continue;
            }
            if start < last_end {
                edit_skips.push(Skipped {
                    symbol: Some(sym),
                    reason: format!("overlapping edit at {rel}:{start}; left alone"),
                });
                continue;
            }
            last_end = end;
            edits.push(TextEdit { start, end, new_text });
        }
        if edits.is_empty() {
            continue;
        }
        edit_count += edits.len() as u32;
        touched.push(rel.clone());
        let fi_final = if (file as usize) < index.files.len() {
            final_file(file, &rel)
        } else {
            // Config and doc files live in directories that may be renamed.
            let (dir, name) = rel.rsplit_once('/').unwrap_or(("", &rel));
            join(&final_dir(dir), name)
        };
        edit_ops.push(FileOp::Edit {
            path: abs(&index.root, &fi_final),
            base_hash: blake3::hash(&bytes).to_hex().to_string(),
            edits,
        });
    }
    skipped.extend(edit_skips);
    ops.extend(edit_ops);

    // Two files must not end up at the same place.
    let mut seen: HashMap<String, String> = HashMap::new();
    for (fi, f) in index.files.iter().enumerate() {
        let fin = final_file(fi as u32, &f.path);
        if fin != f.path
            && let Some(prev) = seen.insert(lower(&fin), f.path.clone())
        {
            skipped.push(Skipped {
                symbol: None,
                reason: format!("`{prev}` and `{}` would both become `{fin}`", f.path),
            });
        }
    }

    if ops.is_empty() {
        return Err(PlanError::Empty);
    }
    let n = accepted.len();
    let dirty = git_dirty(root)
        .map(|d| {
            let mut v: Vec<String> = touched
                .iter()
                .filter(|t| d.iter().any(|x| x == *t || x.starts_with(&format!("{t}/"))))
                .cloned()
                .collect();
            v.sort();
            v.dedup();
            v
        })
        .unwrap_or_default();
    Ok(RenamePlan {
        change_set: ChangeSet {
            id: ChangeSetId::new(),
            title: if n == 1 {
                "Rename 1 symbol".into()
            } else {
                format!("Rename {n} symbols")
            },
            origin: "rename".into(),
            ops,
            created_at: now_ms,
        },
        skipped,
        dirty,
        edit_count,
    })
}

/// Parse `git status --porcelain` output into repository-relative paths (renames give the new path).
pub fn parse_porcelain(out: &str) -> Vec<String> {
    out.lines()
        .filter(|l| l.len() > 3)
        .map(|l| {
            let p = &l[3..];
            let p = p.rsplit_once(" -> ").map(|(_, b)| b).unwrap_or(p);
            p.trim_matches('"').to_owned()
        })
        .collect()
}

/// Project-relative paths with uncommitted changes, or `None` when `root` is not in a git repository
/// (or git is not installed). Fixed argument list, no shell.
pub fn git_dirty(root: &Path) -> Option<Vec<String>> {
    let run = |args: &[&str]| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
    };
    let prefix = run(&["rev-parse", "--show-prefix"])?.trim().to_owned();
    let status = run(&["status", "--porcelain", "--untracked-files=all"])?;
    Some(
        parse_porcelain(&status)
            .into_iter()
            .filter_map(|p| p.strip_prefix(&prefix).map(str::to_owned))
            .collect(),
    )
}
