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
    pub path: Option<String>,
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub expression: Option<String>,
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
        // `allowed_ids` carries the allowed actions.
        if ctx.allowed_ids.iter().any(|a| a == &self.action) {
            Ok(())
        } else {
            Err(vec![format!(
                "`{}` is not an allowed action; use one of: {}",
                self.action,
                ctx.allowed_ids.join(", ")
            )])
        }
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
        cancel: CancellationToken,
    ) -> Option<RouterAction> {
        let spec = StructuredSpec {
            name: "router_action",
            schema,
            temperature: 0.2,
            max_tokens: 120,
        };
        let vctx = ValidationCtx {
            allowed_ids: allowed.to_vec(),
            ..ValidationCtx::default()
        };
        match chat_json::<RouterAction>(&self.0, Priority::Interactive, messages, spec, &vctx, cancel).await {
            Ok(Structured::Valid(a)) => Some(a),
            _ => None,
        }
    }
}

/// The action schema for `allowed` tools plus `answer` (flat on purpose, ASSISTANTS §3.2).
pub fn router_schema(allowed_tools: &[String]) -> Value {
    let mut actions = vec!["answer".to_owned()];
    actions.extend(allowed_tools.iter().cloned());
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["action"],
        "properties": {
            "action": {"type": "string", "enum": actions},
            "path": {"type": "string", "maxLength": 1024},
            "query": {"type": "string", "maxLength": 200},
            "expression": {"type": "string", "maxLength": 200}
        }
    })
}

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
        "doc_search" | "kb_search" => {
            let q = clean(&a.query).ok_or("a query is required")?;
            args.insert("query".into(), Value::String(q.to_owned()));
        }
        "calculate" => {
            let e = clean(&a.expression).ok_or("an expression is required")?;
            args.insert("expression".into(), Value::String(e.to_owned()));
        }
        "now" => {}
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
    /// Why the loop ended: `skipped`, `answer`, `repeat`, `invalid`, `max_hops`, `cancelled`.
    pub stopped: String,
}

impl RouterOutcome {
    /// Tool results as attachment blocks for the turn engine (one per successful or failed call).
    pub fn as_attachments(&self) -> Vec<AttachmentBlock> {
        let mut left = TOTAL_RESULT_BUDGET;
        let mut out = Vec::new();
        for r in &self.runs {
            let text = if r.ok {
                r.llm_text.clone()
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
    if req.allowed_tools.is_empty() || !needs_tools_heuristic(req.user_text, &roots, req.always_route) {
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
    let mut allowed_actions = vec!["answer".to_owned()];
    allowed_actions.extend(req.allowed_tools.iter().cloned());
    let schema = router_schema(&req.allowed_tools);
    let mut seen = HashSet::new();
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
            for r in &outcome.runs {
                user.push_str(&format!(
                    "- {} ({})\n",
                    r.summary,
                    if r.ok { "ok" } else { "failed" }
                ));
            }
        }
        msgs.push(ChatMessage::user(user));

        let decision = llm
            .decide(msgs, &schema, &allowed_actions, cancel.clone())
            .await
            .unwrap_or_else(RouterAction::answer);
        if decision.action == "answer" {
            outcome.stopped = "answer".into();
            break;
        }
        let call = match resolve_action(&decision, &req.allowed_tools, &req.home, &roots) {
            Ok(c) => c,
            Err(reason) => {
                tracing::debug!(%reason, "router decision rejected");
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
            gate: cx.gate.clone(),
            reason: cx.reason.clone(),
            budget_tokens: cx.budget_tokens,
            docs: cx.docs.clone(),
            kb: cx.kb.clone(),
        };
        cx_run.reason = format!("to run {}", call.tool);
        let run = match tool.run(call.args, &cx_run).await {
            Ok(o) => ToolRun {
                tool: call.tool.clone(),
                args_json,
                ok: true,
                summary: o.summary,
                llm_text: o.llm_text,
            },
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
        outcome.runs.push(run);
        if stop_after {
            // A declined or ungranted folder: answer now so the assistant can explain, instead of retrying.
            outcome.stopped = "answer".into();
            break;
        }
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

    struct Scripted(Mutex<VecDeque<Option<RouterAction>>>, Mutex<usize>);

    impl Scripted {
        fn new(v: Vec<Option<RouterAction>>) -> Self {
            Self(Mutex::new(v.into()), Mutex::new(0))
        }
        fn calls(&self) -> usize {
            *self.1.lock().unwrap()
        }
    }

    #[async_trait]
    impl RouterLlm for Scripted {
        async fn decide(
            &self,
            _m: Vec<ChatMessage>,
            _s: &Value,
            _a: &[String],
            _c: CancellationToken,
        ) -> Option<RouterAction> {
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
    fn schema_lists_answer_and_the_allowed_tools() {
        let s = router_schema(&["read_file".into(), "now".into()]);
        assert_eq!(
            s["properties"]["action"]["enum"],
            json!(["answer", "read_file", "now"])
        );
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
        let ev = sink.events();
        assert!(matches!(&ev[0], ChatEvent::ToolCall { tool, .. } if tool == "read_file"));
        assert!(
            matches!(&ev[1], ChatEvent::ToolResult { summary, ok: true, .. } if summary == "Read notes.md")
        );
        let att = o.as_attachments();
        assert_eq!(att.len(), 1);
        assert!(att[0].text.contains("buy milk"));
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
        assert!(o.runs.is_empty());
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
        assert!(o.runs.is_empty());
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
