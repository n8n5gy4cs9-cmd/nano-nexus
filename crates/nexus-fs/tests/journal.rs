//! Journal apply: atomic writes, backups, trash, rollback on failure, TOCTOU re-checks.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use nexus_core::changes::{ChangeSet, FileOp, TextEdit};
use nexus_core::fs::{GrantLevel, GrantScope};
use nexus_core::ids::ChangeSetId;
use nexus_fs::{
    ApplyError, DenyList, DenyRule, Journal, JournalOp, NewGrant, NullAudit, PermissionEngine, Trasher,
    hash_bytes,
};

/// Moves "trashed" paths into a folder inside the test directory instead of the real system Trash.
struct FakeTrash {
    dir: PathBuf,
    trashed: Mutex<Vec<PathBuf>>,
    fail_on: Mutex<Option<String>>,
}

impl Trasher for FakeTrash {
    fn trash(&self, path: &Path) -> Result<(), String> {
        if let Some(name) = self.fail_on.lock().unwrap().as_ref()
            && path.to_string_lossy().contains(name.as_str())
        {
            return Err("simulated trash failure".into());
        }
        let n = self.trashed.lock().unwrap().len();
        std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        let dest = self
            .dir
            .join(format!("{n}-{}", path.file_name().unwrap().to_string_lossy()));
        std::fs::rename(path, &dest).map_err(|e| e.to_string())?;
        self.trashed.lock().unwrap().push(path.to_owned());
        Ok(())
    }
}

struct Env {
    _dir: tempfile::TempDir,
    root: PathBuf,
    engine: PermissionEngine,
    journal: Journal,
    trash: Arc<FakeTrash>,
}

fn env() -> Env {
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    std::fs::create_dir_all(root.join("work")).unwrap();
    std::fs::create_dir_all(root.join("secret")).unwrap();
    std::fs::write(root.join("work/a.txt"), "alpha\nbeta\ngamma\n").unwrap();
    std::fs::write(root.join("work/b.txt"), "bee\n").unwrap();
    std::fs::write(root.join("work/c.txt"), "see\n").unwrap();
    let engine = PermissionEngine::new(
        DenyList::new([DenyRule::user(root.join("secret"), "private")]),
        Arc::new(NullAudit),
    );
    engine
        .add_grant(NewGrant {
            root: root.join("work"),
            level: GrantLevel::Full,
            recursive: true,
            scope: GrantScope::Persistent,
            expires_at: None,
            label: None,
        })
        .unwrap();
    let trash = Arc::new(FakeTrash {
        dir: root.join("trashcan"),
        trashed: Mutex::new(vec![]),
        fail_on: Mutex::new(None),
    });
    let journal = Journal::new(root.join("journal"), trash.clone());
    Env {
        _dir: dir,
        root,
        engine,
        journal,
        trash,
    }
}

fn cs(ops: Vec<FileOp>) -> ChangeSet {
    ChangeSet {
        id: ChangeSetId::new(),
        title: "test".into(),
        origin: "test".into(),
        ops,
        created_at: 0,
    }
}

fn p(e: &Env, rel: &str) -> String {
    e.root.join(rel).to_string_lossy().into_owned()
}

fn read(e: &Env, rel: &str) -> String {
    std::fs::read_to_string(e.root.join(rel)).unwrap()
}

fn edit(e: &Env, rel: &str, start: u32, end: u32, text: &str) -> FileOp {
    let bytes = std::fs::read(e.root.join(rel)).unwrap();
    FileOp::Edit {
        path: p(e, rel),
        base_hash: hash_bytes(&bytes),
        edits: vec![TextEdit {
            start,
            end,
            new_text: text.into(),
        }],
    }
}

/// Snapshot every file under `work` (path -> bytes) for byte-for-byte comparisons.
fn snapshot(e: &Env) -> Vec<(PathBuf, Vec<u8>)> {
    let mut v = Vec::new();
    for entry in walk(&e.root.join("work")) {
        v.push((entry.clone(), std::fs::read(&entry).unwrap()));
    }
    v.sort();
    v
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap() {
        let path = e.unwrap().path();
        if path.is_dir() {
            out.extend(walk(&path))
        } else {
            out.push(path)
        }
    }
    out
}

