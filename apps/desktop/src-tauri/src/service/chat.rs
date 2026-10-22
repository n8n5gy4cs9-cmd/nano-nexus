//! Chat: send, stop, regenerate, edit-and-resend. Persists the streaming answer every 500 ms so a
//! crash loses at most half a second, and always ends the assistant message in a final state.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use nexus_agent::router::{RouterRequest, SchedulerRouter, run_router};
use nexus_agent::tools::{EngineGatekeeper, KnowledgeSource, ToolCx, builtin_tools};
use nexus_agent::{
    AttachmentBlock, HistoryMessage, PromptContext, StoredSummary, TurnInput, prompt, render_system_prompt,
};
use nexus_core::chat::{AttachmentInput, ChatEvent, Conversation, Message, MessageRole, MessageStatus};
use nexus_core::error::NexusError;
use nexus_core::events::RuntimeState;
use nexus_core::ids::{AssistantId, ConversationId, MessageId};
use nexus_core::sink::Sink;
use nexus_store::{MessageFinish, Store};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::state::Ctx;

use super::Caller;

/// Max characters of one message (the context window is the real limit; this stops accidents).
const MAX_TEXT_CHARS: usize = 200_000;
const PERSIST_EVERY: Duration = Duration::from_millis(500);

#[derive(Clone, Debug, Deserialize, Type)]
pub struct ChatSendArgs {
    /// `None` starts a new conversation.
    pub conversation_id: Option<ConversationId>,
    pub assistant_id: AssistantId,
    pub text: String,
    pub attachments: Vec<AttachmentInput>,
    /// "Think harder" for this turn.
    pub think: bool,
    /// Unsaved assistant TOML from the builder's test chat; used instead of `assistant_id` when set.
    #[serde(default)]
    pub draft_spec: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Type)]
pub struct ChatSendResult {
    pub conversation_id: ConversationId,
    pub user_message_id: MessageId,
    pub assistant_message_id: MessageId,
}

fn resolve_assistant(
    ctx: &Ctx,
    id: &AssistantId,
    draft: Option<&str>,
) -> Result<nexus_agent::AssistantSpec, NexusError> {
    super::assistants::resolve(ctx, id.as_str(), draft)
}

/// The user's home folder (for `~` in tool paths).
fn home_dir() -> std::path::PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(std::path::PathBuf::from)
        .unwrap_or_default()
}

/// Tools the router may use for `spec`: its allowed list (all tools exist now).
/// The tools this assistant may call now: what its definition declares, minus what the user turned
/// off for it, and without the web tools unless web access is on (ADR-016).
fn router_tools(spec: &nexus_agent::AssistantSpec, settings: &nexus_core::settings::Settings) -> Vec<String> {
    spec.tools
        .allowed
        .iter()
        .filter(|t| !settings.tool_disabled(&spec.id, t))
        .filter(|t| settings.tools_web_enabled || !matches!(t.as_str(), "search_web" | "fetch_url"))
        .cloned()
        .collect()
}

/// What the model should know about its tools this turn, so it neither invents tools it lacks nor
/// tells the user to "use the search_web tool" itself.
fn tools_note(
    spec: &nexus_agent::AssistantSpec,
    active: &[String],
    settings: &nexus_core::settings::Settings,
) -> String {
    let has = |n: &str| active.iter().any(|t| t == n);
    let declares = |n: &str| spec.tools.allowed.iter().any(|t| t == n);
    let mut note = String::new();
    if has("write_file") || has("edit_file") || has("move_file") || has("trash_file") {
        note.push_str("\nTo save or change files the app proposes the change for you; the app writes it when automatic apply is on, otherwise the user presses Apply on the change card in this chat. Say a file is written only if a <tool_result> says DONE; if it says NOT written yet, tell the user to press Apply on the change card. If no change was proposed, say what is missing (a folder grant, the file name) in one sentence.");
    } else if spec.workflow.kind != "pipeline" {
        note.push_str("\nYou cannot save files in this assistant. Put the content in a code block and tell the user the Agent assistant can save it.");
    }
    if (declares("search_web") || declares("fetch_url")) && !settings.tools_web_enabled {
        note.push_str("\nWeb access is turned off. If the user needs current information from the web, tell them to turn on Settings → Tools → Allow web access.");
    } else if has("search_web") {
        note.push_str(
            "\nWeb search is on: results appear as <web_results> in a <tool_result>. Name the page you used.",
        );
    }
    note
}

/// Stages the change sets that `write_file` and friends propose, tagged with the assistant.
struct ChatProposals {
    ctx: Arc<crate::state::AppCtx>,
    origin: String,
}

impl nexus_agent::tools::ProposalSink for ChatProposals {
    fn propose(&self, title: &str, ops: Vec<nexus_core::changes::FileOp>) -> Result<String, String> {
        super::fs::changeset_plan(&self.ctx, title, &self.origin, ops)
            .map(|id| id.0)
            .map_err(|e| e.to_string())
    }

    fn auto_apply(&self, id: &str) -> nexus_agent::tools::AutoApply {
        use nexus_agent::tools::AutoApply;
        if !self.ctx.settings_snapshot().chat_auto_apply_proposals {
            return AutoApply::Off;
        }
        match super::fs::changeset_apply(&self.ctx, &super::Caller::Desktop, id) {
            Ok(_) => AutoApply::Applied,
            Err(e) => AutoApply::Failed(e.to_string()),
        }
    }
}

/// `search_web` / `fetch_url` over [`nexus_models::web::WebClient`].
struct ChatWeb(nexus_models::web::WebClient);

#[async_trait::async_trait]
impl nexus_agent::tools::WebSource for ChatWeb {
    async fn search(&self, query: &str, max: usize) -> Result<Vec<nexus_agent::tools::WebHit>, String> {
        let hits = self.0.search(query, max).await.map_err(|e| e.to_string())?;
        Ok(hits
            .into_iter()
            .map(|h| nexus_agent::tools::WebHit {
                title: h.title,
                url: h.url,
                snippet: h.snippet,
            })
            .collect())
    }
    async fn fetch(&self, url: &str, max_chars: usize) -> Result<nexus_agent::tools::WebPage, String> {
        let p = self.0.fetch(url, max_chars).await.map_err(|e| e.to_string())?;
        Ok(nexus_agent::tools::WebPage {
            url: p.url,
            title: p.title,
            text: p.text,
        })
    }
}

/// Search indexes for long attached documents, so `doc_search` can cite pages.
fn attached_docs(
    attachments: &[nexus_core::chat::Attachment],
) -> Vec<Arc<nexus_agent::pipelines::doc::DocIndex>> {
    attachments
        .iter()
        .filter_map(|a| {
            let text = a.snapshot.as_deref().filter(|t| t.chars().count() >= 2000)?;
            let ex = nexus_analyze::extract::Extracted {
                kind: "plain".into(),
                title: None,
                pages: vec![nexus_analyze::extract::Page {
                    number: 1,
                    text: text.to_owned(),
                }],
                scanned_pages: vec![],
            };
            nexus_agent::pipelines::doc::DocIndex::from_extracted(&a.label, &ex)
                .ok()
                .map(Arc::new)
        })
        .collect()
}

