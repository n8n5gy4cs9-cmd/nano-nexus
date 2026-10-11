//! The chat workflow: build the request (system + summary + history + attachments + question),
//! stream the answer as `ChatEvent`s, and report what happened. Thoughts are shown but never re-sent.

use std::future::Future;
use std::time::Instant;

use futures_util::StreamExt;
use nexus_core::chat::{ChatEvent, MessageRole, MessageStatus};
use nexus_core::error::{ErrorCode, NexusError};
use nexus_core::ids::MessageId;
use nexus_core::sink::Sink;
use nexus_llm::scheduler::JobStream;
use nexus_llm::{ChatMessage, ChatRequest, LlmError, Priority, Scheduler, StreamEvent};
use tokio_util::sync::CancellationToken;

use crate::budget::{HistoryItem, Limits, SummaryInfo, TokenCounter, Tokenize, plan_history};
use crate::spec::AssistantSpec;

/// Something that can run an interactive streaming chat request.
pub trait ChatBackend: Send + Sync {
    fn stream(
        &self,
        req: ChatRequest,
        cancel: CancellationToken,
    ) -> impl Future<Output = Result<JobStream, LlmError>> + Send;
}

impl ChatBackend for Scheduler {
    async fn stream(&self, req: ChatRequest, cancel: CancellationToken) -> Result<JobStream, LlmError> {
        Scheduler::stream(self, Priority::Interactive, req, cancel).await
    }
}

/// A past user or assistant message (finished, with text).
#[derive(Clone, Debug)]
pub struct HistoryMessage {
    pub id: MessageId,
    pub role: MessageRole,
    pub content: String,
}

/// The rolling summary of older history.
#[derive(Clone, Debug)]
pub struct StoredSummary {
    pub text: String,
    /// Last message the summary covers.
    pub upto: Option<MessageId>,
}

/// Text attached to the new message.
#[derive(Clone, Debug)]
pub struct AttachmentBlock {
    pub label: String,
    pub path: Option<String>,
    pub text: String,
}

/// Everything one turn needs.
#[derive(Clone, Debug)]
pub struct TurnInput {
    pub assistant: AssistantSpec,
    /// Preamble + assistant prompt, already rendered.
    pub system_prompt: String,
    /// Finished messages before the new one, oldest first.
    pub history: Vec<HistoryMessage>,
    pub summary: Option<StoredSummary>,
    pub user_text: String,
    pub attachments: Vec<AttachmentBlock>,
    /// "Think harder" is on for this turn.
    pub think: bool,
    pub ctx_size: u32,
}

/// What happened.
#[derive(Clone, Debug)]
pub struct TurnOutcome {
    pub status: MessageStatus,
    pub content: String,
    pub reasoning: Option<String>,
    pub finish_reason: Option<String>,
    pub tokens_prompt: Option<u32>,
    pub tokens_out: Option<u32>,
    pub tps: Option<f32>,
    pub ttft_ms: Option<u32>,
    pub error: Option<(ErrorCode, String)>,
    /// History messages that did not fit.
    pub dropped: Vec<MessageId>,
    /// Dropped messages are not covered by a summary: the caller should enqueue a summary job.
    pub needs_summary: bool,
    /// Token cost of each attachment as sent (same order as the input).
    pub attachment_tokens: Vec<u32>,
}

