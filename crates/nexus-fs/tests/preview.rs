//! Change-set preview: diffs, conflicts, missing grants, validation.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::sync::Arc;

use nexus_core::changes::{ChangeSet, ConflictKind, FileOp, TextEdit};
use nexus_core::fs::{Access, GrantLevel, GrantScope};
use nexus_core::ids::ChangeSetId;
use nexus_fs::{DenyList, DenyRule, NewGrant, NullAudit, PermissionEngine, apply_edits, hash_bytes, preview};

struct Env {
    _dir: tempfile::TempDir,
    root: PathBuf,
    engine: PermissionEngine,
}

fn env(level: Option<GrantLevel>) -> Env {
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    std::fs::create_dir_all(root.join("work")).unwrap();
    std::fs::create_dir_all(root.join("secret")).unwrap();
    std::fs::write(root.join("work/a.txt"), "line one\nline two\nline three\n").unwrap();
    std::fs::write(root.join("work/b.txt"), "bee\n").unwrap();
    let deny = DenyList::new([DenyRule::user(root.join("secret"), "private")]);
    let engine = PermissionEngine::new(deny, Arc::new(NullAudit));
    if let Some(level) = level {
        engine
            .add_grant(NewGrant {
                root: root.clone(),
                level,
                recursive: true,
                scope: GrantScope::Persistent,
                expires_at: None,
                label: None,
            })
            .unwrap();
    }
    Env {
        _dir: dir,
        root,
        engine,
    }
}

fn cs(ops: Vec<FileOp>) -> ChangeSet {
    ChangeSet {
        id: ChangeSetId::new(),
        title: "t".into(),
        origin: "test".into(),
        ops,
        created_at: 0,
    }
}

fn p(e: &Env, rel: &str) -> String {
    e.root.join(rel).to_string_lossy().into_owned()
}

fn edit(e: &Env, rel: &str, edits: Vec<TextEdit>) -> FileOp {
    let bytes = std::fs::read(e.root.join(rel)).unwrap();
    FileOp::Edit {
        path: p(e, rel),
        base_hash: hash_bytes(&bytes),
        edits,
    }
}

fn te(start: u32, end: u32, t: &str) -> TextEdit {
    TextEdit {
        start,
        end,
        new_text: t.into(),
    }
}

#[test]
fn apply_edits_handles_multiple_insertions_and_deletions() {
    assert_eq!(
        apply_edits("hello world", &[te(0, 5, "HELLO"), te(6, 11, "there")]).unwrap(),
        "HELLO there"
    );
    assert_eq!(apply_edits("abc", &[te(3, 3, "d")]).unwrap(), "abcd");
    assert_eq!(apply_edits("abc", &[te(0, 1, "")]).unwrap(), "bc");
    assert_eq!(apply_edits("abc", &[]).unwrap(), "abc");
    assert_eq!(apply_edits("a", &[te(1, 1, "é")]).unwrap(), "aé");
}

#[test]
fn apply_edits_rejects_bad_ranges() {
    assert!(
        apply_edits("abcdef", &[te(1, 4, "x"), te(3, 5, "y")]).is_err(),
        "overlap"
    );
    assert!(
        apply_edits("abcdef", &[te(4, 5, "x"), te(0, 1, "y")]).is_err(),
        "out of order"
    );
    assert!(apply_edits("abc", &[te(2, 9, "x")]).is_err(), "past the end");
    assert!(apply_edits("abc", &[te(2, 1, "x")]).is_err(), "start after end");
    assert!(
        apply_edits("é", &[te(1, 2, "x")]).is_err(),
        "splits a UTF-8 character"
    );
}

#[test]
fn an_edit_previews_with_a_unified_diff_and_totals() {
    let e = env(Some(GrantLevel::Full));
    let pv = preview(
        &e.engine,
        "t",
        &cs(vec![edit(&e, "work/a.txt", vec![te(5, 8, "1"), te(9, 9, "")])]),
    );
    assert!(pv.can_apply(), "{pv:?}");
    let f = &pv.files[0];
    assert_eq!((f.kind.as_str(), f.needs), ("edit", Access::Write));
    let d = f.diff.as_deref().unwrap();
    assert!(d.contains("-line one") && d.contains("+line 1"), "{d}");
    assert_eq!((f.insertions, f.deletions), (1, 1));
    assert_eq!(
        (pv.totals.files, pv.totals.insertions, pv.totals.deletions),
        (1, 1, 1)
    );
}

