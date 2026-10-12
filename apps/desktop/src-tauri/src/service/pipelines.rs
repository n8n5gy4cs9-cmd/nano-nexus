//! Glue between the pipeline assistants (nexus-agent) and the app: finding what to analyze, running the
//! pipeline with the user's permissions, saving reports and plans, and turning a stream plan into a turn.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use nexus_agent::pipelines::doc::{QaRequest, SummaryRequest, doc_qa, doc_summarize};
use nexus_agent::pipelines::folder::{FolderRequest, folder_report};
use nexus_agent::pipelines::simple::{
    commit_message, data_profile, file_pipeline, log_triage, secrets_report, staged_diff,
};
use nexus_agent::pipelines::tidy::tidy_plan;
use nexus_agent::pipelines::{
    Artifact, PipelineCx, PipelineError, PipelineOutput, SchedulerJson, StreamPlan,
};
use nexus_agent::router::resolve_path;
use nexus_agent::tools::{EngineGatekeeper, ToolError};
use nexus_agent::{AssistantSpec, HistoryMessage, TurnInput};
use nexus_analyze::DupMode;
use nexus_core::chat::{Attachment, AttachmentKind, ChatEvent, MessageRole};
use nexus_core::error::NexusError;
use nexus_core::ids::MessageId;
use nexus_core::sink::Sink;
use nexus_store::ReportRow;
use tokio_util::sync::CancellationToken;

use crate::state::AppCtx;

pub fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_default()
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Map a tool/gatekeeper failure to the app's error type.
pub fn tool_err(e: ToolError) -> NexusError {
    match e {
        ToolError::NotGranted { path, .. } | ToolError::Declined(path) => NexusError::PermissionDenied(path),
        ToolError::Protected(p) => NexusError::PathDenied(p),
        ToolError::NotFound(p) => NexusError::NotFound(p),
        other => NexusError::InvalidInput(other.to_string()),
    }
}

pub fn pipeline_err(e: PipelineError) -> NexusError {
    match e {
        PipelineError::Access(t) => tool_err(t),
        PipelineError::Input(m) => NexusError::InvalidInput(m),
        PipelineError::Failed(m) => NexusError::Internal(m),
        PipelineError::Cancelled => NexusError::Cancelled,
    }
}

/// A gatekeeper that asks the user (through the permission modal) when a grant is missing.
pub fn gatekeeper(ctx: &Arc<AppCtx>, actor: &str) -> EngineGatekeeper {
    gatekeeper_for(ctx, actor, None)
}

/// Like [`gatekeeper`], labelling prompts with the paired device that started the chat.
pub fn gatekeeper_for(ctx: &Arc<AppCtx>, actor: &str, requested_by: Option<String>) -> EngineGatekeeper {
    EngineGatekeeper::new(
        ctx.perms.clone(),
        actor,
        Some(Arc::new(super::permissions::BrokerAsker {
            ctx: ctx.clone(),
            requested_by,
        })),
    )
}

/// Save a report: the JSON body in the data folder plus the index row. Returns the report id.
pub fn save_report(
    ctx: &AppCtx,
    kind: &str,
    target: &str,
    title: &str,
    summary: Option<String>,
    json: &str,
) -> Result<String, NexusError> {
    let id = MessageId::new().0;
    std::fs::create_dir_all(&ctx.paths.reports)?;
    let file = ctx.paths.reports.join(format!("{id}.json"));
    std::fs::write(&file, json)?;
    ctx.store.insert_report(&ReportRow {
        id: id.clone(),
        kind: kind.into(),
        target: target.into(),
        title: title.into(),
        summary,
        json_path: file.display().to_string(),
        ai_narrative: None,
        created_at: now_ms(),
    })?;
    Ok(id)
}

