//! The single-purpose pipelines: `secrets_report`, `log_triage`, `data_profile`, `commit_message`, and the
//! per-file report dispatcher.

use std::path::{Path, PathBuf};
use std::process::Command;

use nexus_analyze::file_report::{FileReport, file_report};
use nexus_analyze::scan::display_root;
use nexus_analyze::{logs, secrets};
use serde::Deserialize;
use serde_json::json;

use super::{
    Artifact, JsonCheck, PipelineCx, PipelineError, PipelineOutput, authorize_read, llm_schema,
    render_prompt, stream_plan, unwrap_facts,
};

const MAX_LOG_BYTES: u64 = 50 * 1024 * 1024;
/// Longest diff text sent to the model.
const MAX_DIFF_CHARS: usize = 7000;

// ---- secrets ----------------------------------------------------------------------------------

/// Scan a folder for secrets and prepare the explanation. The model sees only masked values.
pub async fn secrets_report(cx: &PipelineCx<'_>, root: PathBuf) -> Result<PipelineOutput, PipelineError> {
    let root = authorize_read(cx, &root, "to look for secrets").await?;
    if !root.is_dir() {
        return Err(PipelineError::Input("Pick a folder to scan for secrets.".into()));
    }
    cx.say("Looking for secrets…");
    let cancel = cx.cancel.clone();
    let r2 = root.clone();
    let report = tokio::task::spawn_blocking(move || secrets::scan_folder(&r2, &cancel))
        .await
        .map_err(|e| PipelineError::Failed(e.to_string()))?
        .ok_or(PipelineError::Cancelled)?;
    let facts = secrets::facts(&report, &display_root(&root, cx.home.as_deref()));
    let plan = stream_plan("secrets_summary", &json!({"facts": unwrap_facts(&facts)}), facts)?;
    Ok(PipelineOutput::stream(plan, Artifact::Secrets(Box::new(report))))
}

// ---- logs -------------------------------------------------------------------------------------

pub async fn log_triage(
    cx: &PipelineCx<'_>,
    path: PathBuf,
    question: Option<String>,
) -> Result<PipelineOutput, PipelineError> {
    let path = authorize_read(cx, &path, "to read the log").await?;
    let meta = std::fs::metadata(&path).map_err(|e| PipelineError::Input(e.to_string()))?;
    if !meta.is_file() {
        return Err(PipelineError::Input("Pick a log file.".into()));
    }
    if meta.len() > MAX_LOG_BYTES {
        return Err(PipelineError::Input(
            "This log is larger than 50 MB. Open a smaller slice of it.".into(),
        ));
    }
    cx.say("Reading the log…");
    let p2 = path.clone();
    let report = tokio::task::spawn_blocking(move || -> Result<logs::LogReport, String> {
        let f = std::fs::File::open(&p2).map_err(|e| e.to_string())?;
        Ok(logs::analyze(std::io::BufReader::new(f)))
    })
    .await
    .map_err(|e| PipelineError::Failed(e.to_string()))?
    .map_err(PipelineError::Input)?;
    let facts = logs::facts(&report);
    let plan = stream_plan(
        "log_triage",
        &json!({"facts": unwrap_facts(&facts), "user_question": question.unwrap_or_default()}),
        facts,
    )?;
    Ok(PipelineOutput::stream(plan, Artifact::Logs(Box::new(report))))
}

// ---- data -------------------------------------------------------------------------------------

pub async fn data_profile(
    cx: &PipelineCx<'_>,
    path: PathBuf,
    question: Option<String>,
) -> Result<PipelineOutput, PipelineError> {
    let path = authorize_read(cx, &path, "to profile the data").await?;
    cx.say("Profiling the data…");
    let rep = tokio::task::spawn_blocking(move || file_report(&path))
        .await
        .map_err(|e| PipelineError::Failed(e.to_string()))?
        .map_err(|e| PipelineError::Input(e.to_string()))?;
    stream_from_file_report(rep, question, "data_profile")
}

fn stream_from_file_report(
    rep: FileReport,
    question: Option<String>,
    expect: &str,
) -> Result<PipelineOutput, PipelineError> {
    let llm = rep.llm.clone().filter(|l| l.prompt == expect).ok_or_else(|| {
        PipelineError::Input(format!(
            "{} is not a file this assistant can work with.",
            rep.name
        ))
    })?;
    let plan = stream_plan(
        expect,
        &json!({"facts": unwrap_facts(&llm.facts_text), "user_question": question.unwrap_or_default(), "excerpt": llm.excerpt.clone().unwrap_or_default()}),
        llm.facts_text.clone(),
    )?;
    Ok(PipelineOutput::stream(plan, Artifact::File(Box::new(rep))))
}