#[test]
fn a_changed_file_is_a_hash_conflict() {
    let e = env(Some(GrantLevel::Full));
    let op = edit(&e, "work/a.txt", vec![te(0, 4, "LINE")]);
    std::fs::write(e.root.join("work/a.txt"), "someone else edited this\n").unwrap();
    let pv = preview(&e.engine, "t", &cs(vec![op]));
    assert_eq!(pv.conflicts.len(), 1);
    assert_eq!(pv.conflicts[0].kind, ConflictKind::HashMismatch);
    assert!(!pv.can_apply());
}

#[test]
fn invalid_edits_are_reported_per_operation() {
    let e = env(Some(GrantLevel::Full));
    std::fs::write(e.root.join("work/u.txt"), "héllo").unwrap();
    let ops = vec![
        edit(&e, "work/a.txt", vec![te(0, 10, "x"), te(5, 12, "y")]),
        edit(&e, "work/u.txt", vec![te(2, 3, "?")]),
        edit(&e, "work/b.txt", vec![te(0, 999, "z")]),
    ];
    let pv = preview(&e.engine, "t", &cs(ops));
    assert_eq!(pv.conflicts.len(), 3);
    assert!(pv.conflicts.iter().all(|c| c.kind == ConflictKind::Invalid));
    assert_eq!(
        pv.conflicts.iter().map(|c| c.op_index).collect::<Vec<_>>(),
        [0, 1, 2]
    );
}

#[test]
fn binary_files_cannot_be_edited() {
    let e = env(Some(GrantLevel::Full));
    std::fs::write(e.root.join("work/bin.dat"), [0xff, 0xfe, 0x00, 0x01]).unwrap();
    let pv = preview(
        &e.engine,
        "t",
        &cs(vec![edit(&e, "work/bin.dat", vec![te(0, 1, "x")])]),
    );
    assert_eq!(pv.conflicts[0].kind, ConflictKind::Invalid);
}

#[test]
fn missing_grants_are_listed_with_the_access_needed() {
    let e = env(None);
    let ops = vec![
        edit(&e, "work/a.txt", vec![te(0, 1, "L")]),
        FileOp::Create {
            path: p(&e, "work/new.txt"),
            contents: "n".into(),
        },
        FileOp::Move {
            from: p(&e, "work/b.txt"),
            to: p(&e, "work/c.txt"),
        },
        FileOp::Trash {
            path: p(&e, "work/b.txt"),
        },
    ];
    let pv = preview(&e.engine, "t", &cs(ops));
    let needs: Vec<_> = pv.missing_grants.iter().map(|m| (m.op_index, m.needed)).collect();
    assert!(needs.contains(&(0, Access::Write)) && needs.contains(&(1, Access::Create)));
    assert!(needs.contains(&(2, Access::Move)) && needs.contains(&(3, Access::Trash)));
    assert!(!pv.can_apply());
}

#[test]
fn a_read_grant_is_not_enough_to_edit() {
    let e = env(Some(GrantLevel::Read));
    let pv = preview(
        &e.engine,
        "t",
        &cs(vec![edit(&e, "work/a.txt", vec![te(0, 1, "L")])]),
    );
    assert_eq!(pv.missing_grants.len(), 1);
    assert_eq!(pv.missing_grants[0].needed, Access::Write);
    // ReadWrite edits but cannot create.
    let e = env(Some(GrantLevel::ReadWrite));
    let ok = preview(
        &e.engine,
        "t",
        &cs(vec![edit(&e, "work/a.txt", vec![te(0, 1, "L")])]),
    );
    assert!(ok.can_apply());
    let no = preview(
        &e.engine,
        "t",
        &cs(vec![FileOp::Create {
            path: p(&e, "work/n.txt"),
            contents: String::new(),
        }]),
    );
    assert_eq!(no.missing_grants[0].needed, Access::Create);
}

#[test]
fn protected_locations_are_conflicts_not_missing_grants() {
    let e = env(Some(GrantLevel::Full));
    let pv = preview(
        &e.engine,
        "t",
        &cs(vec![FileOp::Create {
            path: p(&e, "secret/key.pem"),
            contents: "x".into(),
        }]),
    );
    assert_eq!(pv.conflicts[0].kind, ConflictKind::Denied);
    assert!(pv.missing_grants.is_empty());
}

