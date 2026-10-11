//! `folder_report`: scan → aggregate → facts → streamed narrative (ASSISTANTS §3.3).

use std::path::PathBuf;

use nexus_analyze::dups::DupMode;
use nexus_analyze::facts::folder_facts;
use nexus_analyze::scan::{ScanOptions, display_root, scan_folder};
use serde_json::json;

use super::{Artifact, PipelineCx, PipelineError, PipelineOutput, authorize_read, stream_plan, unwrap_facts};

pub struct FolderRequest {
    pub root: PathBuf,
    /// "Find what to clean up", or whatever the user asked.
    pub user_goal: Option<String>,
    pub duplicates: DupMode,
}

/// Scan `req.root` and prepare the narrative. The scan itself runs off the async threads and reports
/// progress to the chat status line.
pub async fn folder_report(cx: &PipelineCx<'_>, req: FolderRequest) -> Result<PipelineOutput, PipelineError> {
    let root = authorize_read(
        cx,
        &req.root,
        &format!(
            "to scan {}",
            req.root.file_name().map_or_else(
                || req.root.display().to_string(),
                |n| n.to_string_lossy().into_owned()
            )
        ),
    )
    .await?;
    if !root.is_dir() {
        return Err(PipelineError::Input(format!(
            "{} is not a folder. Pick a folder to scan.",
            root.display()
        )));
    }
    cx.say("Scanning…");
    let mut opts = ScanOptions::new(&root);
    opts.duplicates = req.duplicates;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let cancel = cx.cancel.clone();
    let job = tokio::task::spawn_blocking(move || {
        scan_folder(
            &opts,
            &move |p| {
                let _ = tx.send(match p.phase.as_str() {
                    "hashing" => "Looking for duplicates…".to_owned(),
                    "summarizing" => "Summing up…".to_owned(),
                    _ => format!("Scanning… {} files", nexus_analyze::fmt::count(p.files_seen)),
                });
            },
            &cancel,
        )
    });
    tokio::pin!(job);
    let report = loop {
        tokio::select! {
            Some(msg) = rx.recv() => cx.say(msg),
            done = &mut job => break done.map_err(|e| PipelineError::Failed(e.to_string()))?,
        }
    };
    let report = report.map_err(|e| match e {
        nexus_analyze::ScanError::Cancelled => PipelineError::Cancelled,
        other => PipelineError::Input(other.to_string()),
    })?;
    cx.check_cancel()?;
    let shown = display_root(&root, cx.home.as_deref());
    let facts = folder_facts(&report, &shown);
    let vars = json!({"facts": unwrap_facts(&facts), "user_goal": req.user_goal.unwrap_or_default()});
    let plan = stream_plan("folder_report_narrative", &vars, facts)?;
    cx.say("Writing the summary…");
    Ok(PipelineOutput::stream(plan, Artifact::Folder(Box::new(report))))
}

#[cfg(test)]
mod tests {
    use super::super::testkit::{Scripted, gate_for, no_grants, write};
    use super::*;
    use std::sync::Mutex;
    use tokio_util::sync::CancellationToken;

    fn cx<'a>(
        llm: &'a Scripted,
        gate: &'a crate::tools::EngineGatekeeper,
        cancel: &'a CancellationToken,
        log: &'a Mutex<Vec<String>>,
    ) -> PipelineCx<'a> {
        PipelineCx {
            llm,
            gate,
            cancel,
            status: Box::leak(Box::new(move |s: String| log.lock().unwrap().push(s))),
            home: None,
        }
    }

    #[tokio::test]
    async fn scans_and_prepares_a_grounded_narrative_prompt() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "Movies/talk.mov", &vec![1u8; 30_000]);
        write(d.path(), "Movies/talk copy.mov", &vec![1u8; 30_000]);
        write(d.path(), "docs/a.pdf", &[0u8; 500]);
        let (llm, gate, cancel, log) = (
            Scripted::new(vec![]),
            gate_for(d.path()),
            CancellationToken::new(),
            Mutex::new(vec![]),
        );
        let out = folder_report(
            &cx(&llm, &gate, &cancel, &log),
            FolderRequest {
                root: d.path().to_owned(),
                user_goal: Some("free up space".into()),
                duplicates: DupMode::Full,
            },
        )
        .await
        .unwrap();
        let plan = out.stream.unwrap();
        assert!(
            plan.system
                .starts_with("You write the summary of a folder scan report")
        );
        assert!(
            plan.user.contains("User goal: free up space")
                && plan.user.contains("<facts>\nroot:")
                && plan.user.contains("duplicates: 1 groups"),
            "{}",
            plan.user
        );
        assert!(
            !plan.user.contains("<facts>\n<facts>"),
            "facts tags are not doubled"
        );
        assert_eq!(plan.sampling.max_tokens, 600);
        match out.artifact {
            Artifact::Folder(r) => assert_eq!(r.totals.files, 3),
            other => panic!("{other:?}"),
        }
        assert_eq!(llm.calls(), 0, "the folder report needs no JSON step");
        assert!(log.lock().unwrap().iter().any(|s| s.starts_with("Scanning")));
    }

    #[tokio::test]
    async fn needs_a_grant_a_folder_and_stops_on_cancel() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "a.txt", b"x");
        let (llm, cancel, log) = (
            Scripted::new(vec![]),
            CancellationToken::new(),
            Mutex::new(vec![]),
        );
        let denied = no_grants();
        let err = folder_report(
            &cx(&llm, &denied, &cancel, &log),
            FolderRequest {
                root: d.path().to_owned(),
                user_goal: None,
                duplicates: DupMode::Off,
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(err, PipelineError::Access(_)));
        let gate = gate_for(d.path());
        let err = folder_report(
            &cx(&llm, &gate, &cancel, &log),
            FolderRequest {
                root: d.path().join("a.txt"),
                user_goal: None,
                duplicates: DupMode::Off,
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(err, PipelineError::Input(_)));
        cancel.cancel();
        let err = folder_report(
            &cx(&llm, &gate, &cancel, &log),
            FolderRequest {
                root: d.path().to_owned(),
                user_goal: None,
                duplicates: DupMode::Off,
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(err, PipelineError::Cancelled));
    }
}
