//! The `router` workflow (ASSISTANTS §3.2): bounded tool use before the final answer.
//!
//! ```text
//! for hop in 0..max_hops:
//!    skip the router entirely for small talk (heuristic gate)
//!    decision = constrained-JSON call (thinking off, temperature 0.2), fallback = answer
//!    validate + resolve paths, break on a repeated call, run the tool, keep its budgeted output
//! ```

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use nexus_core::chat::ChatEvent;
use nexus_core::sink::Sink;
use nexus_llm::{
    ChatMessage, Priority, Scheduler, Structured, StructuredSpec, Validate, ValidationCtx, chat_json,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::internal::{internal_source, render_full};
use crate::tools::{Tool, ToolArgs, ToolCx, ToolError};
use crate::turn::AttachmentBlock;

/// Most tool-result text kept for the final answer (tokens, all hops together).
pub const TOTAL_RESULT_BUDGET: u32 = 2400;

/// What the router model returns.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouterAction {
    pub action: String,
    #[serde(default)]
    pub assistant: Option<String>,
    #[serde(default)]
    pub task: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub expression: Option<String>,
    /// `write_file`: the whole file. `edit_file`: unused.
    #[serde(default)]
    pub content: Option<String>,
    /// `edit_file`: the exact passage to replace.
    #[serde(default)]
    pub old_text: Option<String>,
    /// `edit_file`: its replacement.
    #[serde(default)]
    pub new_text: Option<String>,
    /// `move_file`: the destination path.
    #[serde(default)]
    pub to: Option<String>,
    /// `fetch_url`: the page address.
    #[serde(default)]
    pub url: Option<String>,
}

impl RouterAction {
    pub fn answer() -> Self {
        Self {
            action: "answer".into(),
            ..Self::default()
        }
    }
}

impl Validate for RouterAction {
    fn validate(&self, ctx: &ValidationCtx) -> Result<(), Vec<String>> {
        if !ctx.allowed_ids.iter().any(|a| a == &self.action) {
            return Err(vec![format!(
                "`{}` is not an allowed action; use one of: {}",
                self.action,
                ctx.allowed_ids.join(", ")
            )]);
        }
        let mut errors = Vec::new();
        let mut require = |name: &str, value: &Option<String>, allow_empty: bool| {
            if !value
                .as_ref()
                .is_some_and(|v| allow_empty || !v.trim().is_empty())
            {
                errors.push(format!("{} requires `{name}`", self.action));
            }
        };
        match self.action.as_str() {
            "read_file" | "list_dir" | "file_info" | "scan_folder" | "trash_file" => {
                require("path", &self.path, false)
            }
            "write_file" => {
                require("path", &self.path, false);
                require("content", &self.content, true);
            }
            "edit_file" => {
                require("path", &self.path, false);
                require("old_text", &self.old_text, false);
                require("new_text", &self.new_text, true);
            }
            "move_file" => {
                require("path", &self.path, false);
                require("to", &self.to, false);
            }
            "search_files" | "grep_text" | "search_web" | "doc_search" | "kb_search" | "find_symbol" => {
                require("query", &self.query, false)
            }
            "fetch_url" => require("url", &self.url, false),
            "calculate" => require("expression", &self.expression, false),
            "ask_assistant" => {
                require("assistant", &self.assistant, false);
                require("task", &self.task, false);
            }
            _ => {}
        }
        if errors.is_empty() { Ok(()) } else { Err(errors) }
    }
}

/// Makes the routing decision. The app uses [`SchedulerRouter`]; tests script it.
#[async_trait]
pub trait RouterLlm: Send + Sync {
    /// `None` means "use the fallback" (`answer`).
    async fn decide(
        &self,
        messages: Vec<ChatMessage>,
        schema: &Value,
        allowed: &[String],
        max_tokens: u32,
        cancel: CancellationToken,
    ) -> Option<RouterAction>;
}

/// [`RouterLlm`] over the llama-server scheduler.
pub struct SchedulerRouter(pub Arc<Scheduler>);

#[async_trait]
impl RouterLlm for SchedulerRouter {
    async fn decide(
        &self,
        messages: Vec<ChatMessage>,
        schema: &Value,
        allowed: &[String],
        max_tokens: u32,
        cancel: CancellationToken,
    ) -> Option<RouterAction> {
        let spec = StructuredSpec {
            name: "router_action",
            schema,
            temperature: 0.2,
            max_tokens,
        };
        let vctx = ValidationCtx {
            allowed_ids: allowed.to_vec(),
            ..ValidationCtx::default()
        };
        match chat_json::<RouterAction>(&self.0, Priority::Interactive, messages, spec, &vctx, cancel).await {
            Ok(Structured::Valid(a)) => Some(a),
            Ok(Structured::Fallback { reason }) => {
                tracing::debug!(%reason, "router validation failed after correction");
                None
            }
            Err(error) => {
                tracing::debug!(%error, "router request failed");
                None
            }
        }
    }
}

/// Flat action objects with per-tool required arguments. The union prevents constrained decoding
/// from producing a write action without content (which an all-optional argument schema allowed).
pub fn router_schema(allowed_tools: &[String]) -> Value {
    let properties = json!({
        "path": {"type": "string", "maxLength": 1024},
        "query": {"type": "string", "maxLength": 200},
        "expression": {"type": "string", "maxLength": 200},
        "content": {"type": "string", "maxLength": 12000},
        "old_text": {"type": "string", "maxLength": 4000},
        "new_text": {"type": "string", "maxLength": 12000},
        "to": {"type": "string", "maxLength": 1024},
        "url": {"type": "string", "maxLength": 2048},
        "assistant": {"type": "string", "enum": ["planner", "reviewer"]},
        "task": {"type": "string", "maxLength": 6000}
    });
    let actions = std::iter::once("answer").chain(allowed_tools.iter().map(String::as_str));
    let variants: Vec<Value> = actions
        .map(|action| {
            let fields: &[&str] = match action {
                "read_file" | "list_dir" | "file_info" | "scan_folder" | "trash_file" => &["path"],
                "write_file" => &["path", "content"],
                "edit_file" => &["path", "old_text", "new_text"],
                "move_file" => &["path", "to"],
                "search_files" | "grep_text" | "search_web" | "doc_search" | "kb_search" | "find_symbol" => {
                    &["query"]
                }
                "fetch_url" => &["url"],
                "calculate" => &["expression"],
                "ask_assistant" => &["assistant", "task"],
                _ => &[],
            };
            let mut props = serde_json::Map::new();
            props.insert("action".into(), json!({"type": "string", "enum": [action]}));
            let mut required = vec!["action"];
            for field in fields {
                props.insert((*field).into(), properties[*field].clone());
                required.push(field);
            }
            if matches!(action, "search_files" | "grep_text") {
                props.insert("path".into(), properties["path"].clone());
            }
            json!({"type": "object", "additionalProperties": false,
            "required": required, "properties": props})
        })
        .collect();
    json!({"anyOf": variants})
}

/// Most characters of the previous answer shown to the router.
const MAX_PREVIOUS_CHARS: usize = 6000;

const KEYWORDS: [&str; 17] = [
    "file",
    "folder",
    "directory",
    "scan",
    "read",
    "open",
    "pdf",
    "log",
    "csv",
    "size",
    "find",
    "list",
    "how many",
    "disk",
    "downloads",
    "desktop",
    "documents",
];

fn looks_like_filename(word: &str) -> bool {
    let w = word.trim_matches(|c: char| !c.is_alphanumeric() && c != '.' && c != '_' && c != '-');
    match w.rsplit_once('.') {
        Some((stem, ext)) => {
            !stem.is_empty()
                && (1..=5).contains(&ext.len())
                && ext.chars().all(|c| c.is_ascii_alphanumeric())
                && ext.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        }
        None => false,
    }
}