impl TurnOutcome {
    fn empty(status: MessageStatus) -> Self {
        Self {
            status,
            content: String::new(),
            reasoning: None,
            finish_reason: None,
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
}

/// Runs chat turns.
pub struct TurnEngine<B: ChatBackend, K: Tokenize> {
    backend: B,
    counter: TokenCounter<K>,
}

struct Prepared {
    request: ChatRequest,
    dropped: Vec<MessageId>,
    needs_summary: bool,
    attachment_tokens: Vec<u32>,
}

/// Cut `text` to roughly `keep_tokens` of `total_tokens`, on a char boundary, with a visible note.
fn truncate_with_note(text: &str, total_tokens: u32, keep_tokens: u32) -> String {
    if total_tokens == 0 || keep_tokens >= total_tokens {
        return text.to_owned();
    }
    let chars = text.chars().count();
    let keep_chars = (chars as u64 * keep_tokens as u64 / total_tokens as u64) as usize;
    let cut: String = text.chars().take(keep_chars).collect();
    format!("{cut}\n[… truncated: showing about {keep_tokens} of {total_tokens} tokens]")
}

/// `<file path="…">…</file>`, with a literal closing tag inside the text neutralised.
fn file_block(a: &AttachmentBlock, text: &str) -> String {
    let name = a.path.as_deref().unwrap_or(&a.label);
    let safe = text.replace("</file>", "<\\/file>");
    format!("<file path=\"{}\">\n{}\n</file>", name.replace('"', "'"), safe)
}

impl<B: ChatBackend, K: Tokenize> TurnEngine<B, K> {
    pub fn new(backend: B, tokenizer: K) -> Self {
        Self {
            backend,
            counter: TokenCounter::new(tokenizer),
        }
    }

    /// The token counter (shared with the UI's context meter).
    pub fn counter(&self) -> &TokenCounter<K> {
        &self.counter
    }

    async fn prepare(&self, input: &TurnInput, limits: Limits) -> Prepared {
        let model = &input.assistant.model;
        let system_tokens = self.counter.count(&input.system_prompt).await;
        let user_tokens = self.counter.count(&input.user_text).await;

        // Attachments, within their budget; later ones are cut first.
        let mut remaining = limits.attachments.min(
            input
                .assistant
                .context
                .max_attach_tokens
                .max(limits.attachments.min(1)),
        );
        let mut blocks = Vec::new();
        let mut attachment_tokens = Vec::new();
        for a in &input.attachments {
            let total = self.counter.count(&a.text).await;
            let keep = total.min(remaining);
            let text = truncate_with_note(&a.text, total, keep);
            remaining -= keep;
            attachment_tokens.push(keep);
            blocks.push(file_block(a, &text));
        }
        let attachments_used: u32 = attachment_tokens.iter().sum();

        // History.
        let mut items = Vec::with_capacity(input.history.len());
        for m in &input.history {
            items.push(HistoryItem {
                tokens: self.counter.count(&m.content).await + 4,
            });
        }
        let summary_info = match &input.summary {
            Some(s) => {
                let idx = s
                    .upto
                    .as_ref()
                    .and_then(|id| input.history.iter().position(|m| &m.id == id));
                Some(SummaryInfo {
                    tokens: self.counter.count(&s.text).await,
                    upto: idx.unwrap_or(0),
                })
            }
            None => None,
        };
        let fixed = limits.fixed(system_tokens, attachments_used) + user_tokens;
        let plan = plan_history(
            &limits,
            fixed,
            &items,
            summary_info,
            input.assistant.context.history_turns,
        );

        // Messages.
        let mut system = input.system_prompt.clone();
        if let (true, Some(s)) = (plan.include_summary, &input.summary) {
            system.push_str("\n\n## Earlier in this conversation\n");
            system.push_str(s.text.trim());
        }
        let mut messages = vec![ChatMessage::system(system)];
        for &i in &plan.keep {
            if let Some(m) = input.history.get(i) {
                messages.push(match m.role {
                    MessageRole::Assistant => ChatMessage::assistant(m.content.clone()),
                    _ => ChatMessage::user(m.content.clone()),
                });
            }
        }
        // Documents first, the question last (long-context guidance).
        let question = if blocks.is_empty() {
            input.user_text.clone()
        } else {
            format!("{}\n\n{}", blocks.join("\n\n"), input.user_text)
        };
        messages.push(ChatMessage::user(question));

        let max_tokens = if input.think {
            model.max_tokens.max(limits.answer)
        } else {
            model.max_tokens
        };
        let mut request = ChatRequest::new(messages)
            .streaming()
            .thinking(input.think)
            .temperature(model.temperature)
            .max_tokens(max_tokens);
        request.top_p = model.top_p;
        request.min_p = model.min_p;
        request.top_k = Some(model.top_k);
        request.repeat_penalty = Some(model.repeat_penalty);

        Prepared {
            request,
            dropped: plan
                .dropped
                .iter()
                .filter_map(|&i| input.history.get(i).map(|m| m.id.clone()))
                .collect(),
            needs_summary: plan.needs_summary,
            attachment_tokens,
        }
    }

    /// Run one turn, sending `ChatEvent`s to `sink`. Never panics and never returns an error: failures
    /// are reported in the outcome (and as a `ChatEvent::Error`).
    pub async fn run_turn(
        &self,
        input: TurnInput,
        sink: &dyn Sink<ChatEvent>,
        cancel: CancellationToken,
    ) -> TurnOutcome {
        let mut limits = Limits::for_ctx(input.ctx_size, input.think);
        limits.answer = if input.think {
            input.assistant.model.max_tokens.max(limits.answer)
        } else {
            input.assistant.model.max_tokens
        };

        let mut prepared = self.prepare(&input, limits).await;
        let started = Instant::now();
        let mut stream = match self
            .backend
            .stream(prepared.request.clone(), cancel.clone())
            .await
        {
            Err(LlmError::ContextOverflow { .. }) => {
                // One more try with a tighter budget.
                prepared = self.prepare(&input, limits.tighter()).await;
                self.backend
                    .stream(prepared.request.clone(), cancel.clone())
                    .await
            }
            other => other,
        };
        let mut outcome = TurnOutcome::empty(MessageStatus::Done);
        outcome.dropped = std::mem::take(&mut prepared.dropped);
        outcome.needs_summary = prepared.needs_summary;
        outcome.attachment_tokens = std::mem::take(&mut prepared.attachment_tokens);

        let events = match stream.as_mut() {
            Ok(s) => s,
            Err(e) => return fail(outcome, e_to_code(e), sink),
        };

        let mut content = String::new();
        let mut reasoning = String::new();
        let mut first_token: Option<Instant> = None;
        while let Some(item) = events.next().await {
            match item {
                Ok(StreamEvent::Reasoning(delta)) => {
                    first_token.get_or_insert_with(Instant::now);
                    reasoning.push_str(&delta);
                    if !sink.send(ChatEvent::Reasoning { delta }) {
                        outcome.status = MessageStatus::Stopped;
                        break;
                    }
                }
                Ok(StreamEvent::Content(delta)) => {
                    first_token.get_or_insert_with(Instant::now);
                    content.push_str(&delta);
                    if !sink.send(ChatEvent::Content { delta }) {
                        outcome.status = MessageStatus::Stopped;
                        break;
                    }
                }
                Ok(StreamEvent::Done {
                    finish_reason,
                    usage,
                    timings,
                }) => {
                    outcome.finish_reason = finish_reason;
                    outcome.tokens_prompt = usage
                        .as_ref()
                        .map(|u| u.prompt_tokens)
                        .or(timings.as_ref().map(|t| t.prompt_n));
                    outcome.tokens_out = usage
                        .as_ref()
                        .map(|u| u.completion_tokens)
                        .or(timings.as_ref().map(|t| t.predicted_n));
                    outcome.tps = timings
                        .as_ref()
                        .map(|t| t.predicted_per_second as f32)
                        .filter(|v| *v > 0.0);
                    break;
                }
                Err(LlmError::Cancelled) => {
                    outcome.status = MessageStatus::Stopped;
                    break;
                }
                Err(e) => {
                    outcome.content = content;
                    outcome.reasoning = (!reasoning.is_empty()).then_some(reasoning);
                    return fail(outcome, e_to_code(&e), sink);
                }
            }
        }
        if cancel.is_cancelled() {
            outcome.status = MessageStatus::Stopped;
        }

        outcome.ttft_ms = first_token.map(|t| t.duration_since(started).as_millis() as u32);
        if outcome.tps.is_none()
            && let (Some(t0), Some(n)) = (first_token, outcome.tokens_out)
        {
            let secs = t0.elapsed().as_secs_f32();
            if secs > 0.0 {
                outcome.tps = Some(n as f32 / secs);
            }
        }
        outcome.content = content;
        outcome.reasoning = (!reasoning.is_empty()).then_some(reasoning);
        let finish = match outcome.status {
            MessageStatus::Stopped => "stopped".to_owned(),
            _ => outcome.finish_reason.clone().unwrap_or_else(|| "stop".to_owned()),
        };
        sink.send(ChatEvent::Done {
            finish_reason: finish,
            tokens_out: outcome.tokens_out.unwrap_or(0),
            tps: outcome.tps.unwrap_or(0.0),
            ttft_ms: outcome.ttft_ms.unwrap_or(0),
        });
        outcome
    }
}

fn e_to_code(e: &LlmError) -> (ErrorCode, String) {
    let msg = e.to_string();
    let nexus: NexusError = match e {
        LlmError::RuntimeUnavailable => NexusError::RuntimeUnavailable(msg.clone()),
        LlmError::Busy => NexusError::Busy(msg.clone()),
        LlmError::Cancelled => NexusError::Cancelled,
        LlmError::ContextOverflow { need, have } => {
            NexusError::ContextOverflow(format!("need {need}, have {have}"))
        }
        LlmError::Protocol(m) => NexusError::LlmInvalidOutput(m.clone()),
        LlmError::Http { .. } | LlmError::Transport(_) => NexusError::RuntimeUnavailable(msg.clone()),
    };
    (nexus.code(), msg)
}

fn fail(
    mut outcome: TurnOutcome,
    (code, message): (ErrorCode, String),
    sink: &dyn Sink<ChatEvent>,
) -> TurnOutcome {
    outcome.status = MessageStatus::Error;
    outcome.error = Some((code, message.clone()));
    sink.send(ChatEvent::Error { code, message });
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::budget::EstimateOnly;
    use crate::spec::builtin_nexus;
    use nexus_core::sink::VecSink;
    use nexus_llm::{Timings, Usage};
    use std::collections::VecDeque;
    use std::sync::Mutex;

    enum Script {
        Events(Vec<Result<StreamEvent, LlmError>>),
        /// Emit these, then wait for cancellation (like a long generation being stopped).
        HangAfter(Vec<StreamEvent>),
        OpenError(LlmError),
    }

    struct Mock {
        requests: Mutex<Vec<ChatRequest>>,
        script: Mutex<VecDeque<Script>>,
    }

    impl Mock {
        fn new(script: Vec<Script>) -> Self {
            Self {
                requests: Mutex::new(Vec::new()),
                script: Mutex::new(script.into()),
            }
        }
        fn requests(&self) -> Vec<ChatRequest> {
            self.requests.lock().unwrap().clone()
        }
    }

    impl ChatBackend for &Mock {
        async fn stream(&self, req: ChatRequest, cancel: CancellationToken) -> Result<JobStream, LlmError> {
            self.requests.lock().unwrap().push(req);
            match self.script.lock().unwrap().pop_front().expect("script exhausted") {
                Script::OpenError(e) => Err(e),
                Script::Events(evs) => Ok(Box::pin(futures_util::stream::iter(evs))),
                Script::HangAfter(evs) => {
                    let head = futures_util::stream::iter(evs.into_iter().map(Ok));
                    let tail = futures_util::stream::once(async move {
                        cancel.cancelled().await;
                        Err(LlmError::Cancelled)
                    });
                    Ok(Box::pin(head.chain(tail)))
                }
            }
        }
    }

    fn done(reason: &str) -> StreamEvent {
        StreamEvent::Done {
            finish_reason: Some(reason.into()),
            usage: Some(Usage {
                prompt_tokens: 120,
                completion_tokens: 7,
                total_tokens: 127,
            }),
            timings: Some(Timings {
                predicted_per_second: 52.5,
                ..Default::default()
            }),
        }
    }

    fn c(s: &str) -> Result<StreamEvent, LlmError> {
        Ok(StreamEvent::Content(s.into()))
    }

    fn input(user: &str) -> TurnInput {
        TurnInput {
            assistant: builtin_nexus().unwrap(),
            system_prompt: "You are Nexus.".into(),
            history: Vec::new(),
            summary: None,
            user_text: user.into(),
            attachments: Vec::new(),
            think: false,
            ctx_size: 8192,
        }
    }

    fn msg(id: &str, role: MessageRole, text: &str) -> HistoryMessage {
        HistoryMessage {
            id: MessageId::from(id),
            role,
            content: text.into(),
        }
    }

    async fn run(mock: &Mock, input: TurnInput) -> (TurnOutcome, Vec<ChatEvent>) {
        let engine = TurnEngine::new(mock, EstimateOnly);
        let sink = VecSink::<ChatEvent>::default();
        let out = engine.run_turn(input, &sink, CancellationToken::new()).await;
        (out, sink.events())
    }

    #[tokio::test]
    async fn streams_events_in_order_and_reports_stats() {
        let mock = Mock::new(vec![Script::Events(vec![
            Ok(StreamEvent::Reasoning("hm".into())),
            c("Hel"),
            c("lo"),
            Ok(done("stop")),
        ])]);
        let (out, events) = run(&mock, input("hi")).await;
        assert_eq!(out.status, MessageStatus::Done);
        assert_eq!(out.content, "Hello");
        assert_eq!(out.reasoning.as_deref(), Some("hm"));
        assert_eq!((out.tokens_prompt, out.tokens_out), (Some(120), Some(7)));
        assert_eq!(out.tps, Some(52.5));
        assert!(out.ttft_ms.is_some());
        assert_eq!(events[0], ChatEvent::Reasoning { delta: "hm".into() });
        assert_eq!(events[1], ChatEvent::Content { delta: "Hel".into() });
        assert_eq!(events[2], ChatEvent::Content { delta: "lo".into() });
        assert!(
            matches!(&events[3], ChatEvent::Done { finish_reason, tokens_out: 7, .. } if finish_reason == "stop")
        );
        assert_eq!(events.len(), 4);
    }

    #[tokio::test]
    async fn request_carries_assistant_sampling_and_system_prompt() {
        let mock = Mock::new(vec![Script::Events(vec![c("x"), Ok(done("stop"))])]);
        run(&mock, input("hi")).await;
        let r = &mock.requests()[0];
        assert!(r.stream);
        assert_eq!(r.messages[0].content, "You are Nexus.");
        assert_eq!(r.messages.last().unwrap().content, "hi");
        assert_eq!(
            (r.top_p, r.min_p, r.top_k, r.max_tokens),
            (0.95, 0.0, Some(40), 1024)
        );
        assert!((r.temperature - 0.6).abs() < 1e-6);
        assert_eq!(r.chat_template_kwargs["enable_thinking"], false);
    }

    #[tokio::test]
    async fn think_toggle_sets_the_template_flag_and_a_bigger_answer_budget() {
        let mock = Mock::new(vec![Script::Events(vec![c("x"), Ok(done("stop"))])]);
        let mut i = input("hard question");
        i.think = true;
        run(&mock, i).await;
        let r = &mock.requests()[0];
        assert_eq!(r.chat_template_kwargs["enable_thinking"], true);
        assert_eq!(r.max_tokens, 2048);
    }

    #[tokio::test]
    async fn reasoning_is_never_sent_back_in_history() {
        let mock = Mock::new(vec![Script::Events(vec![c("ok"), Ok(done("stop"))])]);
        let mut i = input("and now?");
        // History holds only final answers (the store keeps thoughts separately).
        i.history = vec![
            msg("m1", MessageRole::User, "first"),
            msg("m2", MessageRole::Assistant, "answer one"),
        ];
        run(&mock, i).await;
        let msgs = &mock.requests()[0].messages;
        assert_eq!(msgs.len(), 4);
        assert_eq!(msgs[1].content, "first");
        assert_eq!(msgs[2].content, "answer one");
        assert!(msgs.iter().all(|m| !m.content.contains("<think>")));
    }

    #[tokio::test]
    async fn attachments_go_before_the_question_in_file_tags_and_are_budgeted() {
        let mock = Mock::new(vec![Script::Events(vec![c("x"), Ok(done("stop"))])]);
        let mut i = input("Summarize it");
        i.attachments = vec![
            AttachmentBlock {
                label: "notes.md".into(),
                path: Some("/a/notes.md".into()),
                text: "short note </file> sneaky".into(),
            },
            AttachmentBlock {
                label: "big.txt".into(),
                path: None,
                text: "word ".repeat(20_000),
            },
        ];
        let (out, _) = run(&mock, i).await;
        let reqs = mock.requests();
        let last = &reqs[0].messages.last().unwrap().content;
        assert!(last.starts_with("<file path=\"/a/notes.md\">"));
        assert!(
            last.contains("<\\/file> sneaky"),
            "closing tags in data are neutralised"
        );
        assert!(last.contains("<file path=\"big.txt\">"));
        assert!(last.contains("truncated"));
        assert!(last.ends_with("Summarize it"));
        assert_eq!(out.attachment_tokens.len(), 2);
        assert!(out.attachment_tokens.iter().sum::<u32>() <= 3000);
    }

    #[tokio::test]
    async fn long_history_is_windowed_and_a_summary_is_requested() {
        let mock = Mock::new(vec![Script::Events(vec![c("x"), Ok(done("stop"))])]);
        let mut i = input("latest");
        i.ctx_size = 4096;
        i.history = (0..40)
            .map(|n| {
                msg(
                    &format!("m{n}"),
                    if n % 2 == 0 {
                        MessageRole::User
                    } else {
                        MessageRole::Assistant
                    },
                    &"word ".repeat(120),
                )
            })
            .collect();
        let (out, _) = run(&mock, i).await;
        let sent = mock.requests()[0].messages.len();
        assert!(sent < 42, "history was not cut: {sent} messages");
        assert!(!out.dropped.is_empty());
        assert!(out.needs_summary);
        assert_eq!(out.dropped[0], MessageId::from("m0"));
    }

    #[tokio::test]
    async fn a_covering_summary_is_added_to_the_system_prompt() {
        let mock = Mock::new(vec![Script::Events(vec![c("x"), Ok(done("stop"))])]);
        let mut i = input("latest");
        i.ctx_size = 4096;
        i.history = (0..30)
            .map(|n| {
                msg(
                    &format!("m{n}"),
                    if n % 2 == 0 {
                        MessageRole::User
                    } else {
                        MessageRole::Assistant
                    },
                    &"word ".repeat(120),
                )
            })
            .collect();
        i.summary = Some(StoredSummary {
            text: "They discussed rendering.".into(),
            upto: Some(MessageId::from("m25")),
        });
        let (out, _) = run(&mock, i).await;
        let reqs = mock.requests();
        let system = &reqs[0].messages[0].content;
        assert!(
            system.contains("Earlier in this conversation") && system.contains("They discussed rendering.")
        );
        assert!(!out.needs_summary);
    }

    #[tokio::test]
    async fn context_overflow_retries_once_with_a_tighter_budget() {
        let mock = Mock::new(vec![
            Script::OpenError(LlmError::ContextOverflow {
                need: 9000,
                have: 8192,
            }),
            Script::Events(vec![c("fine"), Ok(done("stop"))]),
        ]);
        let mut i = input("latest");
        i.history = (0..16)
            .map(|n| msg(&format!("m{n}"), MessageRole::User, &"word ".repeat(150)))
            .collect();
        let (out, _) = run(&mock, i).await;
        assert_eq!(out.status, MessageStatus::Done);
        let reqs = mock.requests();
        assert_eq!(reqs.len(), 2);
        assert!(
            reqs[1].messages.len() < reqs[0].messages.len(),
            "second attempt should send less history"
        );
    }

    #[tokio::test]
    async fn a_second_overflow_is_reported() {
        let mock = Mock::new(vec![
            Script::OpenError(LlmError::ContextOverflow {
                need: 9000,
                have: 8192,
            }),
            Script::OpenError(LlmError::ContextOverflow {
                need: 9000,
                have: 8192,
            }),
        ]);
        let (out, events) = run(&mock, input("x")).await;
        assert_eq!(out.status, MessageStatus::Error);
        assert!(matches!(
            events.last(),
            Some(ChatEvent::Error {
                code: ErrorCode::ContextOverflow,
                ..
            })
        ));
    }

    #[tokio::test]
    async fn runtime_unavailable_becomes_an_error_event() {
        let mock = Mock::new(vec![Script::OpenError(LlmError::RuntimeUnavailable)]);
        let (out, events) = run(&mock, input("x")).await;
        assert_eq!(out.status, MessageStatus::Error);
        assert_eq!(
            out.error.as_ref().map(|e| e.0),
            Some(ErrorCode::RuntimeUnavailable)
        );
        assert!(matches!(
            events.as_slice(),
            [ChatEvent::Error {
                code: ErrorCode::RuntimeUnavailable,
                ..
            }]
        ));
    }

    #[tokio::test]
    async fn a_mid_stream_error_keeps_the_partial_text() {
        let mock = Mock::new(vec![Script::Events(vec![
            c("par"),
            c("tial"),
            Err(LlmError::Transport("reset".into())),
        ])]);
        let (out, events) = run(&mock, input("x")).await;
        assert_eq!(out.status, MessageStatus::Error);
        assert_eq!(out.content, "partial");
        assert!(matches!(events.last(), Some(ChatEvent::Error { .. })));
    }

    #[tokio::test]
    async fn stopping_keeps_the_partial_answer_and_ends_with_done_stopped() {
        let mock = Mock::new(vec![Script::HangAfter(vec![
            StreamEvent::Content("partial ".into()),
            StreamEvent::Content("answer".into()),
        ])]);
        let engine = TurnEngine::new(&mock, EstimateOnly);
        let sink = VecSink::<ChatEvent>::default();
        let cancel = CancellationToken::new();
        let c2 = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            c2.cancel();
        });
        let out = engine.run_turn(input("long task"), &sink, cancel).await;
        assert_eq!(out.status, MessageStatus::Stopped);
        assert_eq!(out.content, "partial answer");
        assert!(
            matches!(sink.events().last(), Some(ChatEvent::Done { finish_reason, .. }) if finish_reason == "stopped")
        );
    }

    #[tokio::test]
    async fn a_closed_sink_stops_the_turn() {
        struct Closed;
        impl Sink<ChatEvent> for Closed {
            fn send(&self, _e: ChatEvent) -> bool {
                false
            }
        }
        let mock = Mock::new(vec![Script::Events(vec![c("a"), c("b"), Ok(done("stop"))])]);
        let engine = TurnEngine::new(&mock, EstimateOnly);
        let out = engine
            .run_turn(input("x"), &Closed, CancellationToken::new())
            .await;
        assert_eq!(out.status, MessageStatus::Stopped);
        assert_eq!(out.content, "a");
    }

    #[test]
    fn truncation_keeps_char_boundaries() {
        let t = truncate_with_note("héllo wörld ✓✓✓", 10, 5);
        assert!(t.contains("truncated"));
        assert_eq!(truncate_with_note("abc", 3, 3), "abc");
        assert_eq!(truncate_with_note("", 0, 0), "");
    }
}