/// Writes the partial answer to the store at most every `every`, and forwards every event.
pub struct PersistingSink {
    inner: Arc<dyn Sink<ChatEvent>>,
    store: Arc<Store>,
    id: MessageId,
    every: Duration,
    buf: Mutex<Buffer>,
    /// Activity page (M19): the app context and conversation id, when this turn reports its tool calls.
    observe: Option<(Ctx, String)>,
}

struct Buffer {
    content: String,
    reasoning: String,
    last_flush: Instant,
    /// `{tool, args, summary, ok}` per tool call, in order (saved as the message's `tool_trace`).
    tools: Vec<serde_json::Value>,
}

impl PersistingSink {
    pub fn new(inner: Arc<dyn Sink<ChatEvent>>, store: Arc<Store>, id: MessageId, every: Duration) -> Self {
        Self {
            inner,
            store,
            id,
            every,
            buf: Mutex::new(Buffer {
                content: String::new(),
                reasoning: String::new(),
                last_flush: Instant::now(),
                tools: Vec::new(),
            }),
            observe: None,
        }
    }

    /// Report this turn's tool calls to the Activity page (names and outcomes only).
    pub fn with_observe(mut self, ctx: Ctx, conversation: String) -> Self {
        self.observe = Some((ctx, conversation));
        self
    }

    /// The tool calls seen so far as a JSON array, or `None` when there were none.
    pub fn tool_trace(&self) -> Option<String> {
        let b = self.buf.lock().ok()?;
        (!b.tools.is_empty()).then(|| serde_json::Value::Array(b.tools.clone()).to_string())
    }
}

impl Sink<ChatEvent> for PersistingSink {
    fn send(&self, event: ChatEvent) -> bool {
        // (call id, tool, result) for the Activity page; `None` result = the call just started.
        let mut seen: Option<(String, String, Option<bool>)> = None;
        if let Ok(mut b) = self.buf.lock() {
            let dirty = match &event {
                ChatEvent::Content { delta } => {
                    b.content.push_str(delta);
                    true
                }
                ChatEvent::Reasoning { delta } => {
                    b.reasoning.push_str(delta);
                    true
                }
                ChatEvent::ToolCall { tool, args_json } => {
                    let args = serde_json::from_str::<serde_json::Value>(args_json)
                        .unwrap_or(serde_json::Value::Null);
                    b.tools
                        .push(serde_json::json!({"tool": tool, "args": args, "summary": null, "ok": null}));
                    seen = Some((format!("{}-{}", self.id, b.tools.len()), tool.clone(), None));
                    false
                }
                ChatEvent::ToolResult { tool, summary, ok } => {
                    if let Some((i, t)) = b
                        .tools
                        .iter_mut()
                        .enumerate()
                        .rev()
                        .find(|(_, t)| t["tool"] == tool.as_str() && t["ok"].is_null())
                    {
                        t["summary"] = serde_json::Value::String(summary.clone());
                        t["ok"] = serde_json::Value::Bool(*ok);
                        seen = Some((format!("{}-{}", self.id, i + 1), tool.clone(), Some(*ok)));
                    }
                    false
                }
                _ => false,
            };
            if dirty && b.last_flush.elapsed() >= self.every {
                let reasoning = (!b.reasoning.is_empty()).then_some(b.reasoning.as_str());
                if let Err(e) = self.store.update_message_stream(&self.id, &b.content, reasoning) {
                    tracing::warn!("could not persist streaming message: {e}");
                }
                b.last_flush = Instant::now();
            }
        }
        if let (Some((ctx, conv)), Some((call, tool, result))) = (&self.observe, seen) {
            match result {
                None => super::observe::tool_started(ctx, conv, &call, &tool),
                Some(ok) => super::observe::tool_finished(ctx, conv, &call, ok),
            }
        }
        self.inner.send(event)
    }
}

fn check_text(text: &str, has_attachments: bool) -> Result<(), NexusError> {
    if text.trim().is_empty() && !has_attachments {
        return Err(NexusError::InvalidInput("the message is empty".into()));
    }
    if text.chars().count() > MAX_TEXT_CHARS {
        return Err(NexusError::InvalidInput("the message is too long".into()));
    }
    Ok(())
}

/// Send a message and start the answer. Returns as soon as the messages exist; the answer streams to
/// `sink` from a background task.
pub fn send(
    ctx: &Ctx,
    caller: &Caller,
    args: ChatSendArgs,
    sink: Arc<dyn Sink<ChatEvent>>,
) -> Result<ChatSendResult, NexusError> {
    check_text(&args.text, !args.attachments.is_empty())?;
    let spec = resolve_assistant(ctx, &args.assistant_id, args.draft_spec.as_deref())?;
    let conv = ctx
        .store
        .get_or_create_conversation(args.conversation_id.as_ref(), &args.assistant_id)?;
    let user = ctx
        .store
        .insert_user_message(&conv.id, args.text.trim_end(), &args.attachments)?;
    let assistant = start_turn(ctx, &conv, &user, spec, args.think, sink, requester(ctx, caller))?;
    Ok(ChatSendResult {
        conversation_id: conv.id,
        user_message_id: user.id,
        assistant_message_id: assistant,
    })
}

/// Stop a running answer; the partial text is kept. `false` if nothing was running.
pub fn stop(ctx: &Ctx, assistant_message_id: &MessageId) -> bool {
    ctx.chats.cancel(assistant_message_id)
}

/// Redo an assistant answer: the old answer (and anything after it) is deleted and a new one streams.
pub fn regenerate(
    ctx: &Ctx,
    caller: &Caller,
    assistant_message_id: &MessageId,
    think: bool,
    sink: Arc<dyn Sink<ChatEvent>>,
) -> Result<ChatSendResult, NexusError> {
    let old = ctx
        .store
        .get_message(assistant_message_id)?
        .ok_or_else(|| NexusError::NotFound("message".into()))?;
    if old.role != MessageRole::Assistant {
        return Err(NexusError::InvalidInput(
            "only assistant messages can be regenerated".into(),
        ));
    }
    if ctx.chats.is_running(&old.id) {
        return Err(NexusError::Busy("that answer is still being written".into()));
    }
    let conv = ctx
        .store
        .get_conversation(&old.conversation_id)?
        .ok_or_else(|| NexusError::NotFound("conversation".into()))?;
    let messages = ctx.store.list_messages(&conv.id)?;
    let idx = messages
        .iter()
        .position(|m| m.id == old.id)
        .ok_or_else(|| NexusError::NotFound("message".into()))?;
    let user = messages[..idx]
        .iter()
        .rev()
        .find(|m| m.role == MessageRole::User)
        .cloned()
        .ok_or_else(|| NexusError::InvalidInput("there is no question to answer again".into()))?;
    let spec = resolve_assistant(ctx, &conv.assistant_id, None)?;
    ctx.store.delete_messages_after(&user.id)?;
    let assistant = start_turn(ctx, &conv, &user, spec, think, sink, requester(ctx, caller))?;
    Ok(ChatSendResult {
        conversation_id: conv.id,
        user_message_id: user.id,
        assistant_message_id: assistant,
    })
}

