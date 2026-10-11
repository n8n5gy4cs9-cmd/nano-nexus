//! The code editor's backend (M10): saves go through a change set, AI actions stream from the model.
//!
//! There is deliberately no other write path: `save` plans one `Write` op and applies it with the
//! journal, so every editor save is undoable from Changes (ADR-005).

use std::path::Path;
use std::sync::Arc;

use futures_util::StreamExt;
use nexus_agent::internal::{internal_source, render_pair};
use nexus_core::changes::FileOp;
use nexus_core::error::NexusError;
use nexus_core::fs::Access;
use nexus_core::ids::MessageId;
use nexus_core::sink::Sink;
use nexus_llm::{ChatMessage, ChatRequest, Priority, StreamEvent};
use serde::{Deserialize, Serialize};
use specta::Type;

use super::{Caller, fs};
use crate::state::Ctx;

/// Largest buffer the editor will save.
pub const MAX_SAVE_BYTES: usize = 2 * 1024 * 1024;
/// Most characters of a selection sent to the model.
const MAX_SELECTION_CHARS: usize = 12_000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Type)]
pub struct EditorSaveResult {
    pub changeset_id: String,
    /// blake3 of the saved contents: the base hash of the next save.
    pub hash: String,
}

fn actor(caller: &Caller) -> String {
    match caller {
        Caller::Desktop => "user".into(),
        Caller::Remote { device_id, .. } => format!("device:{device_id}"),
    }
}

fn desktop_only(caller: &Caller) -> Result<(), NexusError> {
    if caller.is_desktop() { Ok(()) } else { Err(NexusError::ForbiddenRemote) }
}

/// Does a grant allow writing this file? (`false` puts the editor in read-only mode.)
pub fn can_write(ctx: &Ctx, caller: &Caller, path: &str) -> bool {
    caller.is_desktop() && ctx.perms.check(&actor(caller), Path::new(path), Access::Write).is_ok()
}

/// Save the buffer: plan one `Write` op (guarded by `base_hash`) and apply it. Blocking (disk work).
pub fn save(
    ctx: &Ctx,
    caller: &Caller,
    path: &str,
    contents: &str,
    base_hash: Option<&str>,
) -> Result<EditorSaveResult, NexusError> {
    desktop_only(caller)?;
    if contents.len() > MAX_SAVE_BYTES {
        return Err(NexusError::InvalidInput("the file is larger than 2 MB; use another tool for it".into()));
    }
    let name = Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_owned());
    let ops = vec![FileOp::Write {
        path: path.to_owned(),
        base_hash: base_hash.map(str::to_owned),
        contents: contents.to_owned(),
    }];
    let id = fs::changeset_plan(ctx, &format!("Edit {name}"), "editor", ops)?;
    if let Err(e) = fs::changeset_apply(ctx, caller, id.as_str()) {
        // Nothing was written; do not leave a dangling plan in the Changes list.
        if let Err(d) = fs::changeset_discard(ctx, caller, id.as_str()) {
            tracing::warn!("could not discard the failed editor change set: {d}");
        }
        return Err(e);
    }
    Ok(EditorSaveResult {
        changeset_id: id.0,
        hash: nexus_fs::hash_bytes(contents.as_bytes()),
    })
}

/// What the AI panel asks for.
#[derive(Clone, Debug, Deserialize, Type)]
pub struct EditorAiRequest {
    /// `improve_names` · `add_docs` · `simplify` · `fix_bugs` (rewrites) · `explain` · `ask` (text answers).
    pub action: String,
    pub language: String,
    pub file_name: String,
    pub selection: String,
    pub context_before: String,
    pub context_after: String,
    /// The user's question or instruction for `ask`.
    pub instructions: Option<String>,
}

/// Streamed to the panel.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EditorAiEvent {
    Content { delta: String },
    Done,
    Error { message: String },
}

