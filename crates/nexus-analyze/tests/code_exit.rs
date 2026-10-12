#![allow(clippy::unwrap_used, clippy::expect_used)]
//! M09 exit check (T09.19): apply every deterministic suggestion to a copy of each fixture, then run
//! the project's own checker. A fixture whose tool is not installed is skipped.

use std::path::{Path, PathBuf};
use std::process::Command;

use nexus_analyze::code::index::{IndexOptions, build_index};
use nexus_analyze::code::plan::{PlanOptions, RenameItem, plan_renames};
use nexus_analyze::code::rules::{RuleOptions, detect};
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

fn have(tool: &str) -> bool {
    Command::new(tool).arg("--version").output().is_ok()
}

fn run(dir: &Path, cmd: &str, args: &[&str]) -> (bool, String) {
    let out = Command::new(cmd)
        .args(args)
        .current_dir(dir)
        .env("CARGO_TARGET_DIR", dir.join("..").join("target-x"))
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.success(), text)
}

/// Copy the fixture, apply every suggested rename, return the project root.
fn renamed(name: &str) -> Option<(tempfile::TempDir, PathBuf, usize)> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name);
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap().join("proj");
    copy_dir(&src, &root);
    let idx = build_index(&root, &IndexOptions::default(), None, &CancellationToken::new()).unwrap();
    let items: Vec<RenameItem> = detect(&idx, &RuleOptions::default())
        .into_iter()
        .filter_map(|c| {
            c.suggested.map(|new_name| RenameItem {
                symbol: c.symbol,
                new_name,
            })
        })
        .collect();
    for i in &items {
        let sym = idx.symbols.iter().find(|s| s.id == i.symbol).unwrap();
        eprintln!("{name}: {:?} {} -> {}", sym.kind, sym.name, i.new_name);
    }
    let opts = PlanOptions {
        public_api: true,
        ..PlanOptions::default()
    };
    let Ok(plan) = plan_renames(&idx, &items, &opts, 0) else {
        eprintln!("{name}: nothing to rename");
        return None;
    };
    for sk in &plan.skipped {
        eprintln!("{name}: skipped {sk:?}");
    }
    for op in &plan.change_set.ops {
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
    Some((tmp, root, items.len()))
}

fn check(name: &str, tool: &str, steps: &[(&str, &[&str])]) {
    if !have(tool) {
        eprintln!("skipping {name}: `{tool}` is not installed");
        return;
    }
    let Some((_tmp, root, n)) = renamed(name) else {
        return;
    };
    for (cmd, args) in steps {
        let (ok, out) = run(&root, cmd, args);
        assert!(
            ok,
            "{name}: `{cmd} {}` failed after {n} renames:\n{out}",
            args.join(" ")
        );
    }
}

#[test]
fn python_still_runs() {
    check("py_app", "python3", &[("python3", &["main.py"])]);
}

#[test]
fn go_still_builds() {
    check("go_app", "go", &[("go", &["build", "./..."])]);
}

#[test]
fn java_still_compiles() {
    check(
        "java_app",
        "javac",
        &[(
            "sh",
            &[
                "-c",
                "mkdir -p ../out && javac -d ../out $(find src -name '*.java')",
            ],
        )],
    );
}

#[test]
fn rust_still_checks() {
    check("rust_app", "cargo", &[("cargo", &["check", "--offline", "-q"])]);
}

#[test]
fn typescript_still_type_checks() {
    check(
        "ts_app",
        "tsc",
        &[(
            "tsc",
            &[
                "--noEmit",
                "-p",
                ".",
                "--ignoreDeprecations",
                "6.0",
                "--jsx",
                "preserve",
            ],
        )],
    );
}

#[test]
fn ruby_still_runs() {
    check("rb_app", "ruby", &[("ruby", &["main.rb"])]);
}

#[test]
fn php_still_runs() {
    check("php_app", "php", &[("php", &["main.php"])]);
}

#[test]
fn c_still_compiles() {
    check(
        "c_app",
        "gcc",
        &[("gcc", &["-fsyntax-only", "main.c", "util/pricing.c"])],
    );
}

#[test]
fn cpp_still_compiles() {
    check(
        "cpp_app",
        "g++",
        &[("g++", &["-fsyntax-only", "main.cpp", "util/pricing.cpp"])],
    );
}

#[test]
fn lua_still_runs() {
    check("lua_app", "lua", &[("lua", &["main.lua"])]);
}