/// Replace a user message's text, drop everything after it, and answer again.
pub fn edit_resend(
    ctx: &Ctx,
    caller: &Caller,
    user_message_id: &MessageId,
    text: &str,
    think: bool,
    sink: Arc<dyn Sink<ChatEvent>>,
) -> Result<ChatSendResult, NexusError> {
    let user = ctx
        .store
        .get_message(user_message_id)?
        .ok_or_else(|| NexusError::NotFound("message".into()))?;
    if user.role != MessageRole::User {
        return Err(NexusError::InvalidInput(
            "only your own messages can be edited".into(),
        ));
    }
    check_text(text, !user.attachments.is_empty())?;
    let conv = ctx
        .store
        .get_conversation(&user.conversation_id)?
        .ok_or_else(|| NexusError::NotFound("conversation".into()))?;
    for m in ctx.store.list_messages(&conv.id)? {
        if m.role == MessageRole::Assistant && ctx.chats.is_running(&m.id) {
            return Err(NexusError::Busy("an answer is still being written".into()));
        }
    }
    let spec = resolve_assistant(ctx, &conv.assistant_id, None)?;
    ctx.store.set_message_content(&user.id, text.trim_end())?;
    ctx.store.delete_messages_after(&user.id)?;
    let user = ctx
        .store
        .get_message(&user.id)?
        .ok_or_else(|| NexusError::NotFound("message".into()))?;
    let assistant = start_turn(ctx, &conv, &user, spec, think, sink, requester(ctx, caller))?;
    Ok(ChatSendResult {
        conversation_id: conv.id,
        user_message_id: user.id,
        assistant_message_id: assistant,
    })
}

/// The paired device's name when a remote started this chat (so permission prompts say who is asking).
fn requester(ctx: &Ctx, caller: &Caller) -> Option<String> {
    match caller {
        Caller::Desktop => None,
        Caller::Remote { device_id, .. } => ctx
            .store
            .share_device(device_id)
            .ok()
            .flatten()
            .map(|d| d.name)
            .or_else(|| Some("a paired device".into())),
    }
}