/// Should the router run at all? Skips a model call for chit-chat.
pub fn needs_tools_heuristic(input: &str, granted_roots: &[PathBuf], always_route: bool) -> bool {
    if always_route {
        return true;
    }
    let lower = input.to_lowercase();
    if input.contains("~/")
        || input.contains(":\\")
        || input
            .split_whitespace()
            .any(|w| w.starts_with('/') && w.len() > 1)
    {
        return true;
    }
    if input.split_whitespace().any(looks_like_filename) {
        return true;
    }
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    if KEYWORDS.iter().any(|k| {
        if k.contains(' ') {
            lower.contains(k)
        } else {
            words.contains(k) || words.iter().any(|w| w.strip_suffix('s') == Some(k))
        }
    }) {
        return true;
    }
    granted_roots.iter().filter_map(|r| r.file_name()).any(|n| {
        let n = n.to_string_lossy().to_lowercase();
        n.len() >= 3 && lower.contains(&n)
    })
}

/// Words that mean the user wants something saved, created, changed, moved or removed.
const WRITE_WORDS: [&str; 14] = [
    "save", "write", "create", "make", "edit", "change", "update", "append", "rename", "move", "delete",
    "trash", "remove", "replace",
];

/// Phrases that mean the answer lives on the web.
const WEB_PHRASES: [&str; 16] = [
    "search the web",
    "search online",
    "web search",
    "google",
    "look up",
    "lookup",
    "latest",
    "news",
    "current version",
    "right now",
    "today's",
    "http://",
    "https://",
    "www.",
    "on the internet",
    "online",
];

/// [`needs_tools_heuristic`] plus intent: a request to save a file or to look something up routes when
/// the assistant has the matching tools, even without a path in the text ("save that as notes.txt").
pub fn needs_tools_for(
    input: &str,
    granted_roots: &[PathBuf],
    always_route: bool,
    allowed: &[String],
) -> bool {
    if needs_tools_heuristic(input, granted_roots, always_route) {
        return true;
    }
    let has = |names: &[&str]| allowed.iter().any(|a| names.contains(&a.as_str()));
    let lower = input.to_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    let arithmetic = input.chars().any(|c| c.is_ascii_digit())
        && (input
            .chars()
            .any(|c| matches!(c, '+' | '-' | '*' | '/' | '^' | '%' | '×' | '÷'))
            || [
                "calculate",
                "percent",
                "percentage",
                "sum",
                "multiply",
                "divide",
                "plus",
                "minus",
                "times",
            ]
            .iter()
            .any(|w| words.contains(w)));

    (has(&["calculate"]) && arithmetic)
        || (has(&["now"]) && clock_requested(input))
        || (has(&["write_file", "edit_file", "move_file", "trash_file"])
            && WRITE_WORDS.iter().any(|w| words.contains(w)))
        || (has(&["ask_assistant"])
            && ["plan", "review", "planner", "reviewer", "delegate", "consult"]
                .iter()
                .any(|w| words.contains(w)))
        || (has(&["search_web", "fetch_url"]) && WEB_PHRASES.iter().any(|p| lower.contains(p)))
}

pub(crate) fn requested_specialist(input: &str) -> Option<&'static str> {
    let lower = input.to_lowercase();
    let text = lower.trim().strip_prefix("please ").unwrap_or(lower.trim());
    let text = text
        .strip_prefix("can you ")
        .or_else(|| text.strip_prefix("could you "))
        .unwrap_or(text);
    crate::tools::CONSULTANTS
        .into_iter()
        .find(|id| text.starts_with(&format!("ask {id}")) || text.starts_with(&format!("consult {id}")))
}

pub(crate) fn simple_consultation(input: &str) -> bool {
    let lower = input.to_lowercase();
    requested_specialist(input).is_some()
        && crate::tools::CONSULTANTS
            .iter()
            .filter(|id| {
                lower
                    .split(|c: char| !c.is_alphanumeric())
                    .any(|word| word == **id)
            })
            .count()
            == 1
        && !lower.contains(" then ")
        && ![
            "and create",
            "and write",
            "and save",
            "and edit",
            "and change",
            "and fix",
            "and move",
            "and delete",
            "and ask",
            "and review",
        ]
        .iter()
        .any(|s| lower.contains(s))
}

fn clock_requested(input: &str) -> bool {
    let lower = input.to_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    ((words.contains(&"time") || words.contains(&"date") || words.contains(&"day"))
        && ["current", "now", "today"].iter().any(|w| words.contains(w)))
        || lower.contains("what time")
        || lower.contains("what date")
}

/// Conservative completion guard for explicit file-change instructions, not general writing questions.
pub(crate) fn file_change_requested(input: &str, allowed: &[String]) -> bool {
    let lower = input.to_lowercase();
    let text = lower.trim().strip_prefix("please ").unwrap_or(lower.trim());
    let text = text
        .strip_prefix("can you ")
        .or_else(|| text.strip_prefix("could you "))
        .unwrap_or(text);
    let path = input.contains("~/")
        || input.split_whitespace().any(|w| w.starts_with('/'))
        || input.split_whitespace().any(looks_like_filename);
    let imperative = [
        "create ", "write ", "save ", "edit ", "change ", "update ", "append ", "replace ", "move ",
        "rename ", "delete ", "trash ", "remove ",
    ]
    .iter()
    .any(|w| text.starts_with(w))
        || (text.starts_with("read ")
            && [" and change ", " and edit ", " and replace "]
                .iter()
                .any(|w| text.contains(w)));
    path && imperative && allowed.iter().any(|t| is_file_change(t))
}

fn completion_tools(input: &str, allowed: &[String], has_read: bool) -> Vec<String> {
    let lower = input.to_lowercase();
    let text = lower.trim().strip_prefix("please ").unwrap_or(lower.trim());
    let text = text
        .strip_prefix("can you ")
        .or_else(|| text.strip_prefix("could you "))
        .unwrap_or(text);
    let edit = ["read ", "edit ", "change ", "update ", "append ", "replace "]
        .iter()
        .any(|w| text.starts_with(w));
    let tools: &[&str] = if edit && !has_read && allowed.iter().any(|t| t == "read_file") {
        &["read_file"]
    } else if edit {
        &["edit_file", "write_file"]
    } else if text.starts_with("move ") || text.starts_with("rename ") {
        &["move_file"]
    } else if ["delete ", "trash ", "remove "]
        .iter()
        .any(|w| text.starts_with(w))
    {
        &["trash_file"]
    } else {
        &["write_file"]
    };
    allowed
        .iter()
        .filter(|t| tools.contains(&t.as_str()))
        .cloned()
        .collect()
}

pub(crate) fn is_file_change(tool: &str) -> bool {
    matches!(tool, "write_file" | "edit_file" | "move_file" | "trash_file")
}

/// A validated call ready to run.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedCall {
    pub tool: String,
    pub args: ToolArgs,
}

impl ResolvedCall {
    /// Stable key for the loop guard.
    fn key(&self) -> String {
        format!("{}:{}", self.tool, Value::Object(self.args.clone()))
    }
}