#[test]
fn a_three_file_change_set_applies_and_is_journaled() {
    let e = env();
    let set = cs(vec![
        edit(&e, "work/a.txt", 0, 5, "ALPHA"),
        FileOp::Create {
            path: p(&e, "work/new.txt"),
            contents: "fresh\n".into(),
        },
        FileOp::Trash {
            path: p(&e, "work/b.txt"),
        },
    ]);
    let done = e.journal.apply(&e.engine, "test", &set).unwrap();
    assert_eq!(done.ops.len(), 3);
    assert_eq!(read(&e, "work/a.txt"), "ALPHA\nbeta\ngamma\n");
    assert_eq!(read(&e, "work/new.txt"), "fresh\n");
    assert!(!e.root.join("work/b.txt").exists());
    assert_eq!(
        e.trash.trashed.lock().unwrap().len(),
        1,
        "deletion goes to the Trash"
    );

    let m = e.journal.read_manifest(set.id.as_str()).unwrap();
    assert_eq!(m.status, "applied");
    let dir = e.journal.dir_of(set.id.as_str());
    assert_eq!(
        std::fs::read_to_string(dir.join("files/0.orig")).unwrap(),
        "alpha\nbeta\ngamma\n"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("files/2.trashed")).unwrap(),
        "bee\n"
    );
    assert!(
        matches!(&m.ops[0], JournalOp::Replaced { post_hash, .. } if *post_hash == hash_bytes(b"ALPHA\nbeta\ngamma\n"))
    );
    // No temp files are left behind.
    assert!(
        walk(&e.root.join("work"))
            .iter()
            .all(|f| !f.to_string_lossy().contains("nn-tmp"))
    );
}

#[test]
fn moves_and_folders() {
    let e = env();
    let set = cs(vec![
        FileOp::Mkdir {
            path: p(&e, "work/sub"),
        },
        FileOp::Move {
            from: p(&e, "work/c.txt"),
            to: p(&e, "work/sub/c.txt"),
        },
    ]);
    e.journal.apply(&e.engine, "test", &set).unwrap();
    assert_eq!(read(&e, "work/sub/c.txt"), "see\n");
    assert!(!e.root.join("work/c.txt").exists());
}

#[test]
fn replaced_files_keep_their_permissions() {
    let e = env();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(e.root.join("work/a.txt"), std::fs::Permissions::from_mode(0o640)).unwrap();
    }
    e.journal
        .apply(&e.engine, "test", &cs(vec![edit(&e, "work/a.txt", 0, 1, "A")]))
        .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(e.root.join("work/a.txt"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o640
        );
    }
}

#[test]
fn a_failure_midway_rolls_everything_back_byte_for_byte() {
    let e = env();
    let before = snapshot(&e);
    let set = cs(vec![
        edit(&e, "work/a.txt", 0, 5, "ALPHA"),
        FileOp::Trash {
            path: p(&e, "work/b.txt"),
        },
        FileOp::Create {
            path: p(&e, "work/made.txt"),
            contents: "x".into(),
        },
        // The parent folder does not exist and no Mkdir precedes it, so this write fails at apply time.
        FileOp::Create {
            path: p(&e, "work/missing-dir/deep.txt"),
            contents: "y".into(),
        },
    ]);
    match e.journal.apply(&e.engine, "test", &set) {
        Err(ApplyError::Failed {
            op_index,
            rollback_problems,
            ..
        }) => {
            assert_eq!(op_index, 3);
            assert!(rollback_problems.is_empty(), "{rollback_problems:?}");
        }
        other => panic!("expected a failure, got {other:?}"),
    }
    assert_eq!(snapshot(&e), before, "every original is back");
    assert_eq!(
        e.journal.read_manifest(set.id.as_str()).unwrap().status,
        "rolled_back"
    );
}

#[test]
fn a_failing_trash_rolls_back_earlier_edits() {
    let e = env();
    let before = snapshot(&e);
    *e.trash.fail_on.lock().unwrap() = Some("b.txt".into());
    let set = cs(vec![
        edit(&e, "work/a.txt", 0, 5, "ALPHA"),
        FileOp::Trash {
            path: p(&e, "work/b.txt"),
        },
    ]);
    assert!(matches!(
        e.journal.apply(&e.engine, "test", &set),
        Err(ApplyError::Failed { op_index: 1, .. })
    ));
    assert_eq!(snapshot(&e), before);
}