/// Insert the assistant placeholder and spawn the turn. `user` must be the last message.
fn start_turn(
    ctx: &Ctx,
    conv: &Conversation,
    user: &Message,
    spec: nexus_agent::AssistantSpec,
    think: bool,
    sink: Arc<dyn Sink<ChatEvent>>,
    requested_by: Option<String>,
) -> Result<MessageId, NexusError> {
    // Duo (v0.1): pick the light or strong model for this message; the swap happens in the turn task.
    let attach_tokens: u32 = user
        .attachments
        .iter()
        .filter_map(|a| a.snapshot.as_deref())
        .map(nexus_agent::estimate_tokens)
        .sum();
    let duo_model = super::runtime::duo_model_for(ctx, spec.id.as_str(), &user.content, attach_tokens, think);
    let model_id = duo_model.clone().or_else(|| match ctx.supervisor.state() {
        RuntimeState::Ready { model_id, .. } => Some(model_id),
        _ => None,
    });
    let placeholder =
        ctx.store
            .insert_assistant_placeholder(&conv.id, &spec_id(&spec), model_id.as_deref())?;
    let cancel = ctx.chats.register(&placeholder.id);

    // History: finished messages before the question.
    let history: Vec<HistoryMessage> = ctx
        .store
        .list_messages(&conv.id)?
        .into_iter()
        .take_while(|m| m.id != user.id)
        .filter(|m| matches!(m.role, MessageRole::User | MessageRole::Assistant))
        .filter(|m| {
            matches!(m.status, MessageStatus::Done | MessageStatus::Stopped) && !m.content.trim().is_empty()
        })
        .map(|m| HistoryMessage {
            id: m.id,
            role: m.role,
            content: m.content,
        })
        .collect();

    // "Save that" needs the last answer; the router reads it before the history moves into the turn.
    let previous_answer = history
        .iter()
        .rev()
        .find(|m| m.role == MessageRole::Assistant)
        .map(|m| m.content.clone())
        .unwrap_or_default();

    let settings = ctx.settings_snapshot();
    let mut pctx = PromptContext::new(prompt::os_display_name(std::env::consts::OS), prompt::today_iso());
    pctx.granted_roots = ctx.perms.grants().into_iter().map(|g| g.root).collect();
    let tool_names = router_tools(&spec, &settings);
    pctx.tools = builtin_tools()
        .iter()
        .filter(|t| tool_names.iter().any(|n| n == t.name()))
        .map(|t| prompt::PromptTool {
            name: t.name().into(),
            signature: t.signature().into(),
            description: t.description().into(),
        })
        .collect();
    let system_prompt = render_system_prompt(&spec, &pctx).unwrap_or_else(|e| {
        tracing::warn!("prompt rendering failed, using the raw prompt: {e}");
        spec.prompt.system.clone()
    });
    let system_prompt = format!("{system_prompt}{}", tools_note(&spec, &tool_names, &settings));
    let system_prompt = with_model_hint(&ctx.store, &settings, model_id.as_deref(), system_prompt);
    let docs = attached_docs(&user.attachments);
    let is_pipeline = spec.workflow.kind == "pipeline";
    let pipeline_attachments = user.attachments.clone();
    // `/nano-nexus` help questions are answered from the attached report; the model must not go looking for files.
    let help_question = user
        .attachments
        .iter()
        .any(|a| a.label == "NanoNexus diagnostics");
    let routed = spec.workflow.kind == "router" && !tool_names.is_empty() && !help_question;
    let (max_hops, always_route) = (
        spec.workflow.max_hops.unwrap_or(3),
        spec.workflow.always_route.unwrap_or(false),
    );
    let assistant_slug = spec.id.clone();
    let mut input = TurnInput {
        assistant: spec,
        system_prompt,
        history,
        summary: conv.summary.clone().map(|text| StoredSummary {
            text,
            upto: conv.summary_upto.clone(),
        }),
        user_text: user.content.clone(),
        attachments: user
            .attachments
            .iter()
            .filter_map(|a| {
                a.snapshot.clone().map(|text| AttachmentBlock {
                    label: a.label.clone(),
                    path: a.path.clone(),
                    text,
                })
            })
            .collect(),
        think,
        ctx_size: settings.runtime_ctx_size,
    };

    // Knowledge libraries (M17): only touched when a tool asks for them or the setting is on, and only
    // when an enabled, indexed library exists. Otherwise nothing below changes the turn.
    let kb_tool = tool_names.iter().any(|n| n == "kb_search");
    let kb_inject = settings.knowledge_chat_context && !is_pipeline && !kb_tool;
    let kb_tokens = settings.knowledge_max_tokens.clamp(200, 4000);
    let ctx = Arc::clone(ctx);
    let assistant_id = placeholder.id.clone();
    let conv_id = conv.id.clone();
    let observe_model = model_id.clone().unwrap_or_else(|| "model".into());
    let attachments = user.attachments.clone();
    tokio::spawn(async move {
        // engine -> persist (raw, ordered) -> coalesce (16 ms / 32 chars) -> UI
        let coalescing = Arc::new(super::coalesce::CoalescingSink::new(sink.clone()));
        let persisting = PersistingSink::new(
            coalescing.clone(),
            ctx.store.clone(),
            assistant_id.clone(),
            PERSIST_EVERY,
        )
        .with_observe(ctx.clone(), conv_id.to_string());
        super::observe::chat_started(&ctx, conv_id.as_str(), &assistant_slug, &observe_model);
        persisting.send(ChatEvent::Started {
            assistant_message_id: assistant_id.clone(),
        });
        if let Some(id) = &duo_model {
            let already =
                matches!(ctx.supervisor.state(), RuntimeState::Ready { model_id, .. } if &model_id == id);
            if !already {
                persisting.send(ChatEvent::Status {
                    text: "Duo: switching model…".into(),
                });
                super::runtime::ensure_model(&ctx, id).await;
            }
        }
        let mut early_text: Option<String> = None;
        let mut check_facts: Option<String> = None;
        if is_pipeline {
            match super::pipelines::run_pipeline(
                &ctx,
                &input.assistant,
                &input.user_text,
                &pipeline_attachments,
                &persisting,
                &cancel,
                requested_by.clone(),
            )
            .await
            {
                Ok(Some((out, target))) => {
                    super::pipelines::store_artifact(&ctx, &out.artifact, &target, &persisting);
                    for n in &out.notes {
                        persisting.send(ChatEvent::Status { text: n.clone() });
                    }
                    if let Some(plan) = &out.stream {
                        check_facts = Some(plan.facts.clone());
                        input = super::pipelines::plan_input(&input.assistant, plan, input.ctx_size);
                    } else {
                        early_text = Some(out.text.unwrap_or_default());
                    }
                }
                Ok(None) => {}
                Err(nexus_agent::pipelines::PipelineError::Cancelled) => early_text = Some(String::new()),
                Err(e) => early_text = Some(e.to_string()),
            }
        }
        let kb_source = if kb_tool || kb_inject {
            super::knowledge::AppKnowledge::if_enabled(&ctx)
        } else {
            None
        };
        if kb_inject
            && early_text.is_none()
            && let Some(kb) = &kb_source
            && nexus_knowledge::prompt::looks_like_lookup(&input.user_text)
            && let Some(block) = kb.context_for(
                &input.user_text,
                kb_tokens,
                nexus_knowledge::AnswerStyle::from_setting(&settings.knowledge_answer_style),
            )
        {
            input.attachments.push(AttachmentBlock {
                label: "Libraries".into(),
                path: None,
                text: block,
            });
        }
        if routed && early_text.is_none() {
            let gate = EngineGatekeeper::new(
                ctx.perms.clone(),
                assistant_slug.clone(),
                Some(Arc::new(super::permissions::BrokerAsker {
                    ctx: ctx.clone(),
                    requested_by: requested_by.clone(),
                })),
            );
            let mut cx = ToolCx::new(Arc::new(gate));
            cx.docs = docs;
            cx.proposals = Some(Arc::new(ChatProposals {
                ctx: ctx.clone(),
                origin: assistant_slug.clone(),
            }));
            if settings.tools_web_enabled {
                cx.web = Some(Arc::new(ChatWeb(nexus_models::web::WebClient::new())));
            }
            cx.kb = kb_source.clone().map(|k| k as Arc<dyn KnowledgeSource>);
            cx.consultations = Some(Arc::new(super::consultations::AppConsultations(ctx.clone())));
            let request = RouterRequest {
                user_text: &input.user_text,
                allowed_tools: tool_names,
                max_hops,
                always_route,
                home: home_dir(),
                previous_answer,
            };
            let llm = SchedulerRouter(Arc::new(ctx.scheduler.clone()));
            let routed = run_router(request, &llm, &builtin_tools(), &cx, &persisting, cancel.clone()).await;
            if routed.stopped == "incomplete" {
                let reason = routed
                    .runs
                    .last()
                    .map(|r| r.summary.as_str())
                    .unwrap_or("The model stopped before choosing a file-changing tool.");
                early_text = Some(format!(
                    "No file change was proposed or applied. The assistant stopped before completing the requested change. Last step: {reason}."
                ));
            }
            if early_text.is_none() {
                early_text = nexus_agent::receipt::tool_receipt(&input.user_text, &routed.runs);
            }
            if !routed.runs.is_empty() {
                input.system_prompt.push_str("\nThe tool steps below have already finished. Report the observed results and any required Apply action. Do not print tool calls or promise future tool execution.");
            }
            input.attachments.extend(routed.as_attachments());
        }
        let mut outcome = match early_text {
            Some(text) => {
                if !text.is_empty() {
                    persisting.send(ChatEvent::Content { delta: text.clone() });
                }
                persisting.send(ChatEvent::Done {
                    finish_reason: "stop".into(),
                    tokens_out: 0,
                    tps: 0.0,
                    ttft_ms: 0,
                });
                nexus_agent::TurnOutcome {
                    status: MessageStatus::Done,
                    content: text,
                    reasoning: None,
                    finish_reason: Some("stop".into()),
                    tokens_prompt: None,
                    tokens_out: None,
                    tps: None,
                    ttft_ms: None,
                    error: None,
                    dropped: Vec::new(),
                    needs_summary: false,
                    attachment_tokens: Vec::new(),
                }
            }
            None => ctx.engine.run(input, &persisting, cancel).await,
        };
        // Every number in a pipeline's narrative must come from its facts; say so when one does not.
        if let Some(facts) = check_facts.filter(|_| outcome.status == MessageStatus::Done) {
            let bad = nexus_agent::pipelines::unsupported_numbers(&outcome.content, &facts);
            if !bad.is_empty() {
                let note = format!(
                    "\n\n_Note: I could not verify these numbers against the scan: {}._",
                    bad.join(", ")
                );
                persisting.send(ChatEvent::Content { delta: note.clone() });
                outcome.content.push_str(&note);
            }
        }
        // Verified citations: invented `[n]` markers are dropped and the real ones get a Sources list.
        if let Some(kb) = kb_source
            .as_ref()
            .filter(|_| outcome.status == MessageStatus::Done)
        {
            let verified = kb.finalize_answer(&outcome.content);
            if let Some(tail) = verified.strip_prefix(outcome.content.as_str())
                && !tail.is_empty()
            {
                persisting.send(ChatEvent::Content {
                    delta: tail.to_owned(),
                });
            }
            outcome.content = verified;
        }
        coalescing.flush();

        let mut fin = MessageFinish::new(outcome.status, outcome.content.clone());
        fin.reasoning = outcome.reasoning.clone();
        fin.tool_trace = persisting.tool_trace();
        fin.error = outcome.error.as_ref().map(|(_, m)| m.clone());
        fin.tokens_prompt = outcome.tokens_prompt;
        fin.tokens_out = outcome.tokens_out;
        fin.tps = outcome.tps;
        fin.ttft_ms = outcome.ttft_ms;
        if let Err(e) = ctx.store.finish_message(&assistant_id, &fin) {
            tracing::error!("could not save the finished answer: {e}");
        }
        for (a, tokens) in attachments.iter().zip(outcome.attachment_tokens.iter()) {
            let _ = ctx.store.set_attachment_tokens(&a.id, *tokens);
        }
        ctx.chats.finish(&assistant_id);
        super::observe::chat_finished(
            &ctx,
            conv_id.as_str(),
            outcome.tokens_prompt.unwrap_or(0),
            outcome.tokens_out.unwrap_or(0),
            outcome.status != MessageStatus::Error,
        );
        if outcome.status == MessageStatus::Done {
            after_turn(&ctx, &conv_id, &outcome);
        }
    });
    Ok(placeholder.id)
}

/// Background follow-ups of a finished answer: a title after the first exchange, and a rolling
/// summary when older messages no longer fit the context.
fn after_turn(ctx: &Ctx, conv_id: &ConversationId, outcome: &nexus_agent::TurnOutcome) {
    maybe_title(ctx, conv_id);
    if outcome.needs_summary && !outcome.dropped.is_empty() {
        maybe_summarize(ctx, conv_id, outcome.dropped.clone());
    }
}

