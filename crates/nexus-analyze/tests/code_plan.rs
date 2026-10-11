#![allow(clippy::unwrap_used, clippy::expect_used)]
//! The rename planner (T09.10): plan, apply on a copy of a fixture, re-index.

use std::path::{Path, PathBuf};

use nexus_analyze::code::index::{IndexOptions, build_index};
use nexus_analyze::code::plan::{PlanOptions, RenameItem, parse_porcelain, plan_renames};
use nexus_analyze::code::{CodeIndex, SymbolKind};
use nexus_core::changes::FileOp;
use tokio_util::sync::CancellationToken;

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let dest = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &dest);
        } else {
            std::fs::copy(e.path(), dest).unwrap();
        }
    }
}

fn project(name: &str) -> (tempfile::TempDir, PathBuf) {
    let src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name);
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap().join("proj");
    copy_dir(&src, &root);
    (tmp, root)
}

fn index(root: &Path) -> CodeIndex {
    build_index(root, &IndexOptions::default(), None, &CancellationToken::new()).unwrap()
}

fn sym(idx: &CodeIndex, name: &str, kind: SymbolKind) -> nexus_analyze::code::SymbolId {
    idx.symbols
        .iter()
        .find(|s| s.name == name && s.kind == kind)
        .unwrap()
        .id
}

/// A minimal journal: ops in order, edits applied back to front.
fn apply(ops: &[FileOp]) {
    for op in ops {
        match op {
            FileOp::Move { from, to } => std::fs::rename(from, to).unwrap(),
            FileOp::Edit { path, edits, .. } => {
                let mut b = std::fs::read(path).unwrap();
                for e in edits.iter().rev() {
                    b.splice(e.start as usize..e.end as usize, e.new_text.bytes());
                }
                std::fs::write(path, b).unwrap();
            }
            other => panic!("unexpected op {other:?}"),
        }
    }
}

fn item(idx: &CodeIndex, name: &str, kind: SymbolKind, new: &str) -> RenameItem {
    RenameItem {
        symbol: sym(idx, name, kind),
        new_name: new.into(),
    }
}

#[test]
fn renames_a_method_everywhere() {
    let (_t, root) = project("py_app");
    let idx = index(&root);
    let plan = plan_renames(
        &idx,
        &[item(&idx, "doStuff2", SymbolKind::Method, "apply_discount")],
        &PlanOptions::default(),
        0,
    )
    .unwrap();
    assert!(plan.edit_count >= 1, "at least the definition: {plan:?}");
    apply(&plan.change_set.ops);
    let after = index(&root);
    assert!(after.symbols.iter().all(|s| s.name != "doStuff2"));
    assert!(after.symbols.iter().any(|s| s.name == "apply_discount"));
}

#[test]
fn folder_rename_updates_imports() {
    let (_t, root) = project("py_app");
    let idx = index(&root);
    let plan = plan_renames(
        &idx,
        &[item(&idx, "util", SymbolKind::Folder, "helpers")],
        &PlanOptions::default(),
        0,
    )
    .unwrap();
    assert!(
        matches!(plan.change_set.ops[0], FileOp::Move { .. }),
        "moves come first"
    );
    apply(&plan.change_set.ops);
    assert!(root.join("helpers/pricing.py").exists());
    assert!(!root.join("util").exists());
    let main = std::fs::read_to_string(root.join("main.py")).unwrap();
    assert!(main.contains("from helpers.pricing import"), "{main}");
    assert!(main.contains("from helpers import pricing"), "{main}");
    let after = index(&root);
    assert!(
        after
            .symbols
            .iter()
            .all(|s| !(s.kind == SymbolKind::Folder && s.name == "util"))
    );
}

#[test]
fn case_only_file_rename_goes_through_a_temp_name() {
    let (_t, root) = project("py_app");
    let idx = index(&root);
    let plan = plan_renames(
        &idx,
        &[item(&idx, "Helper_Funcs", SymbolKind::File, "helper_funcs")],
        &PlanOptions::default(),
        0,
    )
    .unwrap();
    let moves: Vec<_> = plan
        .change_set
        .ops
        .iter()
        .filter_map(|o| match o {
            FileOp::Move { from, to } => Some((from.clone(), to.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(moves.len(), 2, "{moves:?}");
    assert_eq!(moves[0].1, moves[1].0);
    assert!(moves[0].1.contains(".nexus-rename-tmp"));
    apply(&plan.change_set.ops);
    assert!(root.join("helper_funcs.py").exists());
    let main = std::fs::read_to_string(root.join("main.py")).unwrap();
    assert!(main.contains("from helper_funcs import"), "{main}");
}

#[test]
fn skips_invalid_and_colliding_names() {
    let (_t, root) = project("py_app");
    let idx = index(&root);
    let plan = plan_renames(
        &idx,
        &[
            item(&idx, "doStuff2", SymbolKind::Method, "lambda"),
            item(&idx, "calc_total", SymbolKind::Function, "compute_total"),
        ],
        &PlanOptions {
            public_api: true,
            ..PlanOptions::default()
        },
        0,
    )
    .unwrap();
    assert_eq!(plan.skipped.len(), 1);
    assert!(plan.skipped[0].reason.contains("keyword"));
    assert!(
        plan.edit_count >= 2,
        "calc_total is renamed in its definition and its imports"
    );

    // Public API is left out unless asked for.
    let r = plan_renames(
        &idx,
        &[item(&idx, "calc_total", SymbolKind::Function, "compute_total")],
        &PlanOptions::default(),
        0,
    );
    assert!(matches!(r, Err(nexus_analyze::code::plan::PlanError::Empty)));
}

#[test]
fn stale_file_is_an_error() {
    let (_t, root) = project("py_app");
    let idx = index(&root);
    let p = root.join("util/pricing.py");
    let mut text = std::fs::read_to_string(&p).unwrap();
    text.push_str("\n# changed\n");
    std::fs::write(&p, text).unwrap();
    let r = plan_renames(
        &idx,
        &[item(&idx, "calc_total", SymbolKind::Function, "compute_total")],
        &PlanOptions {
            public_api: true,
            ..PlanOptions::default()
        },
        0,
    );
    assert!(matches!(r, Err(nexus_analyze::code::plan::PlanError::Stale(_))));
}

#[test]
fn porcelain_parsing() {
    let out = " M src/a.py\n?? new.txt\nR  old.py -> \"new name.py\"\n";
    assert_eq!(parse_porcelain(out), vec!["src/a.py", "new.txt", "new name.py"]);
}

#[test]
fn dirty_files_are_reported() {
    let (_t, root) = project("py_app");
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(args)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    };
    if !git(&["init", "-q"]) {
        return; // git is not installed
    }
    git(&["add", "-A"]);
    git(&[
        "-c",
        "user.name=t",
        "-c",
        "user.email=t@t",
        "commit",
        "-qm",
        "init",
    ]);
    let p = root.join("util/pricing.py");
    let mut text = std::fs::read_to_string(&p).unwrap();
    text.push_str("\n# local edit\n");
    std::fs::write(&p, text).unwrap();
    let idx = index(&root);
    let plan = plan_renames(
        &idx,
        &[item(&idx, "calc_total", SymbolKind::Function, "compute_total")],
        &PlanOptions {
            public_api: true,
            ..PlanOptions::default()
        },
        0,
    )
    .unwrap();
    assert_eq!(plan.dirty, vec!["util/pricing.py".to_owned()]);
}