/// Expand `~`, then match bare names against granted folders. Returns `None` if nothing sensible fits.
pub fn resolve_path(raw: &str, home: &Path, roots: &[PathBuf]) -> Option<PathBuf> {
    // Small models sometimes run on into markup (`C:")</param><function name=`): keep the path part only.
    let raw = raw.trim().trim_matches(|c| c == '"' || c == '\'' || c == '`');
    let raw = raw.split(['<', '"']).next().unwrap_or("").trim();
    if raw.is_empty() {
        return None;
    }
    if raw == "~" {
        return Some(home.to_owned());
    }
    if let Some(rest) = raw.strip_prefix("~/").or_else(|| raw.strip_prefix("~\\")) {
        return Some(home.join(rest));
    }
    let p = Path::new(raw);
    if p.is_absolute() {
        return Some(p.to_owned());
    }
    let (first, rest) = match raw.split_once(['/', '\\']) {
        Some((f, r)) => (f, Some(r)),
        None => (raw, None),
    };
    // A bare name equal to a granted folder's name.
    for r in roots {
        if r.file_name()
            .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(first))
        {
            return Some(rest.map_or_else(|| r.clone(), |rest| r.join(rest)));
        }
    }
    // A conventional folder under home ("Downloads"), or something inside a granted folder.
    let under_home = home.join(raw);
    if under_home.exists() {
        return Some(under_home);
    }
    roots
        .iter()
        .map(|r| r.join(raw))
        .find(|c| c.exists())
        .or_else(|| {
            if roots.len() == 1 {
                roots.first().map(|r| r.join(raw))
            } else {
                None
            }
        })
}

fn clean(s: &Option<String>) -> Option<&str> {
    s.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

/// Validate a decision against the allowed tools and resolve its paths. `Err` is a reason (the loop
/// stops and the assistant answers with what it has).
pub fn resolve_action(
    a: &RouterAction,
    allowed: &[String],
    home: &Path,
    roots: &[PathBuf],
) -> Result<ResolvedCall, String> {
    if !allowed.iter().any(|t| t == &a.action) {
        return Err(format!("`{}` is not an allowed tool", a.action));
    }
    let mut args = ToolArgs::new();
    let default_root = || {
        if roots.len() == 1 {
            roots.first().cloned()
        } else {
            None
        }
    };
    let path = |args: &mut ToolArgs, optional_default: bool| -> Result<(), String> {
        let p = match clean(&a.path) {
            Some(raw) => {
                resolve_path(raw, home, roots).ok_or_else(|| format!("could not resolve the path `{raw}`"))?
            }
            None if optional_default => default_root().ok_or("a path is required")?,
            None => return Err("a path is required".into()),
        };
        args.insert("path".into(), Value::String(p.display().to_string()));
        Ok(())
    };
    match a.action.as_str() {
        "read_file" | "list_dir" | "file_info" | "scan_folder" => path(&mut args, false)?,
        "search_files" | "grep_text" => {
            path(&mut args, true)?;
            let q = clean(&a.query).ok_or("a query is required")?;
            args.insert("query".into(), Value::String(q.to_owned()));
        }
        "write_file" => {
            path(&mut args, false)?;
            let c = a.content.as_deref().ok_or("content is required")?;
            args.insert("content".into(), Value::String(c.to_owned()));
        }
        "edit_file" => {
            path(&mut args, false)?;
            let old = a
                .old_text
                .as_deref()
                .filter(|s| !s.is_empty())
                .ok_or("old_text is required")?;
            let new = a.new_text.as_deref().ok_or("new_text is required")?;
            args.insert("old_text".into(), Value::String(old.to_owned()));
            args.insert("new_text".into(), Value::String(new.to_owned()));
        }
        "move_file" => {
            path(&mut args, false)?;
            let raw = clean(&a.to).ok_or("a destination (`to`) is required")?;
            let dest = resolve_path(raw, home, roots)
                .ok_or_else(|| format!("could not resolve the path `{raw}`"))?;
            args.insert("to".into(), Value::String(dest.display().to_string()));
        }
        "trash_file" => path(&mut args, false)?,
        "search_web" => {
            let q = clean(&a.query).ok_or("a query is required")?;
            args.insert("query".into(), Value::String(q.to_owned()));
        }
        "fetch_url" => {
            let u = clean(&a.url).ok_or("a url is required")?;
            args.insert("url".into(), Value::String(u.to_owned()));
        }
        "doc_search" | "kb_search" | "find_symbol" => {
            let q = clean(&a.query).ok_or("a query is required")?;
            args.insert("query".into(), Value::String(q.to_owned()));
        }
        "calculate" => {
            let e = clean(&a.expression).ok_or("an expression is required")?;
            args.insert("expression".into(), Value::String(e.to_owned()));
        }
        "now" => {}
        "ask_assistant" => {
            let assistant = clean(&a.assistant).ok_or("assistant is required")?;
            let task = a
                .task
                .as_deref()
                .filter(|t| !t.trim().is_empty())
                .ok_or("task is required")?;
            args.insert("assistant".into(), Value::String(assistant.to_owned()));
            args.insert("task".into(), Value::String(task.to_owned()));
        }
        other => return Err(format!("unknown tool `{other}`")),
    }
    Ok(ResolvedCall {
        tool: a.action.clone(),
        args,
    })
}

/// One executed (or failed) call.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ToolRun {
    pub tool: String,
    pub args_json: String,
    pub ok: bool,
    pub summary: String,
    /// Text for the final answer's context (a result, or the model-facing error).
    pub llm_text: String,
}

/// What the router produced.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct RouterOutcome {
    pub runs: Vec<ToolRun>,
    /// Why the loop ended: `skipped`, `answer`, `repeat`, `invalid`, `incomplete`, `max_hops`, `cancelled`.
    pub stopped: String,
}

impl RouterOutcome {
    /// Tool results, newest first, within the shared budget. Recent failures and write status
    /// take priority over older reads so the final answer can accurately report what happened.
    pub fn as_attachments(&self) -> Vec<AttachmentBlock> {
        let mut left = TOTAL_RESULT_BUDGET;
        let mut out = Vec::new();
        for r in self.runs.iter().rev() {
            let text = if r.ok {
                if r.llm_text.starts_with("<tool_result") || r.llm_text.starts_with("<file ") {
                    r.llm_text.clone()
                } else {
                    format!(
                        "<tool_result name=\"{}\">\n{}\n</tool_result>",
                        r.tool, r.llm_text
                    )
                }
            } else {
                format!(
                    "<tool_result name=\"{}\" error=\"true\">\n{}\n</tool_result>",
                    r.tool, r.llm_text
                )
            };
            let (text, _) = crate::tools::truncate_to_tokens(&text, left);
            left = left.saturating_sub(crate::budget::estimate_tokens(&text));
            out.push(AttachmentBlock {
                label: format!("tool result: {}", r.tool),
                path: None,
                text,
            });
            if left == 0 {
                break;
            }
        }
        out
    }
}

/// Inputs of one router run.
pub struct RouterRequest<'a> {
    pub user_text: &'a str,
    /// Tools this assistant may use (names), already intersected with the registry.
    pub allowed_tools: Vec<String>,
    pub max_hops: u32,
    pub always_route: bool,
    pub home: PathBuf,
    /// The assistant's last answer, so "save that" has something to save. May be empty.
    pub previous_answer: String,
}

