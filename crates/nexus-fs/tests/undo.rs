//! Journal undo, interrupted-apply recovery, pruning, and the byte-identical property tests.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use nexus_core::changes::{ChangeSet, FileOp, TextEdit};
use nexus_core::fs::{GrantLevel, GrantScope};
use nexus_core::ids::ChangeSetId;
use nexus_fs::{
    ApplyError, ConflictDecision, DenyList, Journal, NewGrant, NullAudit, PermissionEngine, Trasher,
    hash_bytes,
};
use proptest::prelude::*;

struct FakeTrash {
    dir: PathBuf,
    count: Mutex<usize>,
}

impl Trasher for FakeTrash {
    fn trash(&self, path: &Path) -> Result<(), String> {
        let n = {
            let mut c = self.count.lock().unwrap();
            *c += 1;
            *c
        };
        std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        std::fs::rename(
            path,
            self.dir
                .join(format!("{n}-{}", path.file_name().unwrap().to_string_lossy())),
        )
        .map_err(|e| e.to_string())
    }
}

struct Env {
    _dir: tempfile::TempDir,
    root: PathBuf,
    engine: PermissionEngine,
    journal: Journal,
}

fn env() -> Env {
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    std::fs::create_dir_all(root.join("work")).unwrap();
    let engine = PermissionEngine::new(DenyList::default(), Arc::new(NullAudit));
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
    let journal = Journal::new(
        root.join("journal"),
        Arc::new(FakeTrash {
            dir: root.join("trashcan"),
            count: Mutex::new(0),
        }),
    );
    Env {
        _dir: dir,
        root,
        engine,
        journal,
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
    e.root.join("work").join(rel).to_string_lossy().into_owned()
}

fn write(e: &Env, rel: &str, text: &str) {
    let path = e.root.join("work").join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn read(e: &Env, rel: &str) -> String {
    std::fs::read_to_string(e.root.join("work").join(rel)).unwrap()
}

/// path (relative to work) -> bytes, plus directories as `None`.
fn tree(e: &Env) -> BTreeMap<String, Option<Vec<u8>>> {
    fn go(base: &Path, dir: &Path, out: &mut BTreeMap<String, Option<Vec<u8>>>) {
        for ent in std::fs::read_dir(dir).unwrap() {
            let path = ent.unwrap().path();
            let rel = path.strip_prefix(base).unwrap().to_string_lossy().into_owned();
            if path.is_dir() {
                out.insert(rel, None);
                go(base, &path, out);
            } else {
                out.insert(rel, Some(std::fs::read(&path).unwrap()));
            }
        }
    }
    let mut out = BTreeMap::new();
    go(&e.root.join("work"), &e.root.join("work"), &mut out);
    out
}

fn edit(e: &Env, rel: &str, start: u32, end: u32, text: &str) -> FileOp {
    let bytes = std::fs::read(e.root.join("work").join(rel)).unwrap();
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

fn no_decisions() -> HashMap<usize, ConflictDecision> {
    HashMap::new()
}

#[test]
fn undo_restores_originals_removes_created_and_moves_back() {
    let e = env();
    write(&e, "a.txt", "alpha\n");
    write(&e, "b.txt", "bee\n");
    write(&e, "c.txt", "see\n");
    let before = tree(&e);
    let set = cs(vec![
        edit(&e, "a.txt", 0, 5, "ALPHA"),
        FileOp::Trash { path: p(&e, "b.txt") },
        FileOp::Mkdir { path: p(&e, "sub") },
        FileOp::Move {
            from: p(&e, "c.txt"),
            to: p(&e, "sub/c.txt"),
        },
        FileOp::Create {
            path: p(&e, "new.txt"),
            contents: "n".into(),
        },
    ]);
    e.journal.apply(&e.engine, "test", &set).unwrap();
    assert_ne!(tree(&e), before);
    let report = e
        .journal
        .undo(&e.engine, "test", set.id.as_str(), &no_decisions())
        .unwrap();
    assert!(
        report.conflicts.is_empty() && report.problems.is_empty(),
        "{report:?}"
    );
    assert_eq!(tree(&e), before, "byte-identical, nothing left over");
    assert_eq!(e.journal.read_manifest(set.id.as_str()).unwrap().status, "undone");
    // Undoing twice is refused.
    assert!(
        e.journal
            .undo(&e.engine, "test", set.id.as_str(), &no_decisions())
            .is_err()
    );
}

#[test]
fn a_file_changed_after_apply_is_a_conflict_with_three_outcomes() {
    let e = env();
    write(&e, "a.txt", "alpha\n");
    write(&e, "z.txt", "zed\n");
    let set = cs(vec![
        edit(&e, "a.txt", 0, 5, "ALPHA"),
        edit(&e, "z.txt", 0, 3, "ZED"),
    ]);
    e.journal.apply(&e.engine, "test", &set).unwrap();
    write(&e, "a.txt", "ALPHA, then I kept typing\n"); // the user edited it afterwards

    let found = e.journal.undo_conflicts(set.id.as_str()).unwrap();
    assert_eq!(found.len(), 1);
    assert!(found[0].path.ends_with("a.txt"));

    // 1. No decision: the conflict is reported, the clean file is restored, status stays applied.
    let r = e
        .journal
        .undo(&e.engine, "test", set.id.as_str(), &no_decisions())
        .unwrap();
    assert_eq!(r.conflicts.len(), 1);
    assert_eq!(r.restored.len(), 1);
    assert_eq!(read(&e, "z.txt"), "zed\n");
    assert_eq!(read(&e, "a.txt"), "ALPHA, then I kept typing\n");
    assert_eq!(
        e.journal.read_manifest(set.id.as_str()).unwrap().status,
        "applied"
    );

    // 2. Keep current: skipped, and the undo completes.
    let keep: HashMap<_, _> = [(0usize, ConflictDecision::KeepCurrent)].into();
    let r = e.journal.undo(&e.engine, "test", set.id.as_str(), &keep).unwrap();
    assert!(r.conflicts.is_empty());
    assert_eq!(r.skipped, vec![0]);
    assert_eq!(read(&e, "a.txt"), "ALPHA, then I kept typing\n");
    assert_eq!(e.journal.read_manifest(set.id.as_str()).unwrap().status, "undone");
}

#[test]
fn restore_anyway_overwrites_the_newer_edit() {
    let e = env();
    write(&e, "a.txt", "alpha\n");
    let set = cs(vec![edit(&e, "a.txt", 0, 5, "ALPHA")]);
    e.journal.apply(&e.engine, "test", &set).unwrap();
    write(&e, "a.txt", "mine\n");
    let force: HashMap<_, _> = [(0usize, ConflictDecision::RestoreAnyway)].into();
    let r = e
        .journal
        .undo(&e.engine, "test", set.id.as_str(), &force)
        .unwrap();
    assert!(r.conflicts.is_empty() && r.problems.is_empty(), "{r:?}");
    assert_eq!(read(&e, "a.txt"), "alpha\n");
}

#[test]
fn a_recreated_file_blocks_restoring_a_trashed_one_unless_forced_and_is_never_overwritten() {
    let e = env();
    write(&e, "b.txt", "original\n");
    let set = cs(vec![FileOp::Trash { path: p(&e, "b.txt") }]);
    e.journal.apply(&e.engine, "test", &set).unwrap();
    write(&e, "b.txt", "someone made a new b\n");
    let r = e
        .journal
        .undo(&e.engine, "test", set.id.as_str(), &no_decisions())
        .unwrap();
    assert_eq!(r.conflicts.len(), 1);
    assert_eq!(read(&e, "b.txt"), "someone made a new b\n");
    let force: HashMap<_, _> = [(0usize, ConflictDecision::RestoreAnyway)].into();
    e.journal
        .undo(&e.engine, "test", set.id.as_str(), &force)
        .unwrap();
    assert_eq!(read(&e, "b.txt"), "original\n");
    // The newer file was moved to the Trash, not destroyed.
    let trashed: Vec<_> = std::fs::read_dir(e.root.join("trashcan"))
        .unwrap()
        .map(|x| x.unwrap().path())
        .collect();
    assert!(trashed.iter().any(|t| {
        std::fs::read_to_string(t)
            .map(|s| s == "someone made a new b\n")
            .unwrap_or(false)
    }));
}

#[test]
fn undo_is_rechecked_against_current_grants() {
    let e = env();
    write(&e, "a.txt", "alpha\n");
    let set = cs(vec![edit(&e, "a.txt", 0, 5, "ALPHA")]);
    e.journal.apply(&e.engine, "test", &set).unwrap();
    for g in e.engine.grants() {
        e.engine.remove_grant(&g.id);
    }
    let r = e
        .journal
        .undo(&e.engine, "test", set.id.as_str(), &no_decisions())
        .unwrap();
    assert!(r.restored.is_empty() && r.problems.len() == 1);
    assert_eq!(
        read(&e, "a.txt"),
        "ALPHA\n",
        "nothing was written without a grant"
    );
}

#[test]
fn an_interrupted_apply_is_rolled_back_on_recovery() {
    let e = env();
    write(&e, "a.txt", "alpha\n");
    let before = tree(&e);
    let set = cs(vec![
        edit(&e, "a.txt", 0, 5, "ALPHA"),
        FileOp::Create {
            path: p(&e, "x.txt"),
            contents: "x".into(),
        },
    ]);
    e.journal.apply(&e.engine, "test", &set).unwrap();
    // Simulate a crash after both ops: flip the manifest back to "applying".
    let mut m = e.journal.read_manifest(set.id.as_str()).unwrap();
    m.status = "applying".into();
    std::fs::write(
        e.journal.dir_of(set.id.as_str()).join("manifest.json"),
        serde_json::to_vec(&m).unwrap(),
    )
    .unwrap();
    let recovered = e.journal.recover_interrupted();
    assert_eq!(recovered, vec![set.id.as_str().to_owned()]);
    assert_eq!(tree(&e), before);
    assert_eq!(
        e.journal.read_manifest(set.id.as_str()).unwrap().status,
        "rolled_back"
    );
    assert!(
        e.journal.recover_interrupted().is_empty(),
        "nothing left to recover"
    );
}

#[test]
fn old_and_oversized_journals_are_pruned_oldest_first() {
    let e = env();
    write(&e, "a.txt", "aaaaaaaaaaaaaaaa\n");
    let mut ids = Vec::new();
    for (n, created) in [(0u32, 1_000i64), (1, 2_000), (2, 3_000)] {
        let mut set = cs(vec![edit(&e, "a.txt", 0, 1, &format!("{n}"))]);
        set.created_at = created;
        e.journal.apply(&e.engine, "test", &set).unwrap();
        ids.push(set.id.as_str().to_owned());
    }
    // Age: everything older than 1 day relative to "now" goes except the newest.
    let day = 86_400_000;
    let removed = e.journal.prune(day + 2_500, 1, u64::MAX);
    assert_eq!(removed, vec![ids[0].clone(), ids[1].clone()]);
    assert!(e.journal.read_manifest(&ids[2]).is_ok());
    // Size: a budget of zero removes the rest.
    let removed = e.journal.prune(3_000, 30, 0);
    assert_eq!(removed, vec![ids[2].clone()]);
}

// ---------------------------------------------------------------------------------------------
// Property tests: random trees + random change sets -> apply -> undo is byte-identical.
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
enum Spec {
    Edit {
        file: usize,
        a: usize,
        b: usize,
        text: String,
    },
    Replace {
        file: usize,
        text: String,
    },
    Trash {
        file: usize,
    },
    Move {
        file: usize,
        name: usize,
    },
    Create {
        name: usize,
        text: String,
    },
    Mkdir {
        name: usize,
    },
}

fn spec() -> impl Strategy<Value = Spec> {
    prop_oneof![
        (0usize..6, 0usize..40, 0usize..40, ".{0,12}").prop_map(|(file, a, b, text)| Spec::Edit {
            file,
            a,
            b,
            text
        }),
        (0usize..6, ".{0,30}").prop_map(|(file, text)| Spec::Replace { file, text }),
        (0usize..6).prop_map(|file| Spec::Trash { file }),
        (0usize..6, 0usize..6).prop_map(|(file, name)| Spec::Move { file, name }),
        (0usize..6, ".{0,20}").prop_map(|(name, text)| Spec::Create { name, text }),
        (0usize..4).prop_map(|name| Spec::Mkdir { name }),
    ]
}

/// Turn specs into a valid change set over `files` (names f0.txt..). Each file/name is used at most once.
fn build(e: &Env, files: &[(String, String)], specs: &[Spec]) -> Vec<FileOp> {
    let mut used_files: HashSet<usize> = HashSet::new();
    let mut used_names: HashSet<String> = HashSet::new();
    let mut ops = Vec::new();
    for s in specs {
        match s {
            Spec::Edit { file, a, b, text } => {
                let i = file % files.len();
                if !used_files.insert(i) {
                    continue;
                }
                let content = &files[i].1;
                let idx: Vec<usize> = content
                    .char_indices()
                    .map(|(b, _)| b)
                    .chain(std::iter::once(content.len()))
                    .collect();
                let (x, y) = (idx[a % idx.len()], idx[b % idx.len()]);
                let (start, end) = (x.min(y) as u32, x.max(y) as u32);
                ops.push(FileOp::Edit {
                    path: p(e, &files[i].0),
                    base_hash: hash_bytes(content.as_bytes()),
                    edits: vec![TextEdit {
                        start,
                        end,
                        new_text: text.clone(),
                    }],
                });
            }
            Spec::Replace { file, text } => {
                let i = file % files.len();
                if !used_files.insert(i) {
                    continue;
                }
                ops.push(FileOp::Write {
                    path: p(e, &files[i].0),
                    base_hash: Some(hash_bytes(files[i].1.as_bytes())),
                    contents: text.clone(),
                });
            }
            Spec::Trash { file } => {
                let i = file % files.len();
                if used_files.insert(i) {
                    ops.push(FileOp::Trash {
                        path: p(e, &files[i].0),
                    });
                }
            }
            Spec::Move { file, name } => {
                let i = file % files.len();
                let to = format!("moved{name}.txt");
                if used_files.contains(&i) || !used_names.insert(to.clone()) {
                    continue;
                }
                used_files.insert(i);
                ops.push(FileOp::Move {
                    from: p(e, &files[i].0),
                    to: p(e, &to),
                });
            }
            Spec::Create { name, text } => {
                let n = format!("new{name}.txt");
                if used_names.insert(n.clone()) {
                    ops.push(FileOp::Create {
                        path: p(e, &n),
                        contents: text.clone(),
                    });
                }
            }
            Spec::Mkdir { name } => {
                let n = format!("dir{name}");
                if used_names.insert(n.clone()) {
                    ops.push(FileOp::Mkdir { path: p(e, &n) });
                }
            }
        }
    }
    ops
}

fn seed(e: &Env, files: &[(String, String)]) {
    for (name, content) in files {
        write(e, name, content);
    }
}

fn files_strategy() -> impl Strategy<Value = Vec<(String, String)>> {
    prop::collection::vec(".{0,40}", 1..6).prop_map(|contents| {
        contents
            .into_iter()
            .enumerate()
            .map(|(i, c)| (format!("f{i}.txt"), c))
            .collect()
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    /// apply then undo leaves every file byte-identical and no strays.
    #[test]
    fn apply_then_undo_is_byte_identical(files in files_strategy(), specs in prop::collection::vec(spec(), 1..10)) {
        let e = env();
        seed(&e, &files);
        let ops = build(&e, &files, &specs);
        prop_assume!(!ops.is_empty());
        let before = tree(&e);
        let set = cs(ops);
        let applied = e.journal.apply(&e.engine, "prop", &set);
        prop_assert!(applied.is_ok(), "apply failed: {:?}", applied.err());
        let report = e.journal.undo(&e.engine, "prop", set.id.as_str(), &no_decisions()).unwrap();
        prop_assert!(report.conflicts.is_empty() && report.problems.is_empty(), "{report:?}");
        prop_assert_eq!(tree(&e), before);
    }

    /// A failure at any point rolls everything back, byte for byte.
    #[test]
    fn a_failing_last_operation_rolls_back_everything(files in files_strategy(), specs in prop::collection::vec(spec(), 1..8)) {
        let e = env();
        seed(&e, &files);
        let mut ops = build(&e, &files, &specs);
        // The parent folder does not exist, so the final create fails at apply time.
        ops.push(FileOp::Create { path: p(&e, "no-such-dir/deep.txt"), contents: "x".into() });
        let before = tree(&e);
        let set = cs(ops);
        let r = e.journal.apply(&e.engine, "prop", &set);
        let failed = matches!(r, Err(ApplyError::Failed { .. }));
        prop_assert!(failed, "{r:?}");
        prop_assert_eq!(tree(&e), before);
    }
}