fn maybe_title(ctx: &Ctx, conv_id: &ConversationId) {
    let Ok(Some(conv)) = ctx.store.get_conversation(conv_id) else {
        return;
    };
    if conv.title != "New chat" {
        return;
    }
    let Ok(msgs) = ctx.store.list_messages(conv_id) else {
        return;
    };
    let user = msgs.iter().find(|m| m.role == MessageRole::User);
    let answer = msgs
        .iter()
        .find(|m| m.role == MessageRole::Assistant && m.status == MessageStatus::Done);
    let (Some(user), Some(answer)) = (user, answer) else {
        return;
    };
    // Only the first exchange names the conversation.
    if msgs.iter().filter(|m| m.role == MessageRole::User).count() != 1 {
        return;
    }
    let (user_text, answer_text) = (user.content.clone(), answer.content.clone());
    let (ctx, conv_id) = (Arc::clone(ctx), conv_id.clone());
    tokio::spawn(async move {
        let title = nexus_agent::generate_title(
            &ctx.scheduler,
            &user_text,
            &answer_text,
            tokio_util::sync::CancellationToken::new(),
        )
        .await;
        let patch = nexus_store::ConversationPatch {
            title: Some(title),
            ..Default::default()
        };
        // The user may have renamed it meanwhile; only replace the placeholder title.
        if let Ok(Some(c)) = ctx.store.get_conversation(&conv_id)
            && c.title == "New chat"
            && ctx.store.update_conversation(&conv_id, &patch).is_ok()
        {
            let _ = ctx.bus.send(crate::state::AppEvent::ConversationUpdated(conv_id));
        }
    });
}

fn maybe_summarize(ctx: &Ctx, conv_id: &ConversationId, dropped: Vec<MessageId>) {
    let first_in_flight = ctx
        .summarizing
        .lock()
        .map(|mut s| s.insert(conv_id.clone()))
        .unwrap_or(false);
    if !first_in_flight {
        return;
    }
    let (ctx, conv_id) = (Arc::clone(ctx), conv_id.clone());
    tokio::spawn(async move {
        let result = async {
            let previous = ctx
                .store
                .get_conversation(&conv_id)
                .ok()
                .flatten()
                .and_then(|c| c.summary);
            let mut turns = Vec::new();
            for id in &dropped {
                if let Ok(Some(m)) = ctx.store.get_message(id)
                    && matches!(m.role, MessageRole::User | MessageRole::Assistant)
                {
                    turns.push(nexus_agent::SummaryTurn {
                        role: m.role.as_str().to_owned(),
                        content: m.content,
                    });
                }
            }
            let text = nexus_agent::summarize_history(
                &ctx.scheduler,
                previous.as_deref(),
                &turns,
                tokio_util::sync::CancellationToken::new(),
            )
            .await
            .ok()?;
            let upto = dropped.last()?;
            ctx.store.set_conversation_summary(&conv_id, &text, upto).ok()?;
            Some(())
        }
        .await;
        if result.is_some() {
            let _ = ctx
                .bus
                .send(crate::state::AppEvent::ConversationUpdated(conv_id.clone()));
        }
        if let Ok(mut s) = ctx.summarizing.lock() {
            s.remove(&conv_id);
        }
    });
}

fn spec_id(spec: &nexus_agent::AssistantSpec) -> AssistantId {
    AssistantId(spec.id.clone())
}