/// Run the router loop. Never fails: any problem ends the loop and the assistant answers anyway.
pub async fn run_router(
    req: RouterRequest<'_>,
    llm: &dyn RouterLlm,
    tools: &[Arc<dyn Tool>],
    cx: &ToolCx,
    sink: &dyn Sink<ChatEvent>,
    cancel: CancellationToken,
) -> RouterOutcome {
    let roots = cx.gate.granted_roots();
    let mut outcome = RouterOutcome::default();
    if req.allowed_tools.is_empty()
        || !needs_tools_for(req.user_text, &roots, req.always_route, &req.allowed_tools)
    {
        outcome.stopped = "skipped".into();
        return outcome;
    }
    let Some(src) = internal_source("router") else {
        outcome.stopped = "invalid".into();
        return outcome;
    };
    let tool_info: Vec<Value> = req
        .allowed_tools
        .iter()
        .filter_map(|n| tools.iter().find(|t| t.name() == n))
        .map(|t| json!({"signature": t.signature(), "description": t.description()}))
        .collect();
    let has_tool = |n: &str| req.allowed_tools.iter().any(|t| t == n);
    // The router writes a whole file inside its JSON decision, so it needs room for it.
    let max_tokens = if has_tool("write_file") || has_tool("edit_file") {
        3500
    } else if has_tool("ask_assistant") {
        2200
    } else {
        160
    };
    let previous: String = req.previous_answer.chars().take(MAX_PREVIOUS_CHARS).collect();
    let mut allowed_actions = vec!["answer".to_owned()];
    allowed_actions.extend(req.allowed_tools.iter().cloned());
    let schema = router_schema(&req.allowed_tools);
    let mut seen = HashSet::new();
    let needs_change = file_change_requested(req.user_text, &req.allowed_tools);
    let mut completion_reasked = false;
    outcome.stopped = "max_hops".into();

    for _hop in 0..req.max_hops.max(1) {
        if cancel.is_cancelled() {
            outcome.stopped = "cancelled".into();
            break;
        }
        let vars = json!({
            "tools": tool_info,
            // Forward slashes: a Windows `C:\Users\me` pasted into the JSON example breaks the example
            // (invalid escapes), and the model then imitates the broken output. Windows accepts `/`.
            "granted_roots": roots.iter().map(|r| r.display().to_string().replace('\\', "/")).collect::<Vec<_>>(),
            "home_dir": req.home.display().to_string().replace('\\', "/"),
            "request": req.user_text,
            "tool_results_so_far": outcome.runs.len(),
            "has_write": has_tool("write_file"),
            "has_delegate": has_tool("ask_assistant"),
            "has_edit": has_tool("edit_file"),
            "has_move": has_tool("move_file") || has_tool("trash_file"),
            "has_web": has_tool("search_web"),
            "previous_answer": if has_tool("write_file") || has_tool("edit_file") { previous.as_str() } else { "" },
        });
        let Ok(p) = render_full(src, &vars) else {
            outcome.stopped = "invalid".into();
            break;
        };
        let mut msgs = vec![ChatMessage::system(p.system)];
        for (u, a) in p.examples {
            msgs.push(ChatMessage::user(u));
            msgs.push(ChatMessage::assistant(a));
        }
        let mut user = p.user;
        if !outcome.runs.is_empty() {
            user.push_str("\nDone so far:\n");
            // Like REFE/pi-main's agent loop, feed observations back into the next decision.
            // Summaries alone lose file contents, search hits, and actionable tool errors.
            // Reuse the final-answer budget so multi-hop context stays bounded.
            for result in outcome.as_attachments() {
                user.push_str(&result.text);
                user.push('\n');
            }
        }
        msgs.push(ChatMessage::user(user));

        // A library assistant exists to search: do not let a small model decide to skip the search or
        // rephrase the question badly. The first hop searches with the user's own words.
        let forced_library_search = req.always_route
            && outcome.runs.is_empty()
            && req.allowed_tools.len() == 1
            && req.allowed_tools[0] == "kb_search"
            && !req.user_text.trim().is_empty();
        let forced_clock = outcome.runs.is_empty() && has_tool("now") && clock_requested(req.user_text);
        let explicit_consultation = outcome.runs.is_empty() && has_tool("ask_assistant");
        let specialist = if explicit_consultation {
            requested_specialist(req.user_text)
        } else {
            None
        };
        let mut decision = if let Some(assistant) = specialist {
            RouterAction {
                action: "ask_assistant".into(),
                assistant: Some(assistant.into()),
                task: Some(req.user_text.to_owned()),
                ..RouterAction::default()
            }
        } else if forced_clock {
            RouterAction {
                action: "now".into(),
                ..RouterAction::default()
            }
        } else if forced_library_search {
            RouterAction {
                action: "kb_search".into(),
                query: Some(req.user_text.trim().to_owned()),
                ..RouterAction::default()
            }
        } else {
            llm.decide(msgs.clone(), &schema, &allowed_actions, max_tokens, cancel.clone())
                .await
                .unwrap_or_else(|| {
                    outcome.runs.push(ToolRun {
                        tool: "router".into(), args_json: "{}".into(), ok: false,
                        summary: "Tool planning failed".into(),
                        llm_text: "Tool planning failed after correction. No action was executed for this decision. Explain the failure; do not claim a file was written or changed.".into(),
                    });
                    RouterAction::answer()
                })
        };
        let repeated = resolve_action(&decision, &req.allowed_tools, &req.home, &roots)
            .ok()
            .is_some_and(|call| seen.contains(&call.key()));
        if (decision.action == "answer" || repeated)
            && needs_change
            && !completion_reasked
            && !outcome.runs.iter().any(|r| r.ok && is_file_change(&r.tool))
            && !cancel.is_cancelled()
        {
            completion_reasked = true;
            let required = completion_tools(
                req.user_text,
                &req.allowed_tools,
                outcome.runs.iter().any(|r| r.ok && r.tool == "read_file"),
            );
            let mut required_schema = router_schema(&required);
            if let Some(variants) = required_schema["anyOf"].as_array_mut() {
                variants.retain(|v| v["properties"]["action"]["enum"][0] != "answer");
            }
            if !required.is_empty() {
                msgs.push(ChatMessage::assistant("{\"action\":\"answer\"}"));
                msgs.push(ChatMessage::user("Completion check: the user explicitly requested a file change, but no file-changing tool has succeeded. Reading is not completion. Choose the required next tool with its complete arguments."));
                decision = llm
                    .decide(msgs, &required_schema, &required, max_tokens, cancel.clone())
                    .await
                    .unwrap_or_else(RouterAction::answer);
            }
        }
        if decision.action == "answer" {
            outcome.stopped = "answer".into();
            break;
        }
        let call = match resolve_action(&decision, &req.allowed_tools, &req.home, &roots) {
            Ok(c) => c,
            Err(reason) => {
                tracing::debug!(%reason, "router decision rejected");
                outcome.runs.push(ToolRun {
                    tool: decision.action.clone(), args_json: "{}".into(), ok: false,
                    summary: "Tool decision rejected".into(),
                    llm_text: format!("Tool decision rejected: {reason}. No action was executed. Explain what is missing; do not claim success."),
                });
                outcome.stopped = "invalid".into();
                break;
            }
        };
        if !seen.insert(call.key()) {
            outcome.stopped = "repeat".into();
            break;
        }
        let args_json = Value::Object(call.args.clone()).to_string();
        sink.send(ChatEvent::ToolCall {
            tool: call.tool.clone(),
            args_json: args_json.clone(),
        });
        let Some(tool) = tools.iter().find(|t| t.name() == call.tool) else {
            outcome.stopped = "invalid".into();
            break;
        };
        let mut cx_run = ToolCx {
            cancel: cancel.clone(),
            consultations: cx.consultations.clone(),
            gate: cx.gate.clone(),
            reason: cx.reason.clone(),
            budget_tokens: cx.budget_tokens,
            docs: cx.docs.clone(),
            kb: cx.kb.clone(),
            code: cx.code.clone(),
            proposals: cx.proposals.clone(),
            web: cx.web.clone(),
        };
        cx_run.reason = format!("to run {}", call.tool);
        let run = match tool.run(call.args, &cx_run).await {
            Ok(o) => {
                // A proposed change gets its own clickable chip that opens Changes, where Apply is.
                if let Some(id) = o.ui.as_ref().and_then(|u| u["change_set"].as_str()) {
                    sink.send(ChatEvent::ToolCall {
                        tool: "report".into(),
                        args_json: json!({"changeset_id": id}).to_string(),
                    });
                    let state =
                        o.ui.as_ref()
                            .and_then(|u| u["state"].as_str())
                            .unwrap_or("planned");
                    sink.send(ChatEvent::ToolResult {
                        tool: "report".into(),
                        summary: match state {
                            "applied" => "Change applied (undo in Changes)",
                            "failed" => "Change not applied: review it",
                            _ => "Change ready: Apply or Discard",
                        }
                        .into(),
                        ok: state != "failed",
                    });
                }
                ToolRun {
                    tool: call.tool.clone(),
                    args_json,
                    ok: true,
                    summary: o.summary,
                    llm_text: o.llm_text,
                }
            }
            Err(e) => ToolRun {
                tool: call.tool.clone(),
                args_json,
                ok: false,
                summary: tool_error_summary(&call.tool, &e),
                llm_text: e.for_model(),
            },
        };
        sink.send(ChatEvent::ToolResult {
            tool: run.tool.clone(),
            summary: run.summary.clone(),
            ok: run.ok,
        });
        let stop_after = matches!(&run, ToolRun { ok: false, .. }) && is_permission_stop(&run.llm_text);
        let consultation_finished = run.tool == "ask_assistant" && simple_consultation(req.user_text);
        outcome.runs.push(run);
        if consultation_finished {
            outcome.stopped = "answer".into();
            break;
        }
        if stop_after {
            // A declined or ungranted folder: answer now so the assistant can explain, instead of retrying.
            outcome.stopped = "answer".into();
            break;
        }
    }
    if needs_change && !cancel.is_cancelled() && !outcome.runs.iter().any(|r| r.ok && is_file_change(&r.tool))
    {
        outcome.stopped = "incomplete".into();
    }
    outcome
}

