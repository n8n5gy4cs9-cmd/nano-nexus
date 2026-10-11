//! Verify (M09, T09.12, ANALYZER §5.7): run the project's own checker after a rename. Never automatic,
//! never through a shell: the program and its argument list are fixed per project type.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

/// A checker the project supports.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct Checker {
    pub id: String,
    /// What the button says: "cargo check".
    pub label: String,
    pub program: String,
    pub args: Vec<String>,
    /// Directory to run in, relative to the project root (`""` = the root).
    pub dir: String,
    /// The program was found on PATH.
    pub available: bool,
}

/// How a run ended.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct VerifyResult {
    pub ok: bool,
    pub exit_code: Option<i32>,
    pub cancelled: bool,
    pub timed_out: bool,
    /// The program could not be started (not installed).
    pub missing: bool,
    /// The first lines that look like errors.
    pub first_errors: Vec<String>,
}

const MAX_ERRORS: usize = 10;
const TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// Is `program` an executable on PATH?
pub fn on_path(program: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    let exts: &[&str] = if cfg!(windows) {
        &["", ".exe", ".cmd", ".bat"]
    } else {
        &[""]
    };
    std::env::split_paths(&path).any(|dir| exts.iter().any(|e| dir.join(format!("{program}{e}")).is_file()))
}

fn mk(id: &str, label: &str, program: &str, args: &[&str], dir: &str) -> Checker {
    Checker {
        id: id.into(),
        label: label.into(),
        program: program.into(),
        args: args.iter().map(|s| (*s).to_owned()).collect(),
        dir: dir.into(),
        available: on_path(program),
    }
}

fn has_ext(dir: &Path, ext: &str) -> bool {
    std::fs::read_dir(dir).is_ok_and(|rd| {
        rd.flatten()
            .any(|e| e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case(ext)))
    })
}

/// Checkers for the project at `root`, from its manifests (root directory only).
pub fn detect(root: &Path) -> Vec<Checker> {
    let has = |n: &str| root.join(n).is_file();
    let mut out = Vec::new();
    if has("Cargo.toml") {
        out.push(mk("cargo", "cargo check", "cargo", &["check"], ""));
    }
    if has("tsconfig.json") {
        out.push(mk("tsc", "tsc --noEmit", "tsc", &["--noEmit"], ""));
    }
    if has("pyproject.toml") || has("setup.py") || has("requirements.txt") || has_ext(root, "py") {
        let py = if cfg!(windows) { "python" } else { "python3" };
        out.push(mk(
            "python",
            "python -m compileall",
            py,
            &["-m", "compileall", "-q", "."],
            "",
        ));
    }
    if has_ext(root, "csproj") || has_ext(root, "sln") {
        out.push(mk(
            "dotnet",
            "dotnet build",
            "dotnet",
            &["build", "--no-restore"],
            "",
        ));
    }
    if has("go.mod") {
        out.push(mk("go", "go build", "go", &["build", "./..."], ""));
    }
    if has("pom.xml") {
        out.push(mk("maven", "mvn compile", "mvn", &["-q", "-o", "compile"], ""));
    }
    if has("build.gradle") || has("build.gradle.kts") {
        out.push(mk(
            "gradle",
            "gradle compileJava",
            "gradle",
            &["--offline", "compileJava"],
            "",
        ));
    }
    out
}

fn looks_like_error(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    l.contains("error") || l.contains("failed") || l.contains("traceback") || l.contains("cannot find")
}

/// Run `checker` in `root`, calling `on_line` for every output line (stdout and stderr).
pub async fn run(
    root: &Path,
    checker: &Checker,
    cancel: &CancellationToken,
    mut on_line: impl FnMut(&str) + Send,
) -> VerifyResult {
    let dir: PathBuf = if checker.dir.is_empty() {
        root.to_owned()
    } else {
        root.join(&checker.dir)
    };
    let mut result = VerifyResult {
        ok: false,
        exit_code: None,
        cancelled: false,
        timed_out: false,
        missing: false,
        first_errors: Vec::new(),
    };
    let spawned = Command::new(&checker.program)
        .args(&checker.args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn();
    let mut child = match spawned {
        Ok(c) => c,
        Err(_) => {
            result.missing = true;
            return result;
        }
    };
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    if let Some(out) = child.stdout.take() {
        let tx = tx.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(out).lines();
            while let Ok(Some(l)) = lines.next_line().await {
                let _ = tx.send(l);
            }
        });
    }
    if let Some(err) = child.stderr.take() {
        let tx = tx.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(err).lines();
            while let Ok(Some(l)) = lines.next_line().await {
                let _ = tx.send(l);
            }
        });
    }
    drop(tx);
    let deadline = tokio::time::sleep(TIMEOUT);
    tokio::pin!(deadline);
    let mut status = None;
    loop {
        tokio::select! {
            line = rx.recv() => match line {
                Some(l) => {
                    if result.first_errors.len() < MAX_ERRORS && looks_like_error(&l) {
                        result.first_errors.push(l.clone());
                    }
                    on_line(&l);
                }
                None => {
                    status = child.wait().await.ok();
                    break;
                }
            },
            () = cancel.cancelled() => {
                let _ = child.kill().await;
                result.cancelled = true;
                break;
            }
            () = &mut deadline => {
                let _ = child.kill().await;
                result.timed_out = true;
                break;
            }
        }
    }
    if let Some(s) = status {
        result.exit_code = s.code();
        result.ok = s.success();
    }
    result
}