/// Append the catalog's short hint for the running model (if it has one and hints are on). The hint is
/// plain guidance such as "answer in the user's language"; it never changes tools, sampling or permissions.
fn with_model_hint(
    store: &nexus_store::Store,
    settings: &nexus_core::settings::Settings,
    model_id: Option<&str>,
    system_prompt: String,
) -> String {
    if !settings.models_use_prompt_hints {
        return system_prompt;
    }
    let Some(id) = model_id else { return system_prompt };
    let catalog_id = store
        .get_model(id)
        .ok()
        .flatten()
        .and_then(|m| m.catalog_id)
        .unwrap_or_else(|| id.to_owned());
    match nexus_models::catalog::Catalog::embedded()
        .get(&catalog_id)
        .and_then(|e| e.prompt_hint.as_deref())
    {
        Some(hint) if !hint.trim().is_empty() => {
            format!("{system_prompt}\n\n## Notes for this model\n{}", hint.trim())
        }
        _ => system_prompt,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::future::BoxFuture;
    use nexus_agent::{TurnOutcome, TurnRunner};
    use nexus_core::chat::AttachmentKind;
    use nexus_core::sink::VecSink;
    use std::collections::VecDeque;
    use tokio_util::sync::CancellationToken;

    use crate::state::AppState;

    /// Scripted runner: each call pops one reply (events to emit + the text of the outcome).
    struct Fake {
        replies: Mutex<VecDeque<String>>,
        inputs: Mutex<Vec<TurnInput>>,
        hang: bool,
    }

    impl Fake {
        fn new(replies: &[&str]) -> Arc<Self> {
            Arc::new(Self {
                replies: Mutex::new(replies.iter().map(|s| (*s).to_owned()).collect()),
                inputs: Mutex::new(Vec::new()),
                hang: false,
            })
        }
        fn hanging() -> Arc<Self> {
            Arc::new(Self {
                replies: Mutex::new(VecDeque::new()),
                inputs: Mutex::new(Vec::new()),
                hang: true,
            })
        }
    }

    fn outcome(status: MessageStatus, content: &str) -> TurnOutcome {
        TurnOutcome {
            status,
            content: content.into(),
            reasoning: Some("thoughts".into()),
            finish_reason: Some("stop".into()),
            tokens_prompt: Some(10),
            tokens_out: Some(5),
            tps: Some(40.0),
            ttft_ms: Some(100),
            error: None,
            dropped: vec![],
            needs_summary: false,
            attachment_tokens: vec![7],
        }
    }

    impl TurnRunner for Fake {
        fn run<'a>(
            &'a self,
            input: TurnInput,
            sink: &'a dyn Sink<ChatEvent>,
            cancel: CancellationToken,
        ) -> BoxFuture<'a, TurnOutcome> {
            Box::pin(async move {
                self.inputs.lock().unwrap().push(input);
                if self.hang {
                    sink.send(ChatEvent::Content {
                        delta: "partial".into(),
                    });
                    cancel.cancelled().await;
                    sink.send(ChatEvent::Done {
                        finish_reason: "stopped".into(),
                        tokens_out: 0,
                        tps: 0.0,
                        ttft_ms: 0,
                    });
                    return outcome(MessageStatus::Stopped, "partial");
                }
                let text = self
                    .replies
                    .lock()
                    .unwrap()
                    .pop_front()
                    .unwrap_or_else(|| "ok".into());
                sink.send(ChatEvent::Reasoning {
                    delta: "thoughts".into(),
                });
                for word in text.split_inclusive(' ') {
                    sink.send(ChatEvent::Content {
                        delta: word.to_owned(),
                    });
                }
                sink.send(ChatEvent::Done {
                    finish_reason: "stop".into(),
                    tokens_out: 5,
                    tps: 40.0,
                    ttft_ms: 100,
                });
                outcome(MessageStatus::Done, &text)
            })
        }
    }

    fn args(text: &str, conv: Option<ConversationId>) -> ChatSendArgs {
        ChatSendArgs {
            conversation_id: conv,
            assistant_id: AssistantId::from("nexus"),
            text: text.into(),
            attachments: vec![],
            draft_spec: None,
            think: false,
        }
    }

    async fn wait_idle(ctx: &Ctx, id: &MessageId) {
        for _ in 0..200 {
            if !ctx.chats.is_running(id) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("turn never finished");
    }

    async fn live_turn(ctx: &Ctx, assistant: &str, text: String) -> Result<Vec<ChatEvent>, String> {
        let sink = VecSink::<ChatEvent>::default();
        let mut request = args(&text, None);
        request.assistant_id = AssistantId::from(assistant);
        let sent = send(ctx, &Caller::Desktop, request, Arc::new(sink.clone())).map_err(|e| e.to_string())?;
        let wait = async {
            while ctx.chats.is_running(&sent.assistant_message_id) {
                for pending in ctx.broker.open_requests() {
                    super::super::permissions::respond(
                        ctx,
                        &pending.id,
                        super::super::permissions::Decision::Deny,
                    )
                    .map_err(|e| e.to_string())?;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            Ok::<(), String>(())
        };
        let failure = match tokio::time::timeout(Duration::from_secs(180), wait).await {
            Ok(result) => result.err(),
            Err(_) => Some(format!("{assistant} turn timed out")),
        };
        if let Some(error) = failure {
            stop(ctx, &sent.assistant_message_id);
            return Err(error);
        }
        let messages = ctx
            .store
            .list_messages(&sent.conversation_id)
            .map_err(|e| e.to_string())?;
        let answer = messages.last().ok_or("missing assistant message")?;
        println!(
            "{assistant}: status={:?}, trace={:?}, answer={}",
            answer.status, answer.tool_trace, answer.content
        );
        if answer.status != MessageStatus::Done {
            return Err(format!("{assistant} did not finish: {:?}", answer.error));
        }
        let events = sink.events();
        let factual_action = events.iter().any(|event| {
            matches!(event,
            ChatEvent::ToolResult { tool, ok: true, .. }
                if matches!(tool.as_str(), "write_file" | "edit_file" | "calculate" | "now"))
        });
        if factual_action && (answer.content.contains("<function") || answer.content.contains("<tool_result"))
        {
            return Err("simple action answer leaked internal tool markup".into());
        }
        Ok(events)
    }

    #[tokio::test]
    #[ignore = "needs NEXUS_MODEL and the real local runtime; models are referenced in place"]
    async fn real_model_agent_and_duo_apply_files_and_consult_a_specialist() {
        use nexus_core::fs::{GrantLevel, GrantScope};
        let dir = tempfile::tempdir().unwrap();
        let work = dir.path().join("work");
        std::fs::create_dir(&work).unwrap();
        let work = dunce::canonicalize(work).unwrap();
        let ctx = AppState::for_tests(dir.path());
        ctx.perms
            .add_grant(nexus_fs::NewGrant {
                root: work.clone(),
                level: GrantLevel::Full,
                recursive: true,
                scope: GrantScope::Session,
                expires_at: None,
                label: None,
            })
            .unwrap();
        super::super::settings::set(
            &ctx,
            &Caller::Desktop,
            "chat.auto_apply_proposals",
            super::super::settings::SettingValue::Bool(true),
        )
        .unwrap();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let model = std::env::var_os("NEXUS_MODEL").expect("set NEXUS_MODEL to the existing GGUF");
        let binary = std::env::var_os("NEXUS_LLAMA_BIN")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| root.join("vendor/runtime/macos-arm64/llama-server-aarch64-apple-darwin"));
        let mut config = nexus_llm::RuntimeConfig::new(binary, model);
        config.model_id = "real-eval".into();
        ctx.supervisor.start(config).await.unwrap();
        let result: Result<(), String> = async {
            let target = work.join("created.txt");
            live_turn(&ctx, "agent", format!(
                "Create {} containing exactly alpha followed by one newline. Use the file tool; report briefly.", target.display()
            )).await?;
            if std::fs::read_to_string(&target).map_err(|e| e.to_string())? != "alpha\n" {
                return Err("Agent created incorrect content".into());
            }
            live_turn(&ctx, "agent", format!(
                "Read {} and change alpha to bravo using edit_file. Keep the newline. Report briefly.", target.display()
            )).await?;
            if std::fs::read_to_string(&target).map_err(|e| e.to_string())? != "bravo\n" {
                return Err("Agent edit did not reach disk".into());
            }
            let events = live_turn(&ctx, "agent", "Ask Reviewer to review this supplied Python code: def average(xs): return sum(xs)/len(xs). The requirement is to return None for empty input. Consult the reviewer specialist and summarize its finding briefly.".into()).await?;
            if !events.iter().any(|e| matches!(e, ChatEvent::ToolResult { tool, ok: true, .. } if tool == "ask_assistant")) {
                return Err("Agent did not consult Reviewer successfully".into());
            }
            let calculation = live_turn(&ctx, "nexus", "What is 18% of 2450?".into()).await?;
            if !calculation.iter().any(|e| matches!(e, ChatEvent::ToolResult { tool, ok: true, .. } if tool == "calculate")) {
                return Err("Nexus did not use calculate for arithmetic".into());
            }
            let answer: String = calculation.iter().filter_map(|e| match e {
                ChatEvent::Content { delta } => Some(delta.as_str()), _ => None,
            }).collect();
            if !answer.contains("441") { return Err(format!("Nexus arithmetic answer was not grounded: {answer}")); }
            let clock = live_turn(&ctx, "nexus", "What is the current date and time in UTC?".into()).await?;
            if !clock.iter().any(|e| matches!(e, ChatEvent::ToolResult { tool, ok: true, .. } if tool == "now")) {
                return Err("Nexus did not use now for the current clock time".into());
            }
            let duo_file = work.join("duo.txt");
            live_turn(&ctx, "duo", format!(
                "Create {} containing exactly duo followed by one newline. Use write_file and report briefly.", duo_file.display()
            )).await?;
            if std::fs::read_to_string(&duo_file).map_err(|e| e.to_string())? != "duo\n" {
                return Err("Duo did not create the requested file".into());
            }
            Ok(())
        }.await;
        ctx.supervisor.shutdown().await;
        assert!(result.is_ok(), "{}", result.unwrap_err());
    }

    #[tokio::test]
    async fn send_persists_streams_and_finishes() {
        let dir = tempfile::tempdir().unwrap();
        let fake = Fake::new(&["Hello there friend"]);
        let ctx = AppState::for_tests_with(dir.path(), Some(fake.clone()));
        let sink = VecSink::<ChatEvent>::default();
        let r = send(&ctx, &Caller::Desktop, args("hi", None), Arc::new(sink.clone())).unwrap();
        wait_idle(&ctx, &r.assistant_message_id).await;

        let events = sink.events();
        assert_eq!(
            events[0],
            ChatEvent::Started {
                assistant_message_id: r.assistant_message_id.clone()
            }
        );
        assert!(matches!(events.last(), Some(ChatEvent::Done { .. })));
        let msgs = ctx.store.list_messages(&r.conversation_id).unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(
            (msgs[0].role, msgs[0].content.as_str()),
            (MessageRole::User, "hi")
        );
        let a = &msgs[1];
        assert_eq!(
            (a.status, a.content.as_str()),
            (MessageStatus::Done, "Hello there friend")
        );
        assert_eq!(a.reasoning.as_deref(), Some("thoughts"));
        assert_eq!((a.tokens_out, a.ttft_ms), (Some(5), Some(100)));
        // The turn input excluded the new question from history and used the Nexus prompt.
        let inputs = fake.inputs.lock().unwrap();
        assert!(inputs[0].history.is_empty());
        assert_eq!(inputs[0].user_text, "hi");
        assert!(inputs[0].system_prompt.starts_with("You are Nexus"));
        assert_eq!(inputs[0].ctx_size, 8192);
    }

    #[tokio::test]
    async fn the_first_exchange_gets_a_title_and_listeners_are_told() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppState::for_tests_with(dir.path(), Some(Fake::new(&["Sure thing", "Another"])));
        let mut rx = ctx.bus.subscribe();
        let sink = VecSink::<ChatEvent>::default();
        let r = send(
            &ctx,
            &Caller::Desktop,
            args("plan my weekly reaper mixing session", None),
            Arc::new(sink.clone()),
        )
        .unwrap();
        let event = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("title event")
            .unwrap();
        assert!(
            matches!(&event, crate::state::AppEvent::ConversationUpdated(id) if *id == r.conversation_id)
        );
        // No model runs in tests, so the deterministic fallback names the chat.
        let conv = ctx.store.get_conversation(&r.conversation_id).unwrap().unwrap();
        assert_eq!(conv.title, "Plan My Weekly Reaper Mixing Session");
        // A second exchange does not rename it.
        wait_idle(&ctx, &r.assistant_message_id).await;
        let r2 = send(
            &ctx,
            &Caller::Desktop,
            args("and more", Some(r.conversation_id.clone())),
            Arc::new(sink),
        )
        .unwrap();
        wait_idle(&ctx, &r2.assistant_message_id).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(
            ctx.store
                .get_conversation(&r.conversation_id)
                .unwrap()
                .unwrap()
                .title,
            "Plan My Weekly Reaper Mixing Session"
        );
    }

    #[tokio::test]
    async fn a_second_message_sees_the_first_exchange_as_history() {
        let dir = tempfile::tempdir().unwrap();
        let fake = Fake::new(&["first answer", "second answer"]);
        let ctx = AppState::for_tests_with(dir.path(), Some(fake.clone()));
        let sink = VecSink::<ChatEvent>::default();
        let r1 = send(&ctx, &Caller::Desktop, args("one", None), Arc::new(sink.clone())).unwrap();
        wait_idle(&ctx, &r1.assistant_message_id).await;
        let r2 = send(
            &ctx,
            &Caller::Desktop,
            args("two", Some(r1.conversation_id.clone())),
            Arc::new(sink),
        )
        .unwrap();
        wait_idle(&ctx, &r2.assistant_message_id).await;
        let inputs = fake.inputs.lock().unwrap();
        let h: Vec<_> = inputs[1].history.iter().map(|m| m.content.as_str()).collect();
        assert_eq!(h, ["one", "first answer"]);
        assert_eq!(r1.conversation_id, r2.conversation_id);
    }

    #[tokio::test]
    async fn stop_keeps_the_partial_answer() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppState::for_tests_with(dir.path(), Some(Fake::hanging()));
        let sink = VecSink::<ChatEvent>::default();
        let r = send(&ctx, &Caller::Desktop, args("long task", None), Arc::new(sink)).unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(stop(&ctx, &r.assistant_message_id));
        wait_idle(&ctx, &r.assistant_message_id).await;
        let m = ctx.store.get_message(&r.assistant_message_id).unwrap().unwrap();
        assert_eq!(
            (m.status, m.content.as_str()),
            (MessageStatus::Stopped, "partial")
        );
        assert!(!stop(&ctx, &r.assistant_message_id), "already finished");
    }

    #[tokio::test]
    async fn regenerate_replaces_the_answer() {
        let dir = tempfile::tempdir().unwrap();
        let fake = Fake::new(&["old answer", "new answer"]);
        let ctx = AppState::for_tests_with(dir.path(), Some(fake.clone()));
        let sink = VecSink::<ChatEvent>::default();
        let r = send(
            &ctx,
            &Caller::Desktop,
            args("question", None),
            Arc::new(sink.clone()),
        )
        .unwrap();
        wait_idle(&ctx, &r.assistant_message_id).await;
        let r2 = regenerate(
            &ctx,
            &Caller::Desktop,
            &r.assistant_message_id,
            false,
            Arc::new(sink),
        )
        .unwrap();
        wait_idle(&ctx, &r2.assistant_message_id).await;
        assert_ne!(r2.assistant_message_id, r.assistant_message_id);
        assert_eq!(r2.user_message_id, r.user_message_id);
        let msgs = ctx.store.list_messages(&r.conversation_id).unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[1].content, "new answer");
        assert!(
            fake.inputs.lock().unwrap()[1].history.is_empty(),
            "old answer must not be in the new history"
        );
        // Regenerating a user message is refused.
        let err = regenerate(
            &ctx,
            &Caller::Desktop,
            &r.user_message_id,
            false,
            Arc::new(VecSink::<ChatEvent>::default()),
        )
        .unwrap_err();
        assert_eq!(err.code(), nexus_core::error::ErrorCode::InvalidInput);
    }

    #[tokio::test]
    async fn edit_resend_rewrites_and_drops_later_messages() {
        let dir = tempfile::tempdir().unwrap();
        let fake = Fake::new(&["a1", "a2", "a1-edited"]);
        let ctx = AppState::for_tests_with(dir.path(), Some(fake.clone()));
        let sink = VecSink::<ChatEvent>::default();
        let r1 = send(
            &ctx,
            &Caller::Desktop,
            args("first", None),
            Arc::new(sink.clone()),
        )
        .unwrap();
        wait_idle(&ctx, &r1.assistant_message_id).await;
        let r2 = send(
            &ctx,
            &Caller::Desktop,
            args("second", Some(r1.conversation_id.clone())),
            Arc::new(sink.clone()),
        )
        .unwrap();
        wait_idle(&ctx, &r2.assistant_message_id).await;
        let r3 = edit_resend(
            &ctx,
            &Caller::Desktop,
            &r1.user_message_id,
            "first, but better",
            false,
            Arc::new(sink),
        )
        .unwrap();
        wait_idle(&ctx, &r3.assistant_message_id).await;
        let msgs = ctx.store.list_messages(&r1.conversation_id).unwrap();
        assert_eq!(
            msgs.iter().map(|m| m.content.as_str()).collect::<Vec<_>>(),
            ["first, but better", "a1-edited"]
        );
        assert_eq!(fake.inputs.lock().unwrap()[2].user_text, "first, but better");
    }

    #[tokio::test]
    async fn validation_errors() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppState::for_tests_with(dir.path(), Some(Fake::new(&[])));
        let sink: Arc<dyn Sink<ChatEvent>> = Arc::new(VecSink::<ChatEvent>::default());
        let code = |r: Result<ChatSendResult, NexusError>| r.unwrap_err().code();
        use nexus_core::error::ErrorCode::*;
        assert_eq!(
            code(send(&ctx, &Caller::Desktop, args("   ", None), sink.clone())),
            InvalidInput
        );
        let mut a = args("hi", None);
        a.assistant_id = AssistantId::from("nobody");
        assert_eq!(code(send(&ctx, &Caller::Desktop, a, sink.clone())), NotFound);
        assert_eq!(
            code(send(
                &ctx,
                &Caller::Desktop,
                args("hi", Some(ConversationId::from("missing"))),
                sink.clone()
            )),
            NotFound
        );
        // An attachment alone is a valid message.
        let mut with_file = args("", None);
        with_file.attachments = vec![AttachmentInput {
            kind: AttachmentKind::Text,
            path: None,
            label: "n".into(),
            text: Some("body".into()),
        }];
        let r = send(&ctx, &Caller::Desktop, with_file, sink).unwrap();
        wait_idle(&ctx, &r.assistant_message_id).await;
        let user = ctx.store.get_message(&r.user_message_id).unwrap().unwrap();
        assert_eq!(
            user.attachments[0].tokens,
            Some(7),
            "attachment token cost is recorded"
        );
    }

    #[tokio::test]
    async fn persisting_sink_flushes_partial_text_periodically() {
        let store = Arc::new(Store::open_in_memory().unwrap());
        let conv = store.create_conversation(&AssistantId::from("nexus")).unwrap();
        let m = store
            .insert_assistant_placeholder(&conv.id, &AssistantId::from("nexus"), None)
            .unwrap();
        let inner = VecSink::<ChatEvent>::default();
        let sink = PersistingSink::new(
            Arc::new(inner.clone()),
            store.clone(),
            m.id.clone(),
            Duration::from_millis(20),
        );
        sink.send(ChatEvent::Content { delta: "one ".into() });
        assert_eq!(
            store.get_message(&m.id).unwrap().unwrap().content,
            "",
            "not flushed yet"
        );
        tokio::time::sleep(Duration::from_millis(30)).await;
        sink.send(ChatEvent::Reasoning { delta: "why".into() });
        let saved = store.get_message(&m.id).unwrap().unwrap();
        assert_eq!(
            (saved.content.as_str(), saved.reasoning.as_deref()),
            ("one ", Some("why")),
            "flush after the interval"
        );
        sink.send(ChatEvent::Content { delta: "two".into() });
        assert_eq!(
            store.get_message(&m.id).unwrap().unwrap().content,
            "one ",
            "within the interval: not rewritten"
        );
        tokio::time::sleep(Duration::from_millis(30)).await;
        sink.send(ChatEvent::Content { delta: "!".into() });
        assert_eq!(store.get_message(&m.id).unwrap().unwrap().content, "one two!");
        assert_eq!(inner.events().len(), 4, "every event is forwarded");
    }

    #[tokio::test]
    async fn the_sink_records_a_tool_trace() {
        let store = Arc::new(Store::open_in_memory().unwrap());
        let conv = store.create_conversation(&AssistantId::from("nexus")).unwrap();
        let m = store
            .insert_assistant_placeholder(&conv.id, &AssistantId::from("nexus"), None)
            .unwrap();
        let sink = PersistingSink::new(
            Arc::new(VecSink::<ChatEvent>::default()),
            store,
            m.id,
            Duration::from_secs(5),
        );
        assert_eq!(sink.tool_trace(), None);
        sink.send(ChatEvent::ToolCall {
            tool: "read_file".into(),
            args_json: "{\"path\":\"/a/b.md\"}".into(),
        });
        sink.send(ChatEvent::ToolResult {
            tool: "read_file".into(),
            summary: "Read b.md".into(),
            ok: true,
        });
        let v: serde_json::Value = serde_json::from_str(&sink.tool_trace().unwrap()).unwrap();
        assert_eq!(v[0]["tool"], "read_file");
        assert_eq!(v[0]["args"]["path"], "/a/b.md");
        assert_eq!(
            (v[0]["summary"].as_str(), v[0]["ok"].as_bool()),
            (Some("Read b.md"), Some(true))
        );
    }
}

