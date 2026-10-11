//! Pipelines (ASSISTANTS §3.3): fixed Rust code with small, typed LLM sub-steps. Each pipeline does its
//! deterministic work, makes any constrained-JSON calls it needs, and returns a [`StreamPlan`] for the final
//! streamed answer plus an [`Artifact`] for the UI to store. The app streams the plan through the same
//! turn engine as chat, so events, persistence, and cancellation behave identically.

pub mod doc;
pub mod folder;
pub mod rename;
pub mod simple;
pub mod strings;
#[cfg(test)]
mod testkit;
pub mod tidy;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use nexus_analyze::file_report::FileReport;
use nexus_analyze::logs::LogReport;
use nexus_analyze::report::FolderReport;
use nexus_analyze::secrets::SecretsReport;
use nexus_core::changes::ChangeSet;
use nexus_llm::{
    ChatMessage, Priority, Scheduler, Structured, StructuredSpec, Validate, ValidationCtx, chat_json,
};
use serde::Deserialize;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::internal::{PromptSampling, RenderedPrompt, internal_source, render_full, sampling_of};
use crate::tools::{Gatekeeper, ToolError};

/// Pipeline failures the UI shows as a friendly error card.
#[derive(Debug, thiserror::Error)]
pub enum PipelineError {
    #[error(transparent)]
    Access(#[from] ToolError),
    #[error("{0}")]
    Input(String),
    #[error("{0}")]
    Failed(String),
    #[error("cancelled")]
    Cancelled,
}

/// The final streamed answer: system prompt, few-shot turns, user message, and sampling.
#[derive(Clone, Debug, PartialEq)]
pub struct StreamPlan {
    pub system: String,
    pub examples: Vec<(String, String)>,
    pub user: String,
    pub sampling: PromptSampling,
    /// The facts the answer is checked against.
    pub facts: String,
}

/// What the pipeline produced besides the streamed text.
#[derive(Clone, Debug, PartialEq)]
pub enum Artifact {
    None,
    Folder(Box<FolderReport>),
    File(Box<FileReport>),
    Secrets(Box<SecretsReport>),
    Logs(Box<LogReport>),
    /// A planned change set (moves) for the Changes page.
    Plan(Box<ChangeSet>),
    /// A rendered commit message.
    Commit(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct PipelineOutput {
    pub stream: Option<StreamPlan>,
    pub artifact: Artifact,
    /// Text to show as the whole answer when no model step is needed.
    pub text: Option<String>,
    /// Plain-language notes ("3 chunks were summarized without the model").
    pub notes: Vec<String>,
}

impl PipelineOutput {
    pub fn stream(plan: StreamPlan, artifact: Artifact) -> Self {
        Self {
            stream: Some(plan),
            artifact,
            text: None,
            notes: Vec::new(),
        }
    }
    pub fn text(text: impl Into<String>, artifact: Artifact) -> Self {
        Self {
            stream: None,
            artifact,
            text: Some(text.into()),
            notes: Vec::new(),
        }
    }
}

/// What a constrained-JSON step may check beyond the schema.
#[derive(Clone, Debug, Default)]
pub struct JsonCheck {
    /// Ids the model may use (`allowed_ids` in the validation context).
    pub allowed_ids: Vec<String>,
    /// Page range every `page` value must fall in: `(first, last)`.
    pub pages: Option<(u32, u32)>,
}

/// A constrained-JSON model call. The app uses [`SchedulerJson`]; tests script it.
#[async_trait]
pub trait JsonLlm: Send + Sync {
    /// `None` means "use the deterministic fallback".
    async fn json(
        &self,
        prompt: RenderedPrompt,
        schema: &Value,
        sampling: PromptSampling,
        check: &JsonCheck,
        cancel: &CancellationToken,
    ) -> Option<Value>;
}

/// Validates a JSON value for the `JsonCheck` carried in the validation context.
#[derive(Debug, Deserialize)]
#[serde(transparent)]
struct Checked(Value);

impl Validate for Checked {
    fn validate(&self, ctx: &ValidationCtx) -> Result<(), Vec<String>> {
        let mut errs = Vec::new();
        if let Some(p) = &ctx.pattern
            && let Some(range) = p.strip_prefix("pages:")
            && let Some((a, b)) = range.split_once('-')
            && let (Ok(a), Ok(b)) = (a.parse::<u64>(), b.parse::<u64>())
        {
            collect_pages(&self.0, a, b, &mut errs);
        }
        if !ctx.allowed_ids.is_empty() {
            collect_ids(&self.0, &ctx.allowed_ids, &mut errs);
        }
        if errs.is_empty() { Ok(()) } else { Err(errs) }
    }
}

fn collect_pages(v: &Value, a: u64, b: u64, errs: &mut Vec<String>) {
    match v {
        Value::Object(o) => {
            for (k, x) in o {
                if k == "page"
                    && let Some(n) = x.as_u64()
                    && !(a..=b).contains(&n)
                {
                    errs.push(format!("page {n} is outside this part (pages {a} to {b})"));
                }
                collect_pages(x, a, b, errs);
            }
        }
        Value::Array(arr) => arr.iter().for_each(|x| collect_pages(x, a, b, errs)),
        _ => {}
    }
}

fn collect_ids(v: &Value, allowed: &[String], errs: &mut Vec<String>) {
    match v {
        Value::Object(o) => {
            for (k, x) in o {
                if k == "id"
                    && let Some(s) = x.as_str()
                    && !allowed.iter().any(|a| a == s)
                {
                    errs.push(format!("`{s}` is not one of the given ids"));
                }
                collect_ids(x, allowed, errs);
            }
        }
        Value::Array(arr) => arr.iter().for_each(|x| collect_ids(x, allowed, errs)),
        _ => {}
    }
}

/// [`JsonLlm`] over the llama-server scheduler (Background priority: never delays a chat answer).
pub struct SchedulerJson(pub Arc<Scheduler>);

#[async_trait]
impl JsonLlm for SchedulerJson {
    async fn json(
        &self,
        prompt: RenderedPrompt,
        schema: &Value,
        sampling: PromptSampling,
        check: &JsonCheck,
        cancel: &CancellationToken,
    ) -> Option<Value> {
        let mut msgs = vec![ChatMessage::system(prompt.system)];
        for (u, a) in prompt.examples {
            msgs.push(ChatMessage::user(u));
            msgs.push(ChatMessage::assistant(a));
        }
        msgs.push(ChatMessage::user(prompt.user));
        let spec = StructuredSpec {
            name: "pipeline_step",
            schema,
            temperature: sampling.temperature,
            max_tokens: sampling.max_tokens,
        };
        let vctx = ValidationCtx {
            allowed_ids: check.allowed_ids.clone(),
            pattern: check.pages.map(|(a, b)| format!("pages:{a}-{b}")),
            ..ValidationCtx::default()
        };
        match chat_json::<Checked>(&self.0, Priority::Background, msgs, spec, &vctx, cancel.clone()).await {
            Ok(Structured::Valid(Checked(v))) => Some(v),
            _ => None,
        }
    }
}

/// Everything a pipeline needs from the app.
pub struct PipelineCx<'a> {
    pub llm: &'a dyn JsonLlm,
    pub gate: &'a dyn Gatekeeper,
    pub cancel: &'a CancellationToken,
    /// "Scanning…", "Summarizing part 3 of 12…" for the chat status line.
    pub status: &'a (dyn Fn(String) + Sync),
    pub home: Option<PathBuf>,
}

impl PipelineCx<'_> {
    pub fn say(&self, text: impl Into<String>) {
        (self.status)(text.into());
    }
    pub fn check_cancel(&self) -> Result<(), PipelineError> {
        if self.cancel.is_cancelled() {
            Err(PipelineError::Cancelled)
        } else {
            Ok(())
        }
    }
}

/// Remove the outer `<facts>` tags (the prompt templates add their own).
pub fn unwrap_facts(facts: &str) -> &str {
    facts
        .trim()
        .strip_prefix("<facts>")
        .and_then(|s| s.strip_suffix("</facts>"))
        .map_or(facts.trim(), str::trim)
}

/// Render the internal prompt `name`.
pub fn render_prompt(name: &str, vars: &Value) -> Result<(RenderedPrompt, PromptSampling), PipelineError> {
    let src = internal_source(name)
        .ok_or_else(|| PipelineError::Failed(format!("missing internal prompt {name}")))?;
    let p = render_full(src, vars).map_err(|e| PipelineError::Failed(format!("{name}: {e}")))?;
    Ok((p, sampling_of(src)))
}

/// A stream plan from an internal prompt.
pub fn stream_plan(name: &str, vars: &Value, facts: String) -> Result<StreamPlan, PipelineError> {
    let (p, sampling) = render_prompt(name, vars)?;
    Ok(StreamPlan {
        system: p.system,
        examples: p.examples,
        user: p.user,
        sampling,
        facts,
    })
}

/// Load a JSON schema from `schemas/llm/`.
pub fn llm_schema(json: &str) -> Value {
    serde_json::from_str(json).unwrap_or(Value::Null)
}

/// Authorize a read of `path` (asking the user if needed) and require a folder or file as asked.
pub async fn authorize_read(cx: &PipelineCx<'_>, path: &Path, why: &str) -> Result<PathBuf, PipelineError> {
    Ok(cx.gate.authorize(path, nexus_core::fs::Access::Read, why).await?)
}

/// Numbers in `text`, normalized (`1,234` → `1234`, `12.0` → `12`). List markers at line starts are ignored.
pub fn numbers_in(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        let t = line.trim_start();
        let after_marker = {
            let digits = t.chars().take_while(char::is_ascii_digit).count();
            if digits > 0
                && digits <= 2
                && t[digits..].starts_with([')', '.'])
                && t[digits + 1..].starts_with(' ')
            {
                &t[digits + 1..]
            } else {
                t
            }
        };
        let mut cur = String::new();
        let chars: Vec<char> = after_marker.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            let part_of_word = i > 0 && (chars[i - 1].is_alphabetic() || chars[i - 1] == '_');
            let starts = c.is_ascii_digit() && cur.is_empty() && !part_of_word;
            let continues = !cur.is_empty()
                && (c.is_ascii_digit()
                    || ((c == ',' || c == '.') && chars.get(i + 1).is_some_and(char::is_ascii_digit)));
            if starts || continues {
                cur.push(c);
            } else if !cur.is_empty() {
                out.push(normalize_number(&cur));
                cur.clear();
                continue;
            }
            i += 1;
        }
        if !cur.is_empty() {
            out.push(normalize_number(&cur));
        }
    }
    out
}