fn clip(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

/// Build the model request for an action (pure, so it is testable).
pub fn build_request(req: &EditorAiRequest) -> Result<ChatRequest, NexusError> {
    let selection = clip(&req.selection, MAX_SELECTION_CHARS);
    let messages = match req.action.as_str() {
        "improve_names" | "add_docs" | "simplify" | "fix_bugs" => {
            let src = internal_source("editor_rewrite")
                .ok_or_else(|| NexusError::Internal("editor_rewrite prompt is missing".into()))?;
            let pair = render_pair(
                src,
                &serde_json::json!({
                    "action": req.action,
                    "language": req.language,
                    "file_name": req.file_name,
                    "selection": selection,
                    "context_before": clip(&req.context_before, 4000),
                    "context_after": clip(&req.context_after, 2000),
                    "instructions": req.instructions.clone().unwrap_or_default(),
                }),
            )
            .map_err(|e| NexusError::Internal(e.to_string()))?;
            vec![ChatMessage::system(pair.system), ChatMessage::user(pair.user)]
        }
        "explain" | "ask" => {
            let task = if req.action == "explain" {
                "Explain what this code does in plain language. Be short and concrete.".to_owned()
            } else {
                req.instructions.clone().unwrap_or_default()
            };
            if task.trim().is_empty() {
                return Err(NexusError::InvalidInput("ask a question first".into()));
            }
            vec![
                ChatMessage::system(
                    "You are a careful programming assistant. Answer briefly. Use short code blocks only when needed. Do not invent code that is not in the selection.",
                ),
                ChatMessage::user(format!(
                    "File: {}\nLanguage: {}\n\nCode:\n```{}\n{}\n```\n\n{}",
                    req.file_name, req.language, req.language, selection, task
                )),
            ]
        }
        other => return Err(NexusError::InvalidInput(format!("unknown editor action `{other}`"))),
    };
    let rewrite = !matches!(req.action.as_str(), "explain" | "ask");
    let mut chat = ChatRequest::new(messages).streaming().thinking(false);
    chat.temperature = if rewrite { 0.2 } else { 0.3 };
    chat.top_p = 0.9;
    chat.max_tokens = if rewrite { 1500 } else { 800 };
    Ok(chat)
}

/// Run an AI action and stream its text to `sink`. `job_id` can be passed to [`cancel`].
pub async fn ai(
    ctx: &Ctx,
    caller: &Caller,
    job_id: &str,
    req: EditorAiRequest,
    sink: Arc<dyn Sink<EditorAiEvent>>,
) -> Result<(), NexusError> {
    desktop_only(caller)?;
    let chat = build_request(&req)?;
    // Editor jobs reuse the chat job registry (keyed by an id the UI chooses).
    let key = MessageId(format!("editor:{job_id}"));
    let token = ctx.chats.register(&key);
    let result = async {
        let mut stream = ctx.scheduler.stream(Priority::Interactive, chat, token.clone()).await?;
        while let Some(item) = stream.next().await {
            match item? {
                StreamEvent::Content(delta) => {
                    if !sink.send(EditorAiEvent::Content { delta }) {
                        break;
                    }
                }
                StreamEvent::Reasoning(_) => {}
                StreamEvent::Done { .. } => break,
            }
        }
        Ok::<(), nexus_llm::LlmError>(())
    }
    .await;
    ctx.chats.finish(&key);
    match result {
        Ok(()) => {
            sink.send(EditorAiEvent::Done);
            Ok(())
        }
        Err(nexus_llm::LlmError::Cancelled) => {
            sink.send(EditorAiEvent::Done);
            Ok(())
        }
        Err(e) => {
            let err = NexusError::from(e);
            sink.send(EditorAiEvent::Error { message: err.to_string() });
            Err(err)
        }
    }
}

/// Stop a running AI action; the text so far stays in the panel.
pub fn cancel(ctx: &Ctx, job_id: &str) -> bool {
    ctx.chats.cancel(&MessageId(format!("editor:{job_id}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(action: &str) -> EditorAiRequest {
        EditorAiRequest {
            action: action.into(),
            language: "python".into(),
            file_name: "player.py".into(),
            selection: "def f(a):\n    return a\n".into(),
            context_before: String::new(),
            context_after: String::new(),
            instructions: None,
        }
    }

    #[test]
    fn rewrite_actions_use_the_editor_prompt() {
        let r = build_request(&req("improve_names")).unwrap();
        assert!(r.messages[0].content.contains("rename unclear variables"));
        assert!(r.messages[1].content.contains("<selection>"));
        assert!(r.stream && r.max_tokens == 1500);
    }

    #[test]
    fn text_actions_need_a_question_for_ask() {
        assert!(build_request(&req("explain")).is_ok());
        assert!(build_request(&req("ask")).is_err());
        let mut asked = req("ask");
        asked.instructions = Some("why is this slow?".into());
        assert!(build_request(&asked).unwrap().messages[1].content.contains("why is this slow?"));
        assert!(build_request(&req("delete_everything")).is_err());
    }

    #[test]
    fn remotes_cannot_save() {
        let dir = tempfile::tempdir().unwrap();
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let _g = rt.enter();
        let ctx = crate::state::AppState::for_tests(dir.path());
        let remote = Caller::Remote {
            device_id: "d".into(),
            policy: nexus_core::settings::SharePolicy::Full,
        };
        let err = save(&ctx, &remote, "/x", "y", None).unwrap_err();
        assert_eq!(err.code(), nexus_core::error::ErrorCode::ForbiddenRemote);
        assert!(!can_write(&ctx, &remote, "/x"));
    }
}