/// The folder or file a message is about: an attachment's path, else the first path in the text.
pub fn extract_target(text: &str, attachments: &[Attachment], roots: &[PathBuf]) -> Option<PathBuf> {
    if let Some(p) = attachments
        .iter()
        .filter(|a| matches!(a.kind, AttachmentKind::File | AttachmentKind::Folder))
        .find_map(|a| a.path.clone())
    {
        return Some(PathBuf::from(p));
    }
    let home = home_dir();
    text.split_whitespace()
        .map(|t| t.trim_matches(|c: char| matches!(c, '"' | '\'' | '`' | ',' | ';' | '?' | '!' | '(' | ')')))
        .filter(|t| t.starts_with('/') || t.starts_with("~/") || t == &"~" || t.contains(":\\"))
        .find_map(|t| resolve_path(t.trim_end_matches('.'), &home, roots))
}

/// A turn input for a stream plan: the plan's prompts, the pipeline's sampling, no chat history.
pub fn plan_input(base: &AssistantSpec, plan: &StreamPlan, ctx_size: u32) -> TurnInput {
    let mut assistant = base.clone();
    assistant.model.temperature = plan.sampling.temperature;
    assistant.model.top_p = plan.sampling.top_p;
    assistant.model.max_tokens = plan.sampling.max_tokens;
    assistant.model.thinking = false;
    let mut history = Vec::new();
    for (u, a) in &plan.examples {
        history.push(HistoryMessage {
            id: MessageId::new(),
            role: MessageRole::User,
            content: u.clone(),
        });
        history.push(HistoryMessage {
            id: MessageId::new(),
            role: MessageRole::Assistant,
            content: a.clone(),
        });
    }
    TurnInput {
        assistant,
        system_prompt: plan.system.clone(),
        history,
        summary: None,
        user_text: plan.user.clone(),
        attachments: Vec::new(),
        think: false,
        ctx_size,
    }
}

fn chip(sink: &dyn Sink<ChatEvent>, args: serde_json::Value, summary: &str) {
    sink.send(ChatEvent::ToolCall {
        tool: "report".into(),
        args_json: args.to_string(),
    });
    sink.send(ChatEvent::ToolResult {
        tool: "report".into(),
        summary: summary.into(),
        ok: true,
    });
}

/// Persist what a pipeline produced and tell the chat about it (a clickable chip).
pub fn store_artifact(ctx: &AppCtx, artifact: &Artifact, target: &str, sink: &dyn Sink<ChatEvent>) {
    let saved: Result<(), NexusError> = (|| {
        match artifact {
            Artifact::Folder(r) => {
                let mut r = (**r).clone();
                let json = serde_json::to_string(&r).map_err(|e| NexusError::Internal(e.to_string()))?;
                let summary = format!(
                    "{} files · {}",
                    nexus_analyze::fmt::count(r.totals.files),
                    nexus_analyze::fmt::bytes(r.totals.bytes)
                );
                let id = save_report(
                    ctx,
                    "folder",
                    target,
                    &format!(
                        "Folder report: {}",
                        nexus_analyze::scan::display_root(Path::new(target), Some(&home_dir()))
                    ),
                    Some(summary),
                    &json,
                )?;
                r.id = id.clone();
                chip(
                    sink,
                    serde_json::json!({"report_id": id, "kind": "folder"}),
                    "Open the full report",
                );
            }
            Artifact::File(r) => {
                let json = serde_json::to_string(&**r).map_err(|e| NexusError::Internal(e.to_string()))?;
                let id = save_report(
                    ctx,
                    "file",
                    target,
                    &format!("File report: {}", r.name),
                    None,
                    &json,
                )?;
                chip(
                    sink,
                    serde_json::json!({"report_id": id, "kind": "file"}),
                    "Open the file report",
                );
            }
            Artifact::Secrets(r) => {
                let json = serde_json::to_string(&**r).map_err(|e| NexusError::Internal(e.to_string()))?;
                let id = save_report(
                    ctx,
                    "secrets",
                    target,
                    "Secrets scan",
                    Some(format!("{} findings", r.findings.len())),
                    &json,
                )?;
                chip(
                    sink,
                    serde_json::json!({"report_id": id, "kind": "secrets"}),
                    "Open the secrets report",
                );
            }
            Artifact::Logs(r) => {
                let json = serde_json::to_string(&**r).map_err(|e| NexusError::Internal(e.to_string()))?;
                let id = save_report(
                    ctx,
                    "logs",
                    target,
                    "Log report",
                    Some(format!("{} lines", r.lines)),
                    &json,
                )?;
                chip(
                    sink,
                    serde_json::json!({"report_id": id, "kind": "logs"}),
                    "Open the log report",
                );
            }
            Artifact::Plan(cs) => {
                let id = super::fs::changeset_plan(ctx, &cs.title, "tidy", cs.ops.clone())?;
                chip(
                    sink,
                    serde_json::json!({"changeset_id": id.0}),
                    "Review the plan in Changes",
                );
            }
            Artifact::Commit(_) | Artifact::None => {}
        }
        Ok(())
    })();
    if let Err(e) = saved {
        tracing::warn!("could not save a pipeline result: {e}");
    }
}

