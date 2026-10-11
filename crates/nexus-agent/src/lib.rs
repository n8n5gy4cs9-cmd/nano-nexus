//! `nexus-agent`: assistants, prompts, context budgeting, and workflows.
//!
//! M04 (this milestone) covers the chat workflow only: the built-in Nexus assistant, system-prompt
//! rendering, the context budget, and the turn engine. Tools, routing, pipelines, and the full
//! assistant registry arrive in M06.

pub mod budget;
pub mod internal;
pub mod jobs;
pub mod pipelines;
pub mod prompt;
pub mod registry;
pub mod router;
pub mod spec;
pub mod tools;
pub mod turn;

pub use budget::{
    ContextMeter, HistoryItem, HistoryPlan, Limits, SummaryInfo, TokenCounter, Tokenize, estimate_tokens,
    plan_history,
};
pub use internal::{PromptMeta, RenderedPrompt, internal_names, internal_source, parse_meta, render_full};
pub use jobs::{SummaryTurn, fallback_title, generate_title, summarize_history};
pub use prompt::{PREAMBLE, PromptContext, render_system_prompt};
pub use registry::{FieldError, Registry, Source, ValidationReport, validate_report, validate_toml};
pub use spec::{AssistantSpec, builtin_nexus};
pub use turn::{
    AttachmentBlock, ChatBackend, HistoryMessage, StoredSummary, TurnEngine, TurnInput, TurnOutcome,
    TurnRunner,
};