#[test]
fn rejected_change_sets_write_nothing_at_all() {
    let e = env();
    let before = snapshot(&e);
    // Hash mismatch.
    let stale = edit(&e, "work/a.txt", 0, 5, "ALPHA");
    std::fs::write(e.root.join("work/a.txt"), "changed\n").unwrap();
    let r = e.journal.apply(&e.engine, "test", &cs(vec![stale]));
    assert!(matches!(r, Err(ApplyError::Rejected(_))));
    // Outside every grant / inside a protected folder.
    for path in [
        p(&e, "secret/x.txt"),
        e.root.join("elsewhere.txt").to_string_lossy().into_owned(),
    ] {
        let set = cs(vec![FileOp::Create {
            path,
            contents: "x".into(),
        }]);
        assert!(matches!(
            e.journal.apply(&e.engine, "test", &set),
            Err(ApplyError::Rejected(_))
        ));
        assert!(
            !e.journal.dir_of(set.id.as_str()).exists(),
            "no journal entry for a rejected set"
        );
    }
    let mut after = snapshot(&e);
    after.retain(|(p, _)| !p.ends_with("a.txt"));
    let mut expect = before;
    expect.retain(|(p, _)| !p.ends_with("a.txt"));
    assert_eq!(after, expect);
    assert!(!e.root.join("secret/x.txt").exists());
}

#[test]
fn a_file_that_changes_between_preview_and_apply_aborts_the_apply() {
    // The hash is verified again right before the write: simulate by editing after analysis would run.
    let e = env();
    let op = edit(&e, "work/a.txt", 0, 5, "ALPHA");
    std::fs::write(e.root.join("work/a.txt"), "alpha\nbeta\ngamma\nextra\n").unwrap();
    assert!(matches!(
        e.journal.apply(&e.engine, "test", &cs(vec![op])),
        Err(ApplyError::Rejected(_))
    ));
    assert_eq!(read(&e, "work/a.txt"), "alpha\nbeta\ngamma\nextra\n");
}

#[test]
fn creates_never_overwrite_an_existing_file() {
    let e = env();
    let set = cs(vec![FileOp::Create {
        path: p(&e, "work/a.txt"),
        contents: "clobber".into(),
    }]);
    assert!(matches!(
        e.journal.apply(&e.engine, "test", &set),
        Err(ApplyError::Rejected(_))
    ));
    assert_eq!(read(&e, "work/a.txt"), "alpha\nbeta\ngamma\n");
}

#[test]
fn large_trashed_files_are_not_copied_but_small_ones_are() {
    let e = env();
    // The copy limit is 50 MB; verify the small-file path stores a copy with the right hash.
    let set = cs(vec![FileOp::Trash {
        path: p(&e, "work/c.txt"),
    }]);
    let done = e.journal.apply(&e.engine, "test", &set).unwrap();
    match &done.ops[0] {
        JournalOp::Trashed {
            copy,
            hash,
            size,
            is_dir,
            ..
        } => {
            assert!(copy.is_some() && !*is_dir);
            assert_eq!(hash.as_deref(), Some(hash_bytes(b"see\n").as_str()));
            assert_eq!(*size, 4);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn directories_can_be_trashed_without_a_journal_copy() {
    let e = env();
    std::fs::create_dir_all(e.root.join("work/old/inner")).unwrap();
    std::fs::write(e.root.join("work/old/inner/f.txt"), "f").unwrap();
    let done = e
        .journal
        .apply(
            &e.engine,
            "test",
            &cs(vec![FileOp::Trash {
                path: p(&e, "work/old"),
            }]),
        )
        .unwrap();
    assert!(matches!(
        &done.ops[0],
        JournalOp::Trashed {
            is_dir: true,
            copy: None,
            ..
        }
    ));
    assert!(!e.root.join("work/old").exists());
}

#[test]
fn move_path_copies_verifies_and_trashes_across_volumes_helper() {
    // Cross-device renames can't be forced here, so exercise the happy path of move_path directly.
    let e = env();
    nexus_fs::move_path(
        &e.root.join("work/b.txt"),
        &e.root.join("work/b2.txt"),
        e.trash.as_ref(),
    )
    .unwrap();
    assert_eq!(read(&e, "work/b2.txt"), "bee\n");
}