/// One file's report, then the matching explanation step when there is one.
pub async fn file_pipeline(
    cx: &PipelineCx<'_>,
    path: PathBuf,
    question: Option<String>,
) -> Result<PipelineOutput, PipelineError> {
    let path = authorize_read(cx, &path, "to analyze the file").await?;
    cx.say("Analyzing the file…");
    let rep = tokio::task::spawn_blocking(move || file_report(&path))
        .await
        .map_err(|e| PipelineError::Failed(e.to_string()))?
        .map_err(|e| PipelineError::Input(e.to_string()))?;
    match rep.llm.as_ref().map(|l| l.prompt.clone()) {
        Some(p) if p == "file_report_code" => {
            let l = rep.llm.clone().unwrap_or_else(|| unreachable!());
            let plan = stream_plan(
                "file_report_code",
                &json!({"facts": unwrap_facts(&l.facts_text), "excerpt": l.excerpt.unwrap_or_default()}),
                l.facts_text,
            )?;
            Ok(PipelineOutput::stream(plan, Artifact::File(Box::new(rep))))
        }
        Some(p) if p == "data_profile" || p == "log_triage" => {
            stream_from_file_report(rep.clone(), question, &p)
        }
        _ => {
            let text = describe(&rep);
            Ok(PipelineOutput::text(text, Artifact::File(Box::new(rep))))
        }
    }
}

/// A plain Markdown rendering of a file report (used when no model step follows).
pub fn describe(r: &FileReport) -> String {
    let mut o = format!("**{}**\n\n", r.name);
    for f in &r.facts {
        o.push_str(&format!("- {}: {}\n", f.label, f.value));
    }
    for fl in &r.flags {
        o.push_str(&format!("\n⚠ {}\n", fl.text));
    }
    for t in &r.tables {
        o.push_str(&format!(
            "\n**{}**\n\n| {} |\n|{}|\n",
            t.title,
            t.columns.join(" | "),
            t.columns.iter().map(|_| "---").collect::<Vec<_>>().join("|")
        ));
        for row in &t.rows {
            o.push_str(&format!(
                "| {} |\n",
                row.iter()
                    .map(|c| c.replace('|', "\\|"))
                    .collect::<Vec<_>>()
                    .join(" | ")
            ));
        }
    }
    o
}

// ---- commit messages --------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct CommitParts {
    #[serde(rename = "type")]
    kind: String,
    scope: String,
    subject: String,
    body: Vec<String>,
    breaking: bool,
}

/// Files, insertions, and deletions of a unified diff.
pub fn diff_stats(diff: &str) -> (usize, usize, usize) {
    let (mut files, mut ins, mut del) = (0, 0, 0);
    for l in diff.lines() {
        if l.starts_with("+++ ") {
            files += 1;
        } else if l.starts_with('+') && !l.starts_with("+++") {
            ins += 1;
        } else if l.starts_with('-') && !l.starts_with("---") {
            del += 1;
        }
    }
    (files, ins, del)
}

fn changed_paths(diff: &str) -> Vec<String> {
    diff.lines()
        .filter_map(|l| l.strip_prefix("+++ b/").or_else(|| l.strip_prefix("+++ ")))
        .filter(|p| *p != "/dev/null")
        .map(str::to_owned)
        .collect()
}