#[test]
fn create_mkdir_move_trash_checks() {
    let e = env(Some(GrantLevel::Full));
    let ok = preview(
        &e.engine,
        "t",
        &cs(vec![
            FileOp::Mkdir {
                path: p(&e, "work/sub"),
            },
            FileOp::Create {
                path: p(&e, "work/sub/new.txt"),
                contents: "a\nb\n".into(),
            },
            FileOp::Move {
                from: p(&e, "work/b.txt"),
                to: p(&e, "work/sub/b.txt"),
            },
            FileOp::Trash {
                path: p(&e, "work/a.txt"),
            },
        ]),
    );
    assert!(ok.can_apply(), "{ok:?}");
    assert_eq!((ok.totals.creates, ok.totals.moves, ok.totals.trashes), (2, 1, 1));
    let new_file = ok.files.iter().find(|f| f.kind == "create").unwrap();
    assert_eq!(new_file.insertions, 2);
    let mv = ok.files.iter().find(|f| f.kind == "move").unwrap();
    assert!(mv.to.as_deref().unwrap().ends_with("b.txt"));

    let bad = preview(
        &e.engine,
        "t",
        &cs(vec![
            FileOp::Create {
                path: p(&e, "work/a.txt"),
                contents: "x".into(),
            },
            FileOp::Mkdir { path: p(&e, "work") },
            FileOp::Move {
                from: p(&e, "work/nope.txt"),
                to: p(&e, "work/z.txt"),
            },
            FileOp::Move {
                from: p(&e, "work/a.txt"),
                to: p(&e, "work/b.txt"),
            },
            FileOp::Trash {
                path: p(&e, "work/gone.txt"),
            },
        ]),
    );
    let kinds: Vec<_> = bad.conflicts.iter().map(|c| c.kind).collect();
    assert!(
        kinds.contains(&ConflictKind::Exists) && kinds.contains(&ConflictKind::Missing),
        "{kinds:?}"
    );
    assert!(bad.conflicts.len() >= 5 - 1, "{:?}", bad.conflicts);
}

#[test]
fn write_replaces_or_creates_and_honours_base_hash() {
    let e = env(Some(GrantLevel::Full));
    let h = hash_bytes(&std::fs::read(e.root.join("work/b.txt")).unwrap());
    let ok = preview(
        &e.engine,
        "t",
        &cs(vec![
            FileOp::Write {
                path: p(&e, "work/b.txt"),
                base_hash: Some(h),
                contents: "bee\nbuzz\n".into(),
            },
            FileOp::Write {
                path: p(&e, "work/fresh.txt"),
                base_hash: None,
                contents: "hi\n".into(),
            },
        ]),
    );
    assert!(ok.can_apply(), "{ok:?}");
    assert_eq!(ok.files[0].insertions, 1);
    assert_eq!(ok.files[1].needs, Access::Create);
    let stale = preview(
        &e.engine,
        "t",
        &cs(vec![FileOp::Write {
            path: p(&e, "work/b.txt"),
            base_hash: Some("00".into()),
            contents: "x".into(),
        }]),
    );
    assert_eq!(stale.conflicts[0].kind, ConflictKind::HashMismatch);
    let gone = preview(
        &e.engine,
        "t",
        &cs(vec![FileOp::Write {
            path: p(&e, "work/missing.txt"),
            base_hash: Some("00".into()),
            contents: "x".into(),
        }]),
    );
    assert_eq!(gone.conflicts[0].kind, ConflictKind::Missing);
}

#[test]
fn the_same_path_twice_is_refused() {
    let e = env(Some(GrantLevel::Full));
    let pv = preview(
        &e.engine,
        "t",
        &cs(vec![
            FileOp::Trash {
                path: p(&e, "work/a.txt"),
            },
            edit(&e, "work/a.txt", vec![te(0, 1, "L")]),
        ]),
    );
    assert_eq!(pv.conflicts.len(), 1);
    assert!(pv.conflicts[0].message.contains("also used by operation 1"));
}

#[test]
fn empty_and_relative_inputs_are_invalid() {
    let e = env(Some(GrantLevel::Full));
    assert_eq!(
        preview(&e.engine, "t", &cs(vec![])).conflicts[0].kind,
        ConflictKind::Invalid
    );
    let rel = preview(
        &e.engine,
        "t",
        &cs(vec![FileOp::Create {
            path: "relative.txt".into(),
            contents: String::new(),
        }]),
    );
    assert_eq!(rel.conflicts[0].kind, ConflictKind::Invalid);
}