#[cfg(test)]
mod tool_filter_tests {
    use super::*;
    use nexus_core::settings::Settings;

    fn agent() -> nexus_agent::AssistantSpec {
        let (_, text) = nexus_agent::registry::builtin_sources()
            .into_iter()
            .find(|(f, _)| *f == "agent.toml")
            .expect("agent.toml is built in");
        nexus_agent::registry::validate_toml(text).expect("agent.toml is valid")
    }

    #[test]
    fn web_tools_need_the_setting_and_the_user_can_switch_tools_off() {
        let spec = agent();
        let mut s = Settings::default();
        let tools = router_tools(&spec, &s);
        assert!(tools.contains(&"write_file".to_owned()));
        assert!(!tools.contains(&"search_web".to_owned()) && !tools.contains(&"fetch_url".to_owned()));

        s.tools_web_enabled = true;
        assert!(router_tools(&spec, &s).contains(&"search_web".to_owned()));

        s.tools_disabled
            .insert("agent".into(), vec!["trash_file".into(), "search_web".into()]);
        let tools = router_tools(&spec, &s);
        assert!(!tools.contains(&"trash_file".to_owned()) && !tools.contains(&"search_web".to_owned()));
        assert!(tools.contains(&"fetch_url".to_owned()));
    }
}

#[cfg(test)]
mod hint_tests {
    use super::*;
    use nexus_core::settings::Settings;
    use nexus_store::{ModelRow, Store};