/// Render the parts as a Conventional Commits message.
pub fn render_commit(p: &CommitPartsPublic) -> String {
    let mut head = p.kind.clone();
    if !p.scope.is_empty() {
        head.push_str(&format!("({})", p.scope));
    }
    if p.breaking {
        head.push('!');
    }
    head.push_str(&format!(": {}", p.subject.trim().trim_end_matches('.')));
    let mut out = head;
    if !p.body.is_empty() {
        out.push_str("\n\n");
        out.push_str(
            &p.body
                .iter()
                .map(|l| format!("- {}", l.trim().trim_start_matches("- ")))
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
    if p.breaking {
        out.push_str("\n\nBREAKING CHANGE: see the changes above");
    }
    out
}

/// Public mirror of the commit parts for rendering and tests.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitPartsPublic {
    pub kind: String,
    pub scope: String,
    pub subject: String,
    pub body: Vec<String>,
    pub breaking: bool,
}

/// What the message would be without the model: type from the paths, subject from the first file.
pub fn fallback_commit(diff: &str) -> CommitPartsPublic {
    let paths = changed_paths(diff);
    let all = |f: &dyn Fn(&str) -> bool| !paths.is_empty() && paths.iter().all(|p| f(p));
    let kind = if all(&|p| p.ends_with(".md") || p.starts_with("docs/")) {
        "docs"
    } else if all(&|p| p.contains("test") || p.contains("spec")) {
        "test"
    } else if all(&|p| {
        p.ends_with(".lock") || p.ends_with(".toml") || p.ends_with("package.json") || p.ends_with("Makefile")
    }) {
        "build"
    } else {
        "chore"
    };
    let first = paths.first().cloned().unwrap_or_default();
    let scope = first
        .split('/')
        .next()
        .filter(|s| first.contains('/') && s.chars().all(|c| c.is_ascii_alphanumeric() || "_.-".contains(c)))
        .unwrap_or("")
        .to_lowercase();
    let name = first.rsplit('/').next().unwrap_or("changes");
    let subject = if paths.len() > 1 {
        format!(
            "update {name} and {} more file{}",
            paths.len() - 1,
            if paths.len() == 2 { "" } else { "s" }
        )
    } else {
        format!("update {name}")
    };
    CommitPartsPublic {
        kind: kind.into(),
        scope,
        subject: subject.chars().take(72).collect(),
        body: Vec::new(),
        breaking: false,
    }
}

pub async fn commit_message(cx: &PipelineCx<'_>, diff: &str) -> Result<PipelineOutput, PipelineError> {
    if diff.trim().is_empty() {
        return Err(PipelineError::Input(
            "Paste a diff (or stage some changes) first.".into(),
        ));
    }
    let (files, ins, del) = diff_stats(diff);
    let shown: String = if diff.chars().count() > MAX_DIFF_CHARS {
        format!(
            "{}\n[diff truncated: {} more characters]",
            diff.chars().take(MAX_DIFF_CHARS).collect::<String>(),
            diff.chars().count() - MAX_DIFF_CHARS
        )
    } else {
        diff.to_owned()
    };
    let stats = format!("{files} file{}, +{ins} −{del}", if files == 1 { "" } else { "s" });
    cx.say("Reading the diff…");
    let (prompt, sampling) = render_prompt("commit_message", &json!({"stats": stats, "diff": shown}))?;
    let schema = llm_schema(include_str!("../../../../schemas/llm/commit-message.schema.json"));
    let parts = cx
        .llm
        .json(prompt, &schema, sampling, &JsonCheck::default(), cx.cancel)
        .await
        .and_then(|v| serde_json::from_value::<CommitParts>(v).ok())
        .filter(|p| p.subject.chars().count() <= 72 && !p.subject.trim().is_empty())
        .map(|p| CommitPartsPublic {
            kind: p.kind,
            scope: p.scope,
            subject: p.subject,
            body: p.body,
            breaking: p.breaking,
        });
    let used_model = parts.is_some();
    let message = render_commit(&parts.unwrap_or_else(|| fallback_commit(diff)));
    let mut out = PipelineOutput::text(format!("```\n{message}\n```"), Artifact::Commit(message));
    if !used_model {
        out.notes.push(
            "The model's answer was not usable, so this message was written from the file names.".into(),
        );
    }
    Ok(out)
}

/// `git diff --staged` for a repository folder (read-only, fixed arguments, never through a shell).
pub async fn staged_diff(cx: &PipelineCx<'_>, repo: PathBuf) -> Result<String, PipelineError> {
    let repo = authorize_read(cx, &repo, "to read the staged changes").await?;
    run_git_diff(&repo)
}

fn run_git_diff(repo: &Path) -> Result<String, PipelineError> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["diff", "--staged", "--no-color", "--no-ext-diff"])
        .output()
        .map_err(|_| PipelineError::Input("git is not installed or not on the PATH.".into()))?;
    if !out.status.success() {
        return Err(PipelineError::Input(
            "That folder is not a git repository.".into(),
        ));
    }
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    if text.trim().is_empty() {
        return Err(PipelineError::Input(
            "There are no staged changes. Stage some with `git add` first.".into(),
        ));
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::super::testkit::{Scripted, gate_for, no_grants, write};
    use super::*;
    use std::sync::Mutex;
    use tokio_util::sync::CancellationToken;

    fn run<'a>(
        llm: &'a Scripted,
        gate: &'a crate::tools::EngineGatekeeper,
        cancel: &'a CancellationToken,
    ) -> PipelineCx<'a> {
        let log: &'static Mutex<Vec<String>> = Box::leak(Box::new(Mutex::new(vec![])));
        PipelineCx {
            llm,
            gate,
            cancel,
            status: Box::leak(Box::new(move |s: String| log.lock().unwrap().push(s))),
            home: None,
        }
    }

    const DIFF: &str = "diff --git a/src/player.py b/src/player.py\n--- a/src/player.py\n+++ b/src/player.py\n@@ -1,3 +1,4 @@\n-    velocity.y += 9.8\n+    velocity.y += gravity * delta\n+gravity = 980.0\n";

    #[test]
    fn diff_stats_count_files_and_lines() {
        assert_eq!(diff_stats(DIFF), (1, 2, 1));
        assert_eq!(diff_stats("nothing"), (0, 0, 0));
    }

    #[test]
    fn commit_rendering() {
        let p = CommitPartsPublic {
            kind: "feat".into(),
            scope: "ui".into(),
            subject: "add pause menu.".into(),
            body: vec!["Adds a menu.".into(), "- Keeps state.".into()],
            breaking: true,
        };
        assert_eq!(
            render_commit(&p),
            "feat(ui)!: add pause menu\n\n- Adds a menu.\n- Keeps state.\n\nBREAKING CHANGE: see the changes above"
        );
        let q = CommitPartsPublic {
            kind: "fix".into(),
            scope: String::new(),
            subject: "x y".into(),
            body: vec![],
            breaking: false,
        };
        assert_eq!(render_commit(&q), "fix: x y");
    }

    #[test]
    fn the_fallback_message_comes_from_the_paths() {
        let f = fallback_commit(DIFF);
        assert_eq!(
            (f.kind.as_str(), f.scope.as_str(), f.subject.as_str()),
            ("chore", "src", "update player.py")
        );
        let docs = fallback_commit("+++ b/docs/guide.md\n+x\n+++ b/README.md\n");
        assert_eq!(docs.kind, "docs");
        assert_eq!(docs.subject, "update guide.md and 1 more file");
        assert_eq!(fallback_commit("+++ b/tests/a_test.rs\n").kind, "test");
    }

    #[tokio::test]
    async fn a_good_model_answer_is_rendered() {
        let d = tempfile::tempdir().unwrap();
        let llm = Scripted::new(vec![Some(
            json!({"type": "fix", "scope": "player", "subject": "scale gravity by delta", "body": ["Was frame-rate dependent."], "breaking": false}),
        )]);
        let (gate, cancel) = (gate_for(d.path()), CancellationToken::new());
        let out = commit_message(&run(&llm, &gate, &cancel), DIFF).await.unwrap();
        assert_eq!(
            out.artifact,
            Artifact::Commit("fix(player): scale gravity by delta\n\n- Was frame-rate dependent.".into())
        );
        assert!(out.notes.is_empty() && out.text.unwrap().starts_with("```"));
        assert!(
            llm.prompts.lock().unwrap()[0]
                .user
                .contains("<stats>1 file, +2 −1</stats>")
        );
    }

    #[tokio::test]
    async fn a_bad_model_answer_falls_back_gracefully() {
        let d = tempfile::tempdir().unwrap();
        let (gate, cancel) = (gate_for(d.path()), CancellationToken::new());
        let too_long =
            json!({"type": "fix", "scope": "", "subject": "x".repeat(80), "body": [], "breaking": false});
        for reply in [None, Some(too_long), Some(json!({"nonsense": true}))] {
            let llm = Scripted::new(vec![reply]);
            let out = commit_message(&run(&llm, &gate, &cancel), DIFF).await.unwrap();
            assert!(
                matches!(&out.artifact, Artifact::Commit(m) if m.starts_with("chore(src): update player.py"))
            );
            assert_eq!(out.notes.len(), 1);
        }
        let llm = Scripted::new(vec![]);
        assert!(commit_message(&run(&llm, &gate, &cancel), "  ").await.is_err());
    }

    #[test]
    fn long_diffs_are_truncated_with_a_note() {
        let big = format!("+++ b/a.txt\n{}", "+line\n".repeat(5000));
        let d = tempfile::tempdir().unwrap();
        let llm = Scripted::new(vec![None]);
        let (gate, cancel) = (gate_for(d.path()), CancellationToken::new());
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        rt.block_on(commit_message(&run(&llm, &gate, &cancel), &big))
            .unwrap();
        let sent = llm.prompts.lock().unwrap()[0].user.clone();
        assert!(sent.contains("[diff truncated:") && sent.len() < 9000);
    }

    #[tokio::test]
    async fn secrets_log_and_data_pipelines_prepare_grounded_prompts() {
        let d = tempfile::tempdir().unwrap();
        let key = ["AKIA", "IOSFODNN7", "EXAMPLE"].concat();
        write(d.path(), "src/app.js", format!("const k = '{key}';").as_bytes());
        write(
            d.path(),
            "logs/app.log",
            b"2026-10-11 14:00:00 ERROR db down 5\n2026-10-11 14:00:01 ERROR db down 6\n",
        );
        write(d.path(), "data/t.csv", b"id,city\n1,Paris\n2,Rome\n");
        let (llm, gate, cancel) = (
            Scripted::new(vec![]),
            gate_for(d.path()),
            CancellationToken::new(),
        );
        let cx = run(&llm, &gate, &cancel);

        let out = secrets_report(&cx, d.path().to_owned()).await.unwrap();
        let plan = out.stream.unwrap();
        assert!(
            plan.user
                .contains("aws-access-key at src/app.js:1 value AKIA****MPLE"),
            "{}",
            plan.user
        );
        assert!(!plan.user.contains(&key) && !format!("{:?}", out.artifact).contains(&key));

        let out = log_triage(
            &cx,
            d.path().join("logs/app.log"),
            Some("why did it fail?".into()),
        )
        .await
        .unwrap();
        let plan = out.stream.unwrap();
        assert!(
            plan.user.contains("Question: why did it fail?") && plan.user.contains("×2 db down <N>"),
            "{}",
            plan.user
        );

        let out = data_profile(&cx, d.path().join("data/t.csv"), None)
            .await
            .unwrap();
        assert!(out.stream.unwrap().user.contains("rows: 2   columns: 2"));
        assert!(
            data_profile(&cx, d.path().join("logs/app.log"), None)
                .await
                .is_err(),
            "a log is not data"
        );
        assert!(log_triage(&cx, d.path().to_owned(), None).await.is_err());
    }

    #[tokio::test]
    async fn the_file_pipeline_picks_the_right_step() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "m.rs", b"fn main() {}\n");
        write(
            d.path(),
            "p.png",
            b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x10\0\0\0\x10\x08\x02\0\0\0\0\0\0\0",
        );
        let (llm, gate, cancel) = (
            Scripted::new(vec![]),
            gate_for(d.path()),
            CancellationToken::new(),
        );
        let cx = run(&llm, &gate, &cancel);
        let code = file_pipeline(&cx, d.path().join("m.rs"), None).await.unwrap();
        assert!(code.stream.unwrap().user.contains("fn main() {}"));
        let img = file_pipeline(&cx, d.path().join("p.png"), None).await.unwrap();
        assert!(img.stream.is_none() && img.text.unwrap().contains("16 × 16 px"));
        let denied = no_grants();
        assert!(
            file_pipeline(&run(&llm, &denied, &cancel), d.path().join("m.rs"), None)
                .await
                .is_err()
        );
    }

    #[test]
    fn git_staged_diff_reads_a_real_repo() {
        if Command::new("git").arg("--version").output().is_err() {
            return;
        }
        let d = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| {
            Command::new("git")
                .arg("-C")
                .arg(d.path())
                .args(args)
                .output()
                .unwrap()
        };
        git(&["init", "-q"]);
        write(d.path(), "a.txt", b"hello\n");
        git(&["add", "a.txt"]);
        let diff = run_git_diff(d.path()).unwrap();
        assert!(diff.contains("+++ b/a.txt") && diff.contains("+hello"));
        git(&["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-qm", "x"]);
        assert!(matches!(run_git_diff(d.path()), Err(PipelineError::Input(m)) if m.contains("no staged")));
        let plain = tempfile::tempdir().unwrap();
        assert!(
            matches!(run_git_diff(plain.path()), Err(PipelineError::Input(m)) if m.contains("not a git"))
        );
    }
}