/// Run the pipeline named by `spec.workflow.pipeline`. `Ok(None)` = this pipeline has no code yet.
pub async fn run_pipeline(
    ctx: &Arc<AppCtx>,
    spec: &AssistantSpec,
    user_text: &str,
    attachments: &[Attachment],
    sink: &dyn Sink<ChatEvent>,
    cancel: &CancellationToken,
    requested_by: Option<String>,
) -> Result<Option<(PipelineOutput, String)>, PipelineError> {
    let Some(name) = spec.workflow.pipeline.as_deref() else {
        return Ok(None);
    };
    let gate = gatekeeper_for(ctx, &spec.id, requested_by);
    let llm = SchedulerJson(Arc::new(ctx.scheduler.clone()));
    let status = |t: String| {
        sink.send(ChatEvent::Status { text: t });
    };
    let cx = PipelineCx {
        llm: &llm,
        gate: &gate,
        cancel,
        status: &status,
        home: Some(home_dir()),
    };
    let roots: Vec<PathBuf> = ctx
        .perms
        .grants()
        .into_iter()
        .map(|g| PathBuf::from(g.root))
        .collect();
    let target = extract_target(user_text, attachments, &roots);
    let need = |what: &str| {
        PipelineError::Input(format!(
            "Tell me which {what} to use: attach it, or write its full path (like ~/Downloads)."
        ))
    };
    let goal = (!user_text.trim().is_empty()).then(|| user_text.trim().to_owned());
    let shown = target
        .as_ref()
        .map(|t| t.display().to_string())
        .unwrap_or_default();
    let out = match name {
        "folder_report" => {
            folder_report(
                &cx,
                FolderRequest {
                    root: target.ok_or_else(|| need("folder"))?,
                    user_goal: goal,
                    duplicates: DupMode::Full,
                },
            )
            .await?
        }
        "doc_qa" => {
            doc_qa(
                &cx,
                QaRequest {
                    path: target.ok_or_else(|| need("document"))?,
                    question: user_text.trim().to_owned(),
                },
            )
            .await?
        }
        "doc_summarize" => {
            doc_summarize(
                &cx,
                SummaryRequest {
                    path: target.ok_or_else(|| need("document"))?,
                    user_request: goal,
                },
            )
            .await?
        }
        "tidy_plan" => tidy_plan(&cx, target.ok_or_else(|| need("folder"))?, now_ms()).await?,
        "secrets_report" => secrets_report(&cx, target.ok_or_else(|| need("folder"))?).await?,
        "log_triage" => log_triage(&cx, target.ok_or_else(|| need("log file"))?, goal).await?,
        "data_profile" => data_profile(&cx, target.ok_or_else(|| need("data file"))?, goal).await?,
        "commit_message" => {
            let pasted = attachments
                .iter()
                .filter_map(|a| a.snapshot.clone())
                .chain(std::iter::once(user_text.to_owned()))
                .find(|t| t.contains("\n+++ ") || t.starts_with("diff --git"));
            let diff = match (pasted, target) {
                (Some(d), _) => d,
                (None, Some(repo)) => staged_diff(&cx, repo).await?,
                (None, None) => {
                    return Err(PipelineError::Input(
                        "Paste a diff, or give me the path of a git repository with staged changes.".into(),
                    ));
                }
            };
            commit_message(&cx, &diff).await?
        }
        "file_report" => file_pipeline(&cx, target.ok_or_else(|| need("file"))?, goal).await?,
        _ => return Ok(None),
    };
    Ok(Some((out, shown)))
}
