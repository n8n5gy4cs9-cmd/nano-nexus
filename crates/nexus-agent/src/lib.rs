//! `nexus-agent`: assistants, prompts, context budgeting, and workflows.
//!
//! M04 (this milestone) covers the chat workflow only: the built-in Nexus assistant, system-prompt
//! rendering, the context budget, and the turn engine. Tools, routing, pipelines, and the full
//! assistant registry arrive in M06.

pub mod budget;
pub mod prompt;
pub mod spec;

pub use budget::{
    ContextMeter, HistoryItem, HistoryPlan, Limits, SummaryInfo, TokenCounter, Tokenize, plan_history,
};
pub use prompt::{PREAMBLE, PromptContext, render_system_prompt};
pub use spec::{AssistantSpec, builtin_nexus};