fn normalize_number(s: &str) -> String {
    let no_commas = s.replace(',', "");
    if no_commas.contains('.') {
        no_commas.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        no_commas
    }
}

/// Every number in `narrative` that does not appear in `facts` (the narrative validator, M08).
pub fn unsupported_numbers(narrative: &str, facts: &str) -> Vec<String> {
    let known: std::collections::HashSet<String> = numbers_in(facts).into_iter().collect();
    let mut bad: Vec<String> = numbers_in(narrative)
        .into_iter()
        .filter(|n| !known.contains(n))
        .collect();
    bad.sort();
    bad.dedup();
    bad
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn number_extraction() {
        assert_eq!(
            numbers_in("You have 4,812 files (38.4 GB), 37 groups."),
            ["4812", "38.4", "37"]
        );
        assert_eq!(
            numbers_in("1) Remove 2.4 GB of duplicates\n2. Clear 900 MB"),
            ["2.4", "900"]
        );
        assert_eq!(numbers_in("v2 build_3 and 12.0%"), ["12"]);
        assert_eq!(
            numbers_in("on 2026-10-11 at 14:02"),
            ["2026", "10", "11", "14", "02"]
        );
        assert!(numbers_in("no numbers here").is_empty());
    }

    #[test]
    fn the_validator_flags_invented_numbers_only() {
        let facts = "files: 4,812   size: 38.4 GB   duplicates: 37 groups, 2.4 GB wasted";
        assert!(
            unsupported_numbers(
                "**Bottom line:** 38.4 GB across 4812 files; 2.4 GB wasted in 37 groups.",
                facts
            )
            .is_empty()
        );
        assert_eq!(
            unsupported_numbers("About 51 GB across 4812 files.", facts),
            ["51"]
        );
        assert_eq!(
            unsupported_numbers("1) Free 2.40 GB first\n2) then 99 more", facts),
            ["99"]
        );
    }

    #[test]
    fn facts_tags_are_stripped() {
        assert_eq!(unwrap_facts("<facts>\nroot: x\n</facts>"), "root: x");
        assert_eq!(unwrap_facts("plain"), "plain");
    }

    #[test]
    fn json_checks() {
        let ok = Checked(json!({"points": [{"text": "a", "page": 3}], "numbers": [{"page": 4}]}));
        let ctx = |p: &str| ValidationCtx {
            pattern: Some(p.into()),
            ..ValidationCtx::default()
        };
        assert!(ok.validate(&ctx("pages:3-4")).is_ok());
        assert_eq!(ok.validate(&ctx("pages:5-6")).unwrap_err().len(), 2);
        let ids = Checked(json!({"assignments": [{"id": "a1", "target": "x"}, {"id": "zz"}]}));
        let vctx = ValidationCtx {
            allowed_ids: vec!["a1".into()],
            ..ValidationCtx::default()
        };
        assert_eq!(ids.validate(&vctx).unwrap_err().len(), 1);
    }

    #[test]
    fn prompts_render_with_their_sampling() {
        let (p, s) = render_prompt("title", &json!({"user_text": "hi", "assistant_text": "yo"})).unwrap();
        assert!(p.system.starts_with("Write a short title"));
        assert_eq!(s.max_tokens, 40);
        assert!(render_prompt("nope", &json!({})).is_err());
    }
}