    fn row(id: &str, catalog_id: Option<&str>) -> ModelRow {
        ModelRow {
            id: id.into(),
            source: "download".into(),
            name: id.into(),
            repo: None,
            revision: None,
            file: None,
            path: "/x.gguf".into(),
            sha256: None,
            bytes: 1,
            quant: None,
            catalog_id: catalog_id.map(str::to_owned),
            gguf_meta: None,
            license: None,
            extra: None,
            added_at: 1,
        }
    }

    #[test]
    fn the_default_model_gets_its_hint_others_do_not_and_the_setting_turns_it_off() {
        let store = Store::open_in_memory().unwrap();
        store
            .upsert_model(&row("bundled", Some("minicpm5-1b-fable5-v2-q8")))
            .unwrap();
        store.upsert_model(&row("other", Some("qwen3-1.7b-q4"))).unwrap();
        let on = Settings::default();
        assert!(on.models_use_prompt_hints);
        let with = with_model_hint(&store, &on, Some("bundled"), "SYSTEM".into());
        assert!(with.starts_with("SYSTEM\n\n## Notes for this model\n"));
        assert!(with.contains("same language as the user"));
        // Same result when the row id is the catalog id itself.
        assert_eq!(
            with_model_hint(&store, &on, Some("minicpm5-1b-fable5-v2-q8"), "SYSTEM".into()),
            with
        );
        assert_eq!(
            with_model_hint(&store, &on, Some("other"), "SYSTEM".into()),
            "SYSTEM"
        );
        assert_eq!(with_model_hint(&store, &on, None, "SYSTEM".into()), "SYSTEM");
        let off = Settings {
            models_use_prompt_hints: false,
            ..Settings::default()
        };
        assert_eq!(
            with_model_hint(&store, &off, Some("bundled"), "SYSTEM".into()),
            "SYSTEM"
        );
    }

    #[test]
    fn hints_are_short() {
        for e in &nexus_models::catalog::Catalog::embedded().models {
            if let Some(h) = &e.prompt_hint {
                assert!(
                    h.len() <= 400,
                    "{}: a hint of {} chars costs too many tokens",
                    e.id,
                    h.len()
                );
            }
        }
    }
}