fn tool_error_summary(tool: &str, e: &ToolError) -> String {
    match e {
        ToolError::NotGranted { path, .. } | ToolError::Declined(path) | ToolError::Protected(path) => {
            format!("No access to {path}")
        }
        ToolError::NotFound(p) => format!("Not found: {p}"),
        _ => format!("{tool} failed"),
    }
}

fn is_permission_stop(llm_text: &str) -> bool {
    llm_text.starts_with("Path not granted")
        || llm_text.starts_with("The user declined")
        || llm_text.starts_with("Path is protected")
}

#[cfg(test)]
mod path_tests {
    use super::*;

    #[test]
    fn markup_after_a_path_is_cut_off() {
        let home = Path::new("/home/me");
        let raw = r#"/home/me/Downloads")</param><function name="#;
        assert_eq!(
            resolve_path(raw, home, &[]).unwrap(),
            Path::new("/home/me/Downloads")
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::{EngineGatekeeper, builtin_tools};
    use nexus_core::fs::{GrantLevel, GrantScope};
    use nexus_core::sink::VecSink;
    use nexus_fs::{DenyList, MemoryAudit, NewGrant, PermissionEngine};
    use std::collections::VecDeque;
    use std::sync::Mutex;

    struct Scripted(
        Mutex<VecDeque<Option<RouterAction>>>,
        Mutex<usize>,
        Mutex<Vec<Vec<ChatMessage>>>,
    );

    impl Scripted {
        fn new(v: Vec<Option<RouterAction>>) -> Self {
            Self(Mutex::new(v.into()), Mutex::new(0), Mutex::new(Vec::new()))
        }
        fn calls(&self) -> usize {
            *self.1.lock().unwrap()
        }
    }

    #[async_trait]
    impl RouterLlm for Scripted {
        async fn decide(
            &self,
            m: Vec<ChatMessage>,
            _s: &Value,
            _a: &[String],
            _t: u32,
            _c: CancellationToken,
        ) -> Option<RouterAction> {
            self.2.lock().unwrap().push(m);
            *self.1.lock().unwrap() += 1;
            self.0.lock().unwrap().pop_front().flatten()
        }
    }

    fn act(action: &str, path: Option<&str>, query: Option<&str>) -> Option<RouterAction> {
        Some(RouterAction {
            action: action.into(),
            path: path.map(Into::into),
            query: query.map(Into::into),
            expression: None,
            ..RouterAction::default()
        })
    }

    fn setup(grant: bool) -> (tempfile::TempDir, ToolCx) {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("notes.md"), "# Notes\nbuy milk\n").unwrap();
        let e = Arc::new(PermissionEngine::new(
            DenyList::default(),
            Arc::new(MemoryAudit::default()),
        ));
        if grant {
            e.add_grant(NewGrant {
                root: d.path().to_owned(),
                level: GrantLevel::Read,
                recursive: true,
                scope: GrantScope::Session,
                expires_at: None,
                label: None,
            })
            .unwrap();
        }
        (d, ToolCx::new(Arc::new(EngineGatekeeper::new(e, "nexus", None))))
    }

    fn req<'a>(text: &'a str, home: &Path) -> RouterRequest<'a> {
        RouterRequest {
            user_text: text,
            allowed_tools: [
                "read_file",
                "list_dir",
                "search_files",
                "grep_text",
                "calculate",
                "now",
            ]
            .map(String::from)
            .to_vec(),
            max_hops: 3,
            always_route: false,
            home: home.to_owned(),
            previous_answer: String::new(),
        }
    }

    #[test]
    fn heuristic_gate() {
        let roots = vec![PathBuf::from("/Users/me/Projects")];
        assert!(!needs_tools_heuristic("hello, how are you?", &roots, false));
        assert!(!needs_tools_heuristic("explain what a mutex is", &roots, false));
        assert!(needs_tools_heuristic(
            "what's in ~/Downloads/notes.md?",
            &roots,
            false
        ));
        assert!(needs_tools_heuristic("open /etc/hosts", &roots, false));
        assert!(needs_tools_heuristic("summarize report.pdf", &roots, false));
        assert!(needs_tools_heuristic("how many files are there", &roots, false));
        assert!(needs_tools_heuristic("what is in Projects", &roots, false));
        assert!(needs_tools_heuristic("anything", &roots, true));
        assert!(!needs_tools_heuristic("version 3.5 is out", &roots, false));
    }

    #[test]
    fn latest_write_status_survives_a_full_observation_budget() {
        let outcome = RouterOutcome {
            runs: vec![
                ToolRun {
                    tool: "read_file".into(),
                    args_json: "{}".into(),
                    ok: true,
                    summary: "Read big file".into(),
                    llm_text: "data ".repeat(10_000),
                },
                ToolRun {
                    tool: "write_file".into(),
                    args_json: "{}".into(),
                    ok: true,
                    summary: "Created summary (press Apply)".into(),
                    llm_text: "NOT written yet: press Apply".into(),
                },
            ],
            stopped: "answer".into(),
        };
        let results = outcome.as_attachments();
        assert!(
            results.iter().any(|r| r.text.contains("NOT written yet")),
            "an earlier read must not hide the final write status"
        );
        assert!(
            results
                .iter()
                .map(|r| crate::budget::estimate_tokens(&r.text))
                .sum::<u32>()
                <= TOTAL_RESULT_BUDGET
        );
    }

    #[test]
    fn edit_resolution_preserves_exact_whitespace() {
        let old = "    return value;\n";
        let call = resolve_action(
            &RouterAction {
                action: "edit_file".into(),
                path: Some("/project/code.rs".into()),
                old_text: Some(old.into()),
                new_text: Some("    return fixed;\n".into()),
                ..Default::default()
            },
            &["edit_file".into()],
            Path::new("/home/me"),
            &[],
        )
        .unwrap();
        assert_eq!(call.args["old_text"], old);
    }

    #[test]
    fn incomplete_write_decisions_fail_validation_for_reask() {
        let ctx = ValidationCtx {
            allowed_ids: vec!["write_file".into()],
            ..Default::default()
        };
        let mut action = RouterAction {
            action: "write_file".into(),
            path: Some("/project/new.txt".into()),
            ..Default::default()
        };
        let errors = action
            .validate(&ctx)
            .expect_err("missing content must trigger structured reask");
        assert!(errors.iter().any(|e| e.contains("content")));
        action.content = Some(String::new());
        assert!(action.validate(&ctx).is_ok(), "creating an empty file is valid");
    }

    #[test]
    fn arithmetic_and_clock_requests_route_to_their_available_tools() {
        assert!(needs_tools_for(
            "What is 18% of 2450?",
            &[],
            false,
            &["calculate".into()]
        ));
        assert!(needs_tools_for(
            "What time is it now in UTC?",
            &[],
            false,
            &["now".into()]
        ));
        assert!(!needs_tools_for("What is 18% of 2450?", &[], false, &[]));
        assert!(!needs_tools_for(
            "Explain async functions",
            &[],
            false,
            &["calculate".into(), "now".into()]
        ));
    }

    #[test]
    fn schema_lists_answer_and_the_allowed_tools() {
        let s = router_schema(&["read_file".into(), "now".into()]);
        let variants = s["anyOf"].as_array().unwrap();
        let actions: Vec<&str> = variants
            .iter()
            .map(|v| v["properties"]["action"]["enum"][0].as_str().unwrap())
            .collect();
        assert_eq!(actions, ["answer", "read_file", "now"]);
        let validator = jsonschema::validator_for(&router_schema(&["write_file".into()])).unwrap();
        assert!(!validator.is_valid(&json!({"action": "write_file", "path": "/new.txt"})));
        assert!(
            validator.is_valid(&json!({"action": "write_file", "path": "/new.txt", "content": "alpha\n"}))
        );
        assert!(validator.is_valid(&json!({"action": "answer"})));
    }

    #[test]
    fn paths_resolve_against_home_and_granted_roots() {
        let home = Path::new("/Users/me");
        let roots = vec![PathBuf::from("/Users/me/Projects"), PathBuf::from("/data/Media")];
        assert_eq!(
            resolve_path("~/x.txt", home, &roots),
            Some(PathBuf::from("/Users/me/x.txt"))
        );
        assert_eq!(
            resolve_path("/abs/y", home, &roots),
            Some(PathBuf::from("/abs/y"))
        );
        assert_eq!(
            resolve_path("projects", home, &roots),
            Some(PathBuf::from("/Users/me/Projects"))
        );
        assert_eq!(
            resolve_path("media/clips", home, &roots),
            Some(PathBuf::from("/data/Media/clips"))
        );
        assert_eq!(resolve_path("", home, &roots), None);
        assert_eq!(resolve_path("nowhere-at-all", home, &roots), None);
    }

    #[test]
    fn the_router_prompt_teaches_only_the_tools_it_has() {
        let src = internal_source("router").unwrap();
        let render = |write: bool, web: bool| {
            let vars = json!({
                "tools": [], "granted_roots": [], "home_dir": "/h", "request": "save that",
                "tool_results_so_far": 0, "has_write": write, "has_edit": write, "has_move": false,
                "has_web": web, "previous_answer": if write { "print(1)" } else { "" },
            });
            let p = render_full(src, &vars).unwrap();
            format!("{}\n{}", p.system, p.user)
        };
        let both = render(true, true);
        assert!(both.contains("write_file with an absolute path") && both.contains("search_web"));
        assert!(both.contains("Previous answer:\nprint(1)"));
        let none = render(false, false);
        assert!(
            !none.contains("write_file") && !none.contains("search_web") && !none.contains("Previous answer")
        );
    }

    #[test]
    fn saving_and_web_requests_route_only_when_the_tools_exist() {
        let tools = |names: &[&str]| names.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        let write = tools(&["write_file", "read_file"]);
        let web = tools(&["search_web", "read_file"]);
        let read = tools(&["read_file"]);
        assert!(needs_tools_for("save that as notes", &[], false, &write));
        assert!(!needs_tools_for("save that as notes", &[], false, &read));
        assert!(needs_tools_for(
            "what is the latest Rust version?",
            &[],
            false,
            &web
        ));
        assert!(!needs_tools_for(
            "what is the latest Rust version?",
            &[],
            false,
            &write
        ));
        assert!(!needs_tools_for(
            "explain what a mutex is",
            &[],
            false,
            &tools(&["write_file", "search_web"])
        ));
    }

    #[test]
    fn actions_are_validated() {
        let home = Path::new("/h");
        let roots = vec![PathBuf::from("/h/Docs")];
        let allowed: Vec<String> = ["read_file", "grep_text", "calculate"].map(String::from).to_vec();
        let ok = resolve_action(
            &RouterAction {
                action: "read_file".into(),
                path: Some("/h/Docs/a.md".into()),
                ..Default::default()
            },
            &allowed,
            home,
            &roots,
        )
        .unwrap();
        assert_eq!(ok.args["path"], "/h/Docs/a.md");
        assert!(
            resolve_action(
                &RouterAction {
                    action: "list_dir".into(),
                    path: Some("/x".into()),
                    ..Default::default()
                },
                &allowed,
                home,
                &roots
            )
            .is_err()
        );
        assert!(
            resolve_action(
                &RouterAction {
                    action: "read_file".into(),
                    ..Default::default()
                },
                &allowed,
                home,
                &roots
            )
            .is_err()
        );
        // grep without a path defaults to the only granted folder
        let g = resolve_action(
            &RouterAction {
                action: "grep_text".into(),
                query: Some("milk".into()),
                ..Default::default()
            },
            &allowed,
            home,
            &roots,
        )
        .unwrap();
        assert_eq!(g.args["path"], "/h/Docs");
        assert!(
            resolve_action(
                &RouterAction {
                    action: "grep_text".into(),
                    path: Some("/h/Docs".into()),
                    ..Default::default()
                },
                &allowed,
                home,
                &roots
            )
            .is_err()
        );
        assert!(
            resolve_action(
                &RouterAction {
                    action: "calculate".into(),
                    ..Default::default()
                },
                &allowed,
                home,
                &roots
            )
            .is_err()
        );
    }

    #[tokio::test]
    async fn small_talk_skips_the_model_call_entirely() {
        let (d, cx) = setup(true);
        let llm = Scripted::new(vec![]);
        let sink = VecSink::<ChatEvent>::default();
        let o = run_router(
            req("hi there!", d.path()),
            &llm,
            &builtin_tools(),
            &cx,
            &sink,
            CancellationToken::new(),
        )
        .await;
        assert_eq!(o.stopped, "skipped");
        assert_eq!(llm.calls(), 0);
        assert!(sink.events().is_empty());
    }

    #[tokio::test]
    async fn reads_a_file_then_answers() {
        let (d, cx) = setup(true);
        let path = d.path().join("notes.md");
        let llm = Scripted::new(vec![
            act("read_file", Some(path.to_str().unwrap()), None),
            act("answer", None, None),
        ]);
        let sink = VecSink::<ChatEvent>::default();
        let o = run_router(
            req("what's in notes.md?", d.path()),
            &llm,
            &builtin_tools(),
            &cx,
            &sink,
            CancellationToken::new(),
        )
        .await;
        assert_eq!(o.stopped, "answer");
        assert_eq!(o.runs.len(), 1);
        assert!(o.runs[0].ok && o.runs[0].llm_text.contains("buy milk"));
        let decisions = llm.2.lock().unwrap();
        let next_input = serde_json::to_string(&decisions[1]).unwrap();
        assert!(
            next_input.contains("buy milk"),
            "next tool decision must see the file contents"
        );
        let ev = sink.events();
        assert!(matches!(&ev[0], ChatEvent::ToolCall { tool, .. } if tool == "read_file"));
        assert!(
            matches!(&ev[1], ChatEvent::ToolResult { summary, ok: true, .. } if summary == "Read notes.md")
        );
        let att = o.as_attachments();
        assert_eq!(att.len(), 1);
        assert!(att[0].text.contains("buy milk"));
    }

    struct CaptureProposal(Mutex<Vec<nexus_core::changes::FileOp>>);

    impl crate::tools::ProposalSink for CaptureProposal {
        fn propose(&self, _title: &str, ops: Vec<nexus_core::changes::FileOp>) -> Result<String, String> {
            self.0.lock().map_err(|e| e.to_string())?.extend(ops);
            Ok("proposal-test".into())
        }
    }

    struct ReadThenWrite;

    #[async_trait]
    impl RouterLlm for ReadThenWrite {
        async fn decide(
            &self,
            messages: Vec<ChatMessage>,
            _schema: &Value,
            _allowed: &[String],
            _max_tokens: u32,
            _cancel: CancellationToken,
        ) -> Option<RouterAction> {
            let input = &messages.last().unwrap().content;
            if input.contains("NOT written yet") {
                return Some(RouterAction::answer());
            }
            if input.contains("buy milk") {
                return Some(RouterAction {
                    action: "write_file".into(),
                    path: Some("summary.md".into()),
                    content: Some("Shopping: buy milk\n".into()),
                    ..Default::default()
                });
            }
            act("read_file", Some("notes.md"), None)
        }
    }

    #[tokio::test]
    async fn read_then_create_proposal_applies_through_the_journal() {
        let (d, _) = setup(false);
        let engine = Arc::new(PermissionEngine::new(
            DenyList::default(),
            Arc::new(MemoryAudit::default()),
        ));
        engine
            .add_grant(NewGrant {
                root: d.path().to_owned(),
                level: GrantLevel::Full,
                recursive: true,
                scope: GrantScope::Session,
                expires_at: None,
                label: None,
            })
            .unwrap();
        let proposals = Arc::new(CaptureProposal(Mutex::new(Vec::new())));
        let mut cx = ToolCx::new(Arc::new(EngineGatekeeper::new(engine.clone(), "agent", None)));
        cx.proposals = Some(proposals.clone());
        let mut request = req("Read notes.md and create summary.md", d.path());
        request.allowed_tools.push("write_file".into());
        let sink = VecSink::<ChatEvent>::default();
        let outcome = run_router(
            request,
            &ReadThenWrite,
            &builtin_tools(),
            &cx,
            &sink,
            CancellationToken::new(),
        )
        .await;
        assert_eq!(outcome.stopped, "answer");
        assert_eq!(outcome.runs.len(), 2);
        assert!(outcome.runs.iter().all(|r| r.ok));
        let target = d.path().join("summary.md");
        assert!(!target.exists(), "a proposal must wait for Apply");
        let changes = nexus_core::changes::ChangeSet {
            id: nexus_core::ids::ChangeSetId::new(),
            title: "Create summary".into(),
            origin: "agent".into(),
            ops: proposals.0.lock().unwrap().clone(),
            created_at: 0,
        };
        assert_eq!(changes.ops.len(), 1);
        let journal_dir = tempfile::tempdir().unwrap();
        let journal = nexus_fs::Journal::new(
            journal_dir.path(),
            Arc::new(nexus_fs::MoveToDirTrash::new(journal_dir.path().join("trash"))),
        );
        journal.apply(&engine, "agent", &changes).unwrap();
        assert_eq!(std::fs::read_to_string(target).unwrap(), "Shopping: buy milk\n");
        assert!(sink.events().iter().any(|e| matches!(e,
            ChatEvent::ToolCall { tool, .. } if tool == "report")));
    }

    struct SpecialistResult;

    #[async_trait]
    impl crate::tools::ConsultationSource for SpecialistResult {
        async fn consult(
            &self,
            assistant: &str,
            task: &str,
            _cancel: CancellationToken,
        ) -> Result<String, String> {
            assert_eq!(assistant, "reviewer");
            assert_eq!(task, "Check the supplied code");
            Ok("Review finding: handle the empty input".into())
        }
    }

    #[tokio::test]
    async fn a_specialist_result_reaches_the_parent_decision_and_tool_trace() {
        let (d, mut cx) = setup(false);
        cx.consultations = Some(Arc::new(SpecialistResult));
        let llm = Scripted::new(vec![
            Some(RouterAction {
                action: "ask_assistant".into(),
                assistant: Some("reviewer".into()),
                task: Some("Check the supplied code".into()),
                ..Default::default()
            }),
            Some(RouterAction::answer()),
        ]);
        let mut request = req("Review this supplied code", d.path());
        request.allowed_tools = vec!["ask_assistant".into()];
        let sink = VecSink::<ChatEvent>::default();
        let outcome = run_router(
            request,
            &llm,
            &builtin_tools(),
            &cx,
            &sink,
            CancellationToken::new(),
        )
        .await;
        assert_eq!(outcome.stopped, "answer");
        assert_eq!(outcome.runs.len(), 1);
        assert!(outcome.runs[0].ok);
        let decisions = llm.2.lock().unwrap();
        assert!(
            decisions[1]
                .last()
                .unwrap()
                .content
                .contains("handle the empty input")
        );
        assert!(sink.events().iter().any(|e| matches!(e,
            ChatEvent::ToolResult { tool, ok: true, .. } if tool == "ask_assistant")));
    }

    #[tokio::test]
    async fn the_same_call_twice_breaks_the_loop() {
        let (d, cx) = setup(true);
        let p = d.path().to_str().unwrap();
        let llm = Scripted::new(vec![
            act("list_dir", Some(p), None),
            act("list_dir", Some(p), None),
            act("list_dir", Some(p), None),
        ]);
        let o = run_router(
            req("list the folder", d.path()),
            &llm,
            &builtin_tools(),
            &cx,
            &VecSink::<ChatEvent>::default(),
            CancellationToken::new(),
        )
        .await;
        assert_eq!(o.stopped, "repeat");
        assert_eq!(o.runs.len(), 1);
    }

    #[tokio::test]
    async fn hops_are_bounded() {
        let (d, cx) = setup(true);
        let p = d.path().to_str().unwrap();
        let llm = Scripted::new(vec![
            act("list_dir", Some(p), None),
            act("file_info", Some(p), None),
            act("grep_text", Some(p), Some("milk")),
            act("search_files", Some(p), Some("md")),
        ]);
        let mut r = req("find things in the folder", d.path());
        r.allowed_tools.push("file_info".into());
        let o = run_router(
            r,
            &llm,
            &builtin_tools(),
            &cx,
            &VecSink::<ChatEvent>::default(),
            CancellationToken::new(),
        )
        .await;
        assert_eq!(o.stopped, "max_hops");
        assert_eq!(o.runs.len(), 3);
        assert_eq!(llm.calls(), 3);
    }

    struct VerbatimSpecialist(Mutex<Option<String>>);

    #[async_trait]
    impl crate::tools::ConsultationSource for VerbatimSpecialist {
        async fn consult(
            &self,
            _assistant: &str,
            task: &str,
            _cancel: CancellationToken,
        ) -> Result<String, String> {
            *self.0.lock().unwrap() = Some(task.into());
            Ok("Concrete finding from the supplied code".into())
        }
    }

    #[tokio::test]
    async fn explicit_consultations_forward_the_users_task_without_rewriting_it() {
        let (d, mut cx) = setup(false);
        let source = Arc::new(VerbatimSpecialist(Mutex::new(None)));
        cx.consultations = Some(source.clone());
        let text = "Ask Reviewer to review this exact code: def avg(xs): return sum(xs)/len(xs)";
        let mut request = req(text, d.path());
        request.allowed_tools = vec!["ask_assistant".into()];
        let llm = Scripted::new(vec![]);
        let outcome = run_router(
            request,
            &llm,
            &builtin_tools(),
            &cx,
            &VecSink::<ChatEvent>::default(),
            CancellationToken::new(),
        )
        .await;
        assert_eq!(source.0.lock().unwrap().as_deref(), Some(text));
        assert!(outcome.runs[0].ok);
        assert_eq!(
            llm.calls(),
            0,
            "no parent task rewrite or invented follow-up step"
        );
        assert!(!simple_consultation("Explain how to ask Reviewer for feedback"));
        assert!(!simple_consultation(
            "Ask Reviewer to review, then create report.md"
        ));
    }

    #[tokio::test]
    async fn clock_queries_observe_the_real_clock_before_answering() {
        let (d, cx) = setup(false);
        let llm = Scripted::new(vec![Some(RouterAction::answer())]);
        let mut request = req("What is the current time in UTC?", d.path());
        request.allowed_tools = vec!["now".into()];
        let outcome = run_router(
            request,
            &llm,
            &builtin_tools(),
            &cx,
            &VecSink::<ChatEvent>::default(),
            CancellationToken::new(),
        )
        .await;
        assert_eq!(outcome.runs.len(), 1);
        assert_eq!(outcome.runs[0].tool, "now");
        assert!(outcome.runs[0].ok && outcome.runs[0].llm_text.contains("UTC"));
        assert_eq!(llm.calls(), 1, "clock lookup itself needs no model decision");
    }

    #[tokio::test]
    async fn a_repeated_read_before_an_edit_gets_a_completion_correction() {
        let (d, cx) = setup(true);
        let path = d.path().join("notes.md").display().to_string();
        let llm = Scripted::new(vec![
            act("read_file", Some(&path), None),
            act("read_file", Some(&path), None),
            Some(RouterAction {
                action: "write_file".into(),
                path: Some(path.clone()),
                content: Some("new notes".into()),
                ..Default::default()
            }),
            Some(RouterAction::answer()),
        ]);
        let mut request = req("Read notes.md and change the text", d.path());
        request.allowed_tools = vec!["read_file".into(), "write_file".into()];
        let sink = VecSink::<ChatEvent>::default();
        run_router(
            request,
            &llm,
            &builtin_tools(),
            &cx,
            &sink,
            CancellationToken::new(),
        )
        .await;
        assert!(
            sink.events()
                .iter()
                .any(|e| matches!(e, ChatEvent::ToolCall { tool, .. } if tool == "write_file")),
            "a repeated observation must not silently abandon an explicit pending edit"
        );
        assert_eq!(
            sink.events()
                .iter()
                .filter(|e| matches!(e, ChatEvent::ToolCall { tool, .. } if tool == "read_file"))
                .count(),
            1,
            "the repeated read itself must not execute twice"
        );
    }

    #[tokio::test]
    async fn a_premature_file_completion_gets_one_bounded_correction() {
        let (d, cx) = setup(true);
        let target = d.path().join("new.txt");
        let llm = Scripted::new(vec![
            Some(RouterAction::answer()),
            Some(RouterAction {
                action: "write_file".into(),
                path: Some(target.display().to_string()),
                content: Some("hello".into()),
                ..Default::default()
            }),
            Some(RouterAction::answer()),
        ]);
        let mut request = req("Create new.txt containing hello", d.path());
        request.allowed_tools = vec!["write_file".into()];
        let sink = VecSink::<ChatEvent>::default();
        let out = run_router(
            request,
            &llm,
            &builtin_tools(),
            &cx,
            &sink,
            CancellationToken::new(),
        )
        .await;
        assert!(
            sink.events()
                .iter()
                .any(|e| matches!(e, ChatEvent::ToolCall { tool, .. } if tool == "write_file"))
        );
        assert_eq!(
            out.stopped, "incomplete",
            "no proposal backend means no successful change"
        );
        assert_eq!(llm.calls(), 3, "one correction; no unbounded retry loop");
        assert!(!target.exists());
        assert!(!file_change_requested(
            "Explain how to edit new.txt",
            &["edit_file".into()]
        ));
        assert!(!file_change_requested(
            "Write a poem about spring",
            &["write_file".into()]
        ));
    }

    #[tokio::test]
    async fn a_model_failure_falls_back_to_answer() {
        let (d, cx) = setup(true);
        let llm = Scripted::new(vec![None]);
        let o = run_router(
            req("read notes.md", d.path()),
            &llm,
            &builtin_tools(),
            &cx,
            &VecSink::<ChatEvent>::default(),
            CancellationToken::new(),
        )
        .await;
        assert_eq!(o.stopped, "answer");
        assert_eq!(o.runs.len(), 1);
        assert!(!o.runs[0].ok);
        assert!(o.as_attachments()[0].text.contains("No action was executed"));
    }

    #[tokio::test]
    async fn disallowed_tools_and_bad_paths_stop_the_loop() {
        let (d, cx) = setup(true);
        let llm = Scripted::new(vec![act("scan_folder", Some("/x"), None)]);
        let o = run_router(
            req("scan my folder", d.path()),
            &llm,
            &builtin_tools(),
            &cx,
            &VecSink::<ChatEvent>::default(),
            CancellationToken::new(),
        )
        .await;
        assert_eq!(o.stopped, "invalid");
        assert_eq!(o.runs.len(), 1);
        assert!(!o.runs[0].ok);
        assert!(o.as_attachments()[0].text.contains("not an allowed tool"));
    }

    #[tokio::test]
    async fn an_ungranted_folder_is_reported_to_the_model_not_invented() {
        let (d, cx) = setup(false);
        let p = d.path().join("notes.md");
        let llm = Scripted::new(vec![
            act("read_file", Some(p.to_str().unwrap()), None),
            act("read_file", Some(p.to_str().unwrap()), None),
        ]);
        let sink = VecSink::<ChatEvent>::default();
        let o = run_router(
            req("read notes.md", d.path()),
            &llm,
            &builtin_tools(),
            &cx,
            &sink,
            CancellationToken::new(),
        )
        .await;
        assert_eq!(o.runs.len(), 1);
        assert!(!o.runs[0].ok);
        assert!(o.runs[0].llm_text.contains("Path not granted"));
        assert_eq!(o.stopped, "answer");
        assert_eq!(llm.calls(), 1, "no retry after a permission failure");
        let att = o.as_attachments();
        assert!(att[0].text.contains("error=\"true\"") && att[0].text.contains("Ask the user to grant"));
        assert!(matches!(
            sink.events().last(),
            Some(ChatEvent::ToolResult { ok: false, .. })
        ));
    }

    #[tokio::test]
    async fn cancellation_stops_before_asking() {
        let (d, cx) = setup(true);
        let llm = Scripted::new(vec![act("list_dir", Some(d.path().to_str().unwrap()), None)]);
        let cancel = CancellationToken::new();
        cancel.cancel();
        let o = run_router(
            req("list files", d.path()),
            &llm,
            &builtin_tools(),
            &cx,
            &VecSink::<ChatEvent>::default(),
            cancel,
        )
        .await;
        assert_eq!(o.stopped, "cancelled");
        assert_eq!(llm.calls(), 0);
    }
}
