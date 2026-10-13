//! Tools the router can call (ASSISTANTS §5). The model never writes a file itself (ADR-005, ADR-016):
//! `write_file`, `edit_file`, `move_file` and `trash_file` only *propose* a change set that the user
//! previews and applies. Every file access goes through a [`Gatekeeper`], which checks the permission
//! engine and, when a grant is missing, asks the user. The web tools run only when the user turned
//! web access on (ADR-013, ADR-016).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::UNIX_EPOCH;

use async_trait::async_trait;
use nexus_core::changes::{FileOp, TextEdit};
use nexus_core::fs::Access;
use nexus_fs::PermissionEngine;
use serde::Serialize;
use serde_json::{Map, Value};

use crate::budget::estimate_tokens;
use crate::pipelines::doc::{DocIndex, search_lines};

/// Tool arguments as the router validated them (`path`, `query`, `expression`).
pub type ToolArgs = Map<String, Value>;

/// Default budget for one tool's text (tokens).
pub const DEFAULT_TOOL_BUDGET: u32 = 1200;
/// Largest file `read_file` and `grep_text` open.
pub const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
/// Most directory entries a walk visits before giving up.
const MAX_WALK_ENTRIES: usize = 20_000;

/// Why a tool failed. Messages are written for the model ("Ask the user to grant access.").
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("Path not granted: {path}. Ask the user to grant access to this folder.")]
    NotGranted { path: String, needed: Access },
    #[error("Path is protected and can never be read: {0}.")]
    Protected(String),
    #[error("The user declined access to {0}.")]
    Declined(String),
    #[error("Not found: {0}.")]
    NotFound(String),
    #[error("{0}")]
    Invalid(String),
    #[error("{0} is not available yet.")]
    Unavailable(&'static str),
    #[error("Could not read {path}: {message}")]
    Io { path: String, message: String },
}

impl ToolError {
    /// The model-facing text of a failed call.
    pub fn for_model(&self) -> String {
        self.to_string()
    }
}

/// What a tool returns.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ToolOutput {
    /// Already budgeted; goes into `<tool_result>`.
    pub llm_text: String,
    /// Structured data for the UI chip drawer.
    pub ui: Option<Value>,
    /// Chip text: "Read notes.md", "Listed Downloads (42 items)".
    pub summary: String,
}

/// The user's answer to a permission prompt raised from inside a tool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AskOutcome {
    Allowed,
    Denied,
}

/// Raises the permission prompt (the app implements it with the broker; the grant already exists when
/// it returns `Allowed`).
#[async_trait]
pub trait PermissionAsker: Send + Sync {
    async fn ask(&self, assistant: &str, path: &Path, needed: Access, reason: &str) -> AskOutcome;
}

/// Authorizes file access for tools.
#[async_trait]
pub trait Gatekeeper: Send + Sync {
    /// Canonical path to use if `access` on `path` is allowed (asking the user if a grant is missing).
    async fn authorize(&self, path: &Path, access: Access, reason: &str) -> Result<PathBuf, ToolError>;
    /// Unaudited: is `path` readable? Used to skip protected children while walking an authorized root.
    fn can_read(&self, path: &Path) -> bool;
    /// Granted folders (for path resolution and prompts).
    fn granted_roots(&self) -> Vec<PathBuf>;
}

/// [`Gatekeeper`] backed by the real permission engine.
pub struct EngineGatekeeper {
    engine: Arc<PermissionEngine>,
    actor: String,
    asker: Option<Arc<dyn PermissionAsker>>,
}

impl EngineGatekeeper {
    pub fn new(
        engine: Arc<PermissionEngine>,
        actor: impl Into<String>,
        asker: Option<Arc<dyn PermissionAsker>>,
    ) -> Self {
        Self {
            engine,
            actor: actor.into(),
            asker,
        }
    }
}

#[async_trait]
impl Gatekeeper for EngineGatekeeper {
    async fn authorize(&self, path: &Path, access: Access, reason: &str) -> Result<PathBuf, ToolError> {
        use nexus_fs::FsError;
        match self.engine.check(&self.actor, path, access) {
            Ok(c) => Ok(c.into_path_buf()),
            Err(FsError::Denied { path, .. }) => Err(ToolError::Protected(path.display().to_string())),
            Err(FsError::BadPath(e)) => Err(ToolError::NotFound(format!("{} ({e})", path.display()))),
            Err(FsError::NoGrant { path: p, .. }) => {
                let shown = p.display().to_string();
                let Some(asker) = &self.asker else {
                    return Err(ToolError::NotGranted {
                        path: shown,
                        needed: access,
                    });
                };
                match asker.ask(&self.actor, &p, access, reason).await {
                    AskOutcome::Denied => Err(ToolError::Declined(shown)),
                    AskOutcome::Allowed => self
                        .engine
                        .check(&self.actor, path, access)
                        .map(|c| c.into_path_buf())
                        .map_err(|_| ToolError::NotGranted {
                            path: shown,
                            needed: access,
                        }),
                }
            }
        }
    }

    fn can_read(&self, path: &Path) -> bool {
        self.engine.probe(path, Access::Read)
    }

    fn granted_roots(&self) -> Vec<PathBuf> {
        self.engine
            .grants()
            .into_iter()
            .map(|g| PathBuf::from(g.root))
            .collect()
    }
}

/// Everything a tool run needs.
pub struct ToolCx {
    pub gate: Arc<dyn Gatekeeper>,
    /// Shown in permission prompts: "to read notes.md".
    pub reason: String,
    pub budget_tokens: u32,
    /// Documents attached to this chat, searchable by `doc_search`.
    pub docs: Vec<Arc<DocIndex>>,
    /// Offline knowledge libraries behind `kb_search` (M17). `None` when no library is enabled.
    pub kb: Option<Arc<dyn KnowledgeSource>>,
    /// The indexed code project behind `find_symbol` (M09). `None` until a project is indexed.
    pub code: Option<Arc<dyn CodeSource>>,
    /// Where `write_file` and friends stage their change sets. `None` = proposing is not available.
    pub proposals: Option<Arc<dyn ProposalSink>>,
    /// Web access behind `search_web` and `fetch_url`. `None` = web access is off.
    pub web: Option<Arc<dyn WebSource>>,
}

/// Stores a change set the model proposed. Nothing touches disk until the user applies it (ADR-005).
pub trait ProposalSink: Send + Sync {
    /// Stage `ops` under `title`; returns the change set id.
    fn propose(&self, title: &str, ops: Vec<FileOp>) -> Result<String, String>;
    /// Apply the just-staged change set when the user chose to apply proposals automatically.
    /// The write still goes through the journaled, undoable apply; the default never applies.
    fn auto_apply(&self, _id: &str) -> AutoApply {
        AutoApply::Off
    }
}

/// What happened when a proposal was offered for automatic apply.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AutoApply {
    /// Automatic apply is off: the user applies the change from the chat card or Changes.
    Off,
    /// The change was written to disk.
    Applied,
    /// The apply was refused or failed; nothing was written and the change stays staged.
    Failed(String),
}

/// One web search result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WebHit {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

/// A fetched page, already reduced to plain text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WebPage {
    pub url: String,
    pub title: String,
    pub text: String,
}

/// Implemented by the app over `nexus-models::web` (keeps this crate free of network code).
#[async_trait]
pub trait WebSource: Send + Sync {
    async fn search(&self, query: &str, max_results: usize) -> Result<Vec<WebHit>, String>;
    async fn fetch(&self, url: &str, max_chars: usize) -> Result<WebPage, String>;
}

/// Implemented by the app over the Rename Forge index (keeps this crate free of the project store).
pub trait CodeSource: Send + Sync {
    /// Symbols, files and folders matching `query` with their usages, as text of at most `max_chars`.
    fn find(&self, query: &str, max_chars: usize) -> Result<String, String>;
}

/// Result of a library search: the numbered `<doc>` context block and how many passages it holds.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KnowledgeHits {
    /// `<doc n="1" source="..." page="..">...</doc>` blocks with the grounding rules; empty when nothing matched.
    pub context: String,
    pub passages: usize,
}

/// Implemented by the app over `nexus-knowledge` (keeps this crate free of the knowledge store).
pub trait KnowledgeSource: Send + Sync {
    /// Search the enabled libraries (optionally one, by id or name). Passages are numbered from 1 per call.
    fn search(&self, query: &str, library: Option<&str>, max_tokens: u32) -> Result<KnowledgeHits, String>;
}

impl ToolCx {
    pub fn new(gate: Arc<dyn Gatekeeper>) -> Self {
        Self {
            gate,
            reason: String::new(),
            budget_tokens: DEFAULT_TOOL_BUDGET,
            docs: Vec::new(),
            kb: None,
            code: None,
            proposals: None,
            web: None,
        }
    }
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;
    /// One line, shown in the router prompt.
    fn description(&self) -> &'static str;
    /// e.g. `read_file(path)`.
    fn signature(&self) -> &'static str;
    /// Paths and access the tool will need (so the router can resolve them first).
    fn required_access(&self, args: &ToolArgs) -> Vec<(PathBuf, Access)>;
    async fn run(&self, args: ToolArgs, cx: &ToolCx) -> Result<ToolOutput, ToolError>;
}

// ---- helpers ------------------------------------------------------------------------------------

fn arg_str<'a>(args: &'a ToolArgs, key: &str) -> Result<&'a str, ToolError> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| ToolError::Invalid(format!("Missing argument `{key}`.")))
}

fn arg_path(args: &ToolArgs) -> Result<PathBuf, ToolError> {
    Ok(PathBuf::from(arg_str(args, "path")?))
}

fn io_err(path: &Path, e: std::io::Error) -> ToolError {
    if e.kind() == std::io::ErrorKind::NotFound {
        ToolError::NotFound(path.display().to_string())
    } else {
        ToolError::Io {
            path: path.display().to_string(),
            message: e.to_string(),
        }
    }
}

fn base(p: &Path) -> String {
    p.file_name()
        .map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into_owned())
}

/// Largest prefix of `text` within `tokens` (estimated), cut on a line boundary when possible.
pub fn truncate_to_tokens(text: &str, tokens: u32) -> (String, bool) {
    if estimate_tokens(text) <= tokens {
        return (text.to_owned(), false);
    }
    let max_chars = (f64::from(tokens) * 3.2) as usize;
    let cut: String = text.chars().take(max_chars).collect();
    let cut = match cut.rfind('\n') {
        Some(i) if i > cut.len() / 2 => cut[..i].to_owned(),
        _ => cut,
    };
    (cut, true)
}

fn is_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(4096).any(|b| *b == 0)
}

fn skip_dir(name: &str) -> bool {
    name.starts_with('.')
        || matches!(
            name,
            "node_modules" | "target" | "__pycache__" | "venv" | "dist" | "build"
        )
}

fn modified_ms(meta: &std::fs::Metadata) -> Option<i64> {
    meta.modified()
        .ok()
        .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
}

// ---- read_file ----------------------------------------------------------------------------------

pub struct ReadFile;

#[async_trait]
impl Tool for ReadFile {
    fn name(&self) -> &'static str {
        "read_file"
    }
    fn description(&self) -> &'static str {
        "Read the text of one file."
    }
    fn signature(&self) -> &'static str {
        "read_file(path)"
    }
    fn required_access(&self, args: &ToolArgs) -> Vec<(PathBuf, Access)> {
        arg_path(args)
            .map(|p| vec![(p, Access::Read)])
            .unwrap_or_default()
    }
    async fn run(&self, args: ToolArgs, cx: &ToolCx) -> Result<ToolOutput, ToolError> {
        let given = arg_path(&args)?;
        let path = cx
            .gate
            .authorize(&given, Access::Read, &format!("to read {}", base(&given)))
            .await?;
        let meta = std::fs::metadata(&path).map_err(|e| io_err(&path, e))?;
        if !meta.is_file() {
            return Err(ToolError::Invalid(format!(
                "{} is a folder; use list_dir.",
                path.display()
            )));
        }
        if meta.len() > MAX_FILE_BYTES {
            return Err(ToolError::Invalid(format!(
                "{} is larger than {} MB; use grep_text to search inside it.",
                base(&path),
                MAX_FILE_BYTES / 1024 / 1024
            )));
        }
        let bytes = std::fs::read(&path).map_err(|e| io_err(&path, e))?;
        if is_binary(&bytes) {
            return Err(ToolError::Invalid(format!(
                "{} is a binary file, not text.",
                base(&path)
            )));
        }
        let text = String::from_utf8_lossy(&bytes);
        let total_lines = text.lines().count();
        let (body, truncated) = truncate_to_tokens(&text, cx.budget_tokens);
        let shown_lines = body.lines().count();
        let note = if truncated {
            format!(
                "\n[truncated: {} more lines]",
                total_lines.saturating_sub(shown_lines)
            )
        } else {
            String::new()
        };
        Ok(ToolOutput {
            llm_text: format!("<file path=\"{}\">\n{body}{note}\n</file>", path.display()),
            ui: Some(
                serde_json::json!({"path": path.display().to_string(), "lines": total_lines, "truncated": truncated}),
            ),
            summary: format!("Read {}", base(&path)),
        })
    }
}

// ---- list_dir -----------------------------------------------------------------------------------

pub struct ListDir;

#[async_trait]
impl Tool for ListDir {
    fn name(&self) -> &'static str {
        "list_dir"
    }
    fn description(&self) -> &'static str {
        "List the files and folders directly inside a folder."
    }
    fn signature(&self) -> &'static str {
        "list_dir(path)"
    }
    fn required_access(&self, args: &ToolArgs) -> Vec<(PathBuf, Access)> {
        arg_path(args)
            .map(|p| vec![(p, Access::Read)])
            .unwrap_or_default()
    }
    async fn run(&self, args: ToolArgs, cx: &ToolCx) -> Result<ToolOutput, ToolError> {
        let given = arg_path(&args)?;
        let path = cx
            .gate
            .authorize(&given, Access::Read, &format!("to list {}", base(&given)))
            .await?;
        let mut entries: Vec<(bool, String, u64)> = Vec::new();
        for e in std::fs::read_dir(&path).map_err(|e| io_err(&path, e))? {
            let Ok(e) = e else { continue };
            let Ok(meta) = e.metadata() else { continue };
            entries.push((
                meta.is_dir(),
                e.file_name().to_string_lossy().into_owned(),
                if meta.is_dir() { 0 } else { meta.len() },
            ));
        }
        entries.sort_by(|a, b| {
            b.0.cmp(&a.0)
                .then_with(|| a.1.to_lowercase().cmp(&b.1.to_lowercase()))
        });
        let total = entries.len();
        let mut out = String::new();
        let mut shown = 0usize;
        for (is_dir, name, size) in &entries {
            let line = if *is_dir {
                format!("{name}/\n")
            } else {
                format!("{name} ({})\n", human_size(*size))
            };
            if estimate_tokens(&out) + estimate_tokens(&line) > cx.budget_tokens {
                break;
            }
            out.push_str(&line);
            shown += 1;
        }
        if shown < total {
            out.push_str(&format!("[truncated: {} more entries]\n", total - shown));
        }
        if total == 0 {
            out.push_str("(empty folder)\n");
        }
        Ok(ToolOutput {
            llm_text: format!(
                "<tool_result name=\"list_dir\" path=\"{}\">\n{out}</tool_result>",
                path.display()
            ),
            ui: Some(serde_json::json!({"path": path.display().to_string(), "items": total})),
            summary: format!("Listed {} ({total} items)", base(&path)),
        })
    }
}

fn human_size(b: u64) -> String {
    const U: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{b} B")
    } else {
        format!("{v:.1} {}", U[i])
    }
}

// ---- search_files -------------------------------------------------------------------------------

pub struct SearchFiles;

/// Case-insensitive match of `pattern` (substring, or `*` wildcards) against a file name.
fn name_matches(name: &str, pattern: &str) -> bool {
    let (name, pattern) = (name.to_lowercase(), pattern.to_lowercase());
    if !pattern.contains('*') {
        return name.contains(&pattern);
    }
    let parts: Vec<&str> = pattern.split('*').collect();
    let mut rest = name.as_str();
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        match rest.find(part) {
            Some(at) if i > 0 || !pattern.starts_with(part) || at == 0 => rest = &rest[at + part.len()..],
            _ => return false,
        }
    }
    !(parts.last().is_some_and(|l| !l.is_empty())
        && !rest.is_empty()
        && !name.ends_with(parts.last().copied().unwrap_or("")))
}

/// Walk `root` (not following symlinks), skipping hidden/build folders and anything the gate refuses.
fn walk_files(gate: &dyn Gatekeeper, root: &Path, max_depth: usize) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![(root.to_owned(), 0usize)];
    let mut visited = 0usize;
    while let Some((dir, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            visited += 1;
            if visited > MAX_WALK_ENTRIES {
                return out;
            }
            let Ok(ft) = e.file_type() else { continue };
            let p = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            if ft.is_symlink() {
                continue;
            }
            if ft.is_dir() {
                if depth < max_depth && !skip_dir(&name) && gate.can_read(&p) {
                    stack.push((p, depth + 1));
                }
            } else if ft.is_file() && gate.can_read(&p) {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

#[async_trait]
impl Tool for SearchFiles {
    fn name(&self) -> &'static str {
        "search_files"
    }
    fn description(&self) -> &'static str {
        "Find files by name (part of the name, or a pattern like *.pdf) inside a folder and its subfolders."
    }
    fn signature(&self) -> &'static str {
        "search_files(path, query)"
    }
    fn required_access(&self, args: &ToolArgs) -> Vec<(PathBuf, Access)> {
        arg_path(args)
            .map(|p| vec![(p, Access::Read)])
            .unwrap_or_default()
    }
    async fn run(&self, args: ToolArgs, cx: &ToolCx) -> Result<ToolOutput, ToolError> {
        let given = arg_path(&args)?;
        let query = arg_str(&args, "query")?.to_owned();
        let root = cx
            .gate
            .authorize(&given, Access::Read, &format!("to search {}", base(&given)))
            .await?;
        let gate = cx.gate.clone();
        let (r, q) = (root.clone(), query.clone());
        let hits: Vec<PathBuf> = tokio::task::spawn_blocking(move || {
            walk_files(gate.as_ref(), &r, 8)
                .into_iter()
                .filter(|p| {
                    p.file_name()
                        .is_some_and(|n| name_matches(&n.to_string_lossy(), &q))
                })
                .collect()
        })
        .await
        .map_err(|e| ToolError::Invalid(e.to_string()))?;
        let total = hits.len();
        let mut out = String::new();
        let mut shown = 0;
        for h in &hits {
            let rel = h.strip_prefix(&root).unwrap_or(h).display().to_string();
            let line = format!("{rel}\n");
            if estimate_tokens(&out) + estimate_tokens(&line) > cx.budget_tokens {
                break;
            }
            out.push_str(&line);
            shown += 1;
        }
        if shown < total {
            out.push_str(&format!("[truncated: {} more files]\n", total - shown));
        }
        if total == 0 {
            out.push_str("(no matching files)\n");
        }
        Ok(ToolOutput {
            llm_text: format!(
                "<tool_result name=\"search_files\" path=\"{}\" query=\"{query}\">\n{out}</tool_result>",
                root.display()
            ),
            ui: Some(
                serde_json::json!({"path": root.display().to_string(), "query": query, "matches": total}),
            ),
            summary: format!(
                "Found {total} file{} matching “{query}”",
                if total == 1 { "" } else { "s" }
            ),
        })
    }
}

// ---- grep_text ----------------------------------------------------------------------------------

pub struct GrepText;

const MAX_GREP_MATCHES: usize = 40;

#[async_trait]
impl Tool for GrepText {
    fn name(&self) -> &'static str {
        "grep_text"
    }
    fn description(&self) -> &'static str {
        "Search inside text files for a word or phrase; returns file, line number, and the line."
    }
    fn signature(&self) -> &'static str {
        "grep_text(path, query)"
    }
    fn required_access(&self, args: &ToolArgs) -> Vec<(PathBuf, Access)> {
        arg_path(args)
            .map(|p| vec![(p, Access::Read)])
            .unwrap_or_default()
    }
    async fn run(&self, args: ToolArgs, cx: &ToolCx) -> Result<ToolOutput, ToolError> {
        let given = arg_path(&args)?;
        let query = arg_str(&args, "query")?.to_owned();
        let root = cx
            .gate
            .authorize(
                &given,
                Access::Read,
                &format!("to search inside {}", base(&given)),
            )
            .await?;
        let gate = cx.gate.clone();
        let (r, q) = (root.clone(), query.to_lowercase());
        let (matches, files_scanned) = tokio::task::spawn_blocking(move || {
            let files = if r.is_file() {
                vec![r.clone()]
            } else {
                walk_files(gate.as_ref(), &r, 8)
            };
            let mut found: Vec<(PathBuf, usize, String)> = Vec::new();
            let mut scanned = 0usize;
            for f in files {
                let Ok(meta) = std::fs::metadata(&f) else { continue };
                if meta.len() > MAX_FILE_BYTES {
                    continue;
                }
                let Ok(bytes) = std::fs::read(&f) else { continue };
                if is_binary(&bytes) {
                    continue;
                }
                scanned += 1;
                let text = String::from_utf8_lossy(&bytes);
                for (i, line) in text.lines().enumerate() {
                    if line.to_lowercase().contains(&q) {
                        let shown: String = line.trim().chars().take(160).collect();
                        found.push((f.clone(), i + 1, shown));
                        if found.len() > MAX_GREP_MATCHES {
                            return (found, scanned);
                        }
                    }
                }
            }
            (found, scanned)
        })
        .await
        .map_err(|e| ToolError::Invalid(e.to_string()))?;
        let capped = matches.len() > MAX_GREP_MATCHES;
        let total = matches.len().min(MAX_GREP_MATCHES);
        let mut out = String::new();
        let mut shown = 0;
        for (f, line_no, text) in matches.iter().take(MAX_GREP_MATCHES) {
            let rel = f.strip_prefix(&root).unwrap_or(f).display().to_string();
            let rel = if rel.is_empty() { base(f) } else { rel };
            let line = format!("{rel}:{line_no}: {text}\n");
            if estimate_tokens(&out) + estimate_tokens(&line) > cx.budget_tokens {
                break;
            }
            out.push_str(&line);
            shown += 1;
        }
        if capped || shown < total {
            out.push_str(&format!(
                "[truncated: showing {shown} matches; narrow the query for more]\n"
            ));
        }
        if total == 0 {
            out.push_str("(no matches)\n");
        }
        Ok(ToolOutput {
            llm_text: format!(
                "<tool_result name=\"grep_text\" path=\"{}\" query=\"{query}\">\n{out}</tool_result>",
                root.display()
            ),
            ui: Some(
                serde_json::json!({"path": root.display().to_string(), "query": query, "matches": total, "files_scanned": files_scanned}),
            ),
            summary: format!(
                "Found {}{total} match{} for “{query}”",
                if capped { "over " } else { "" },
                if total == 1 { "" } else { "es" }
            ),
        })
    }
}

// ---- file_info ----------------------------------------------------------------------------------

pub struct FileInfo;

#[async_trait]
impl Tool for FileInfo {
    fn name(&self) -> &'static str {
        "file_info"
    }
    fn description(&self) -> &'static str {
        "Get a file's or folder's size, type, and modified time."
    }
    fn signature(&self) -> &'static str {
        "file_info(path)"
    }
    fn required_access(&self, args: &ToolArgs) -> Vec<(PathBuf, Access)> {
        arg_path(args)
            .map(|p| vec![(p, Access::Read)])
            .unwrap_or_default()
    }
    async fn run(&self, args: ToolArgs, cx: &ToolCx) -> Result<ToolOutput, ToolError> {
        let given = arg_path(&args)?;
        let path = cx
            .gate
            .authorize(&given, Access::Read, &format!("to inspect {}", base(&given)))
            .await?;
        let meta = std::fs::symlink_metadata(&path).map_err(|e| io_err(&path, e))?;
        let kind = if meta.is_dir() {
            "folder"
        } else if meta.file_type().is_symlink() {
            "symlink"
        } else {
            "file"
        };
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().into_owned())
            .unwrap_or_default();
        let modified = modified_ms(&meta)
            .map(format_ms)
            .unwrap_or_else(|| "unknown".into());
        let text = format!(
            "<tool_result name=\"file_info\">\nname: {}\npath: {}\nkind: {kind}\nsize: {} ({} bytes)\nextension: {}\nmodified: {modified}\n</tool_result>",
            base(&path),
            path.display(),
            human_size(meta.len()),
            meta.len(),
            if ext.is_empty() { "none" } else { &ext },
        );
        Ok(ToolOutput {
            llm_text: text,
            ui: Some(
                serde_json::json!({"path": path.display().to_string(), "kind": kind, "size": meta.len()}),
            ),
            summary: format!("Checked {}", base(&path)),
        })
    }
}

/// `YYYY-MM-DD HH:MM UTC` from Unix milliseconds.
pub fn format_ms(ms: i64) -> String {
    match time::OffsetDateTime::from_unix_timestamp(ms.div_euclid(1000)) {
        Ok(t) => format!(
            "{:04}-{:02}-{:02} {:02}:{:02} UTC",
            t.year(),
            u8::from(t.month()),
            t.day(),
            t.hour(),
            t.minute()
        ),
        Err(_) => "unknown".into(),
    }
}

// ---- calculate ----------------------------------------------------------------------------------

pub struct Calculate;

/// Evaluate numbers with `+ - * / ^ % ( )`, `sqrt(x)`, and `round(x)`. Nothing else is accepted.
pub fn evaluate(expr: &str) -> Result<f64, String> {
    struct P<'a> {
        s: &'a [u8],
        i: usize,
    }
    impl P<'_> {
        fn ws(&mut self) {
            while self.s.get(self.i).is_some_and(u8::is_ascii_whitespace) {
                self.i += 1;
            }
        }
        fn peek(&mut self) -> Option<u8> {
            self.ws();
            self.s.get(self.i).copied()
        }
        fn expr(&mut self) -> Result<f64, String> {
            let mut v = self.term()?;
            while let Some(c @ (b'+' | b'-')) = self.peek() {
                self.i += 1;
                let r = self.term()?;
                v = if c == b'+' { v + r } else { v - r };
            }
            Ok(v)
        }
        fn term(&mut self) -> Result<f64, String> {
            let mut v = self.power()?;
            while let Some(c @ (b'*' | b'/' | b'%')) = self.peek() {
                self.i += 1;
                let r = self.power()?;
                v = match c {
                    b'*' => v * r,
                    b'/' if r == 0.0 => return Err("division by zero".into()),
                    b'/' => v / r,
                    _ if r == 0.0 => return Err("division by zero".into()),
                    _ => v % r,
                };
            }
            Ok(v)
        }
        fn power(&mut self) -> Result<f64, String> {
            let base = self.unary()?;
            if self.peek() == Some(b'^') {
                self.i += 1;
                let exp = self.power()?; // right-associative
                return Ok(base.powf(exp));
            }
            Ok(base)
        }
        fn unary(&mut self) -> Result<f64, String> {
            match self.peek() {
                Some(b'-') => {
                    self.i += 1;
                    Ok(-self.unary()?)
                }
                Some(b'+') => {
                    self.i += 1;
                    self.unary()
                }
                _ => self.atom(),
            }
        }
        fn atom(&mut self) -> Result<f64, String> {
            match self.peek() {
                Some(b'(') => {
                    self.i += 1;
                    let v = self.expr()?;
                    if self.peek() != Some(b')') {
                        return Err("missing )".into());
                    }
                    self.i += 1;
                    Ok(v)
                }
                Some(c) if c.is_ascii_digit() || c == b'.' => {
                    let start = self.i;
                    while self
                        .s
                        .get(self.i)
                        .is_some_and(|c| c.is_ascii_digit() || *c == b'.')
                    {
                        self.i += 1;
                    }
                    std::str::from_utf8(&self.s[start..self.i])
                        .map_err(|e| e.to_string())?
                        .parse::<f64>()
                        .map_err(|_| "bad number".to_string())
                }
                Some(c) if c.is_ascii_alphabetic() => {
                    let start = self.i;
                    while self.s.get(self.i).is_some_and(u8::is_ascii_alphabetic) {
                        self.i += 1;
                    }
                    let name = std::str::from_utf8(&self.s[start..self.i])
                        .map_err(|e| e.to_string())?
                        .to_owned();
                    if self.peek() != Some(b'(') {
                        return Err(format!("unknown name `{name}`"));
                    }
                    self.i += 1;
                    let v = self.expr()?;
                    if self.peek() != Some(b')') {
                        return Err("missing )".into());
                    }
                    self.i += 1;
                    match name.as_str() {
                        "sqrt" if v < 0.0 => Err("sqrt of a negative number".into()),
                        "sqrt" => Ok(v.sqrt()),
                        "round" => Ok(v.round()),
                        _ => Err(format!("unknown function `{name}`")),
                    }
                }
                Some(c) => Err(format!("unexpected `{}`", c as char)),
                None => Err("incomplete expression".into()),
            }
        }
    }
    if expr.len() > 200 {
        return Err("expression too long".into());
    }
    let mut p = P {
        s: expr.as_bytes(),
        i: 0,
    };
    let v = p.expr()?;
    if p.peek().is_some() {
        return Err("unexpected trailing characters".into());
    }
    if v.is_finite() {
        Ok(v)
    } else {
        Err("result is not a finite number".into())
    }
}

fn fmt_num(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{v:.0}")
    } else {
        format!("{v}")
    }
}

#[async_trait]
impl Tool for Calculate {
    fn name(&self) -> &'static str {
        "calculate"
    }
    fn description(&self) -> &'static str {
        "Do arithmetic exactly: + - * / ^ % ( ), sqrt(x), round(x)."
    }
    fn signature(&self) -> &'static str {
        "calculate(expression)"
    }
    fn required_access(&self, _args: &ToolArgs) -> Vec<(PathBuf, Access)> {
        Vec::new()
    }
    async fn run(&self, args: ToolArgs, _cx: &ToolCx) -> Result<ToolOutput, ToolError> {
        let expr = arg_str(&args, "expression")?;
        let v = evaluate(expr).map_err(|e| ToolError::Invalid(format!("Cannot calculate `{expr}`: {e}.")))?;
        Ok(ToolOutput {
            llm_text: format!(
                "<tool_result name=\"calculate\">\n{expr} = {}\n</tool_result>",
                fmt_num(v)
            ),
            ui: Some(serde_json::json!({"expression": expr, "result": v})),
            summary: format!("{expr} = {}", fmt_num(v)),
        })
    }
}

// ---- now ----------------------------------------------------------------------------------------

pub struct Now;

#[async_trait]
impl Tool for Now {
    fn name(&self) -> &'static str {
        "now"
    }
    fn description(&self) -> &'static str {
        "Get the current date and time."
    }
    fn signature(&self) -> &'static str {
        "now()"
    }
    fn required_access(&self, _args: &ToolArgs) -> Vec<(PathBuf, Access)> {
        Vec::new()
    }
    async fn run(&self, _args: ToolArgs, _cx: &ToolCx) -> Result<ToolOutput, ToolError> {
        let t = format_ms(time::OffsetDateTime::now_utc().unix_timestamp() * 1000);
        Ok(ToolOutput {
            llm_text: format!("<tool_result name=\"now\">\n{t}\n</tool_result>"),
            ui: None,
            summary: "Checked the time".into(),
        })
    }
}

// ---- stubs until M08 ----------------------------------------------------------------------------

/// Scan a folder and return its facts (no duplicate hashing, so it stays quick inside a chat turn).
pub struct ScanFolder;

#[async_trait]
impl Tool for ScanFolder {
    fn name(&self) -> &'static str {
        "scan_folder"
    }
    fn description(&self) -> &'static str {
        "Scan a folder: sizes, file types, duplicates, and clutter."
    }
    fn signature(&self) -> &'static str {
        "scan_folder(path)"
    }
    fn required_access(&self, args: &ToolArgs) -> Vec<(PathBuf, Access)> {
        arg_path(args)
            .map(|p| vec![(p, Access::Read)])
            .unwrap_or_default()
    }
    async fn run(&self, args: ToolArgs, cx: &ToolCx) -> Result<ToolOutput, ToolError> {
        let given = arg_path(&args)?;
        let root = cx
            .gate
            .authorize(&given, Access::Read, &format!("to scan {}", base(&given)))
            .await?;
        if !root.is_dir() {
            return Err(ToolError::Invalid(format!(
                "{} is a file; use file_info or read_file.",
                root.display()
            )));
        }
        let mut opts = nexus_analyze::ScanOptions::new(&root);
        opts.duplicates = nexus_analyze::DupMode::Off;
        opts.max_entries = 200_000;
        let report = tokio::task::spawn_blocking(move || {
            nexus_analyze::scan_folder(&opts, &|_| {}, &tokio_util::sync::CancellationToken::new())
        })
        .await
        .map_err(|e| ToolError::Invalid(e.to_string()))?
        .map_err(|e| ToolError::Invalid(e.to_string()))?;
        let facts = nexus_analyze::facts::folder_facts(&report, &root.display().to_string());
        let (text, truncated) = truncate_to_tokens(&facts, cx.budget_tokens);
        Ok(ToolOutput {
            llm_text: format!(
                "<tool_result name=\"scan_folder\">\n{text}{}\n</tool_result>",
                if truncated { "\n[truncated]" } else { "" }
            ),
            ui: Some(
                serde_json::json!({"path": root.display().to_string(), "files": report.totals.files, "bytes": report.totals.bytes}),
            ),
            summary: format!("Scanned {} ({} files)", base(&root), report.totals.files),
        })
    }
}

/// Search inside the documents attached to this chat (BM25), returning page-cited snippets.
pub struct DocSearch;

#[async_trait]
impl Tool for DocSearch {
    fn name(&self) -> &'static str {
        "doc_search"
    }
    fn description(&self) -> &'static str {
        "Search inside the attached documents."
    }
    fn signature(&self) -> &'static str {
        "doc_search(query)"
    }
    fn required_access(&self, _args: &ToolArgs) -> Vec<(PathBuf, Access)> {
        Vec::new()
    }
    async fn run(&self, args: ToolArgs, cx: &ToolCx) -> Result<ToolOutput, ToolError> {
        let query = arg_str(&args, "query")?.to_owned();
        if cx.docs.is_empty() {
            return Err(ToolError::Invalid(
                "No document is attached to this chat. Ask the user to attach one.".into(),
            ));
        }
        let mut out = String::new();
        let mut hits = 0;
        for d in &cx.docs {
            let lines = search_lines(d, &query, 3);
            hits += lines.len();
            if !lines.is_empty() {
                out.push_str(&format!("{}:\n{}\n", d.title, lines.join("\n")));
            }
        }
        if hits == 0 {
            out.push_str("(no matches)\n");
        }
        let (text, _) = truncate_to_tokens(&out, cx.budget_tokens);
        Ok(ToolOutput {
            llm_text: format!("<tool_result name=\"doc_search\" query=\"{query}\">\n{text}</tool_result>"),
            ui: Some(serde_json::json!({"query": query, "matches": hits})),
            summary: format!(
                "Searched the document for “{query}” ({hits} match{})",
                if hits == 1 { "" } else { "es" }
            ),
        })
    }
}

/// `kb_search(query, library?)`: passages from the offline knowledge libraries, numbered for `[n]` citations.
pub struct KbSearch;

#[async_trait]
impl Tool for KbSearch {
    fn name(&self) -> &'static str {
        "kb_search"
    }
    fn description(&self) -> &'static str {
        "Search the user's offline reference libraries (manuals, books) and return cited passages."
    }
    fn signature(&self) -> &'static str {
        "kb_search(query, library?)"
    }
    fn required_access(&self, _args: &ToolArgs) -> Vec<(PathBuf, Access)> {
        Vec::new()
    }
    async fn run(&self, args: ToolArgs, cx: &ToolCx) -> Result<ToolOutput, ToolError> {
        let query = arg_str(&args, "query")?.to_owned();
        let library = args.get("library").and_then(|v| v.as_str()).map(str::to_owned);
        let Some(kb) = cx.kb.clone() else {
            return Err(ToolError::Invalid(
                "No knowledge library is enabled. Ask the user to add or enable one on the Library page."
                    .into(),
            ));
        };
        let budget = cx.budget_tokens.min(1200);
        let hits = kb
            .search(&query, library.as_deref(), budget)
            .map_err(ToolError::Invalid)?;
        let body = if hits.passages == 0 {
            "(no matching passages: the answer is not in the library)\n".to_owned()
        } else {
            format!("{}\n", hits.context)
        };
        Ok(ToolOutput {
            llm_text: format!(
                "<tool_result name=\"kb_search\" query=\"{}\">\n{body}</tool_result>",
                query.replace('"', "'")
            ),
            ui: Some(serde_json::json!({"query": query, "passages": hits.passages})),
            summary: format!(
                "Searched the libraries for “{query}” ({} passage{})",
                hits.passages,
                if hits.passages == 1 { "" } else { "s" }
            ),
        })
    }
}

/// `find_symbol(query)`: where a function, class, file or folder is defined and used in the indexed project.
pub struct FindSymbol;

#[async_trait]
impl Tool for FindSymbol {
    fn name(&self) -> &'static str {
        "find_symbol"
    }
    fn description(&self) -> &'static str {
        "Find where a function, class, variable, file or folder is defined and used in the user's indexed code project."
    }
    fn signature(&self) -> &'static str {
        "find_symbol(query)"
    }
    fn required_access(&self, _args: &ToolArgs) -> Vec<(PathBuf, Access)> {
        Vec::new()
    }
    async fn run(&self, args: ToolArgs, cx: &ToolCx) -> Result<ToolOutput, ToolError> {
        let query = arg_str(&args, "query")?.to_owned();
        let Some(code) = cx.code.clone() else {
            return Err(ToolError::Invalid(
                "No code project is indexed. Ask the user to open the Rename Forge page and index a project folder."
                    .into(),
            ));
        };
        let max_chars = (cx.budget_tokens as usize).saturating_mul(3);
        let body = code.find(&query, max_chars).map_err(ToolError::Invalid)?;
        Ok(ToolOutput {
            llm_text: format!(
                "<tool_result name=\"find_symbol\" query=\"{}\">\n{body}</tool_result>",
                query.replace('"', "'")
            ),
            ui: Some(serde_json::json!({"query": query})),
            summary: format!("Looked up “{query}” in the code project"),
        })
    }
}

// ---- proposing tools (ADR-016) -------------------------------------------------------------------

/// Largest text `write_file` and `edit_file` accept in one call.
pub const MAX_WRITE_CHARS: usize = 20_000;

fn arg_text<'a>(args: &'a ToolArgs, key: &str) -> Result<&'a str, ToolError> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::Invalid(format!("Missing argument `{key}`.")))
}

fn sink(cx: &ToolCx) -> Result<&Arc<dyn ProposalSink>, ToolError> {
    cx.proposals
        .as_ref()
        .ok_or(ToolError::Unavailable("Proposing file changes"))
}

/// Stages one op and, when automatic apply is on, writes it. `what` reads "create /a/b.txt (5 characters)".
fn staged(
    cx: &ToolCx,
    title: String,
    op: FileOp,
    ui: Value,
    what: String,
    summary: String,
) -> Result<ToolOutput, ToolError> {
    let sink = sink(cx)?;
    let id = sink
        .propose(&title, vec![op])
        .map_err(|e| ToolError::Invalid(format!("Could not stage the change: {e}")))?;
    let outcome = sink.auto_apply(&id);
    let mut ui = ui;
    if let Some(o) = ui.as_object_mut() {
        o.insert("change_set".into(), Value::String(id));
        o.insert(
            "state".into(),
            Value::String(
                match &outcome {
                    AutoApply::Off => "planned",
                    AutoApply::Applied => "applied",
                    AutoApply::Failed(_) => "failed",
                }
                .into(),
            ),
        );
    }
    let (llm_text, summary) = match outcome {
        AutoApply::Off => (
            format!(
                "Proposed to {what}. NOT written yet: tell the user to press Apply on the change card in this chat (or in Changes)."
            ),
            format!("{summary} (press Apply)"),
        ),
        AutoApply::Applied => (
            format!(
                "DONE: {what}. It is written to disk and the user can undo it in Changes. Tell the user it is done."
            ),
            format!("Applied: {summary}"),
        ),
        AutoApply::Failed(why) => (
            format!(
                "Proposed to {what}, but it was NOT written: {why}. Tell the user why; the change card in this chat still lets them Apply or Discard it."
            ),
            format!("{summary} (not applied: {why})"),
        ),
    };
    Ok(ToolOutput {
        llm_text,
        ui: Some(ui),
        summary,
    })
}

/// `write_file(path, content)`: propose creating a file, or replacing one the user can preview.
pub struct WriteFile;

#[async_trait]
impl Tool for WriteFile {
    fn name(&self) -> &'static str {
        "write_file"
    }
    fn description(&self) -> &'static str {
        "Propose a new file or a full replacement of one. The user reviews and applies it; nothing is written yet."
    }
    fn signature(&self) -> &'static str {
        "write_file(path, content)"
    }
    fn required_access(&self, args: &ToolArgs) -> Vec<(PathBuf, Access)> {
        arg_path(args)
            .map(|p| {
                let a = if p.exists() { Access::Write } else { Access::Create };
                vec![(p, a)]
            })
            .unwrap_or_default()
    }
    async fn run(&self, args: ToolArgs, cx: &ToolCx) -> Result<ToolOutput, ToolError> {
        let given = arg_path(&args)?;
        let content = arg_text(&args, "content")?;
        if content.chars().count() > MAX_WRITE_CHARS {
            return Err(ToolError::Invalid(format!(
                "Content is longer than {MAX_WRITE_CHARS} characters. Write a smaller file, or split it into several files."
            )));
        }
        sink(cx)?;
        let exists = given.exists();
        let access = if exists { Access::Write } else { Access::Create };
        let path = cx
            .gate
            .authorize(&given, access, &format!("to propose changes to {}", base(&given)))
            .await?;
        let shown = path.display().to_string();
        let (op, verb) = if path.exists() {
            if path.is_dir() {
                return Err(ToolError::Invalid(format!("{shown} is a folder.")));
            }
            let bytes = std::fs::read(&path).map_err(|e| io_err(&path, e))?;
            (
                FileOp::Write {
                    path: shown.clone(),
                    base_hash: Some(nexus_fs::hash_bytes(&bytes)),
                    contents: content.to_owned(),
                },
                "replace",
            )
        } else {
            (
                FileOp::Create {
                    path: shown.clone(),
                    contents: content.to_owned(),
                },
                "create",
            )
        };
        staged(
            cx,
            format!(
                "{} {}",
                if verb == "create" { "Create" } else { "Replace" },
                base(&path)
            ),
            op,
            serde_json::json!({"path": shown, "kind": verb}),
            format!("{verb} {shown} ({} characters)", content.chars().count()),
            format!(
                "{} {}",
                if verb == "create" { "Create" } else { "Replace" },
                base(&path)
            ),
        )
    }
}

/// `edit_file(path, old_text, new_text)`: propose replacing one exact passage.
pub struct EditFile;

#[async_trait]
impl Tool for EditFile {
    fn name(&self) -> &'static str {
        "edit_file"
    }
    fn description(&self) -> &'static str {
        "Propose replacing one exact passage in an existing file. The user reviews and applies it."
    }
    fn signature(&self) -> &'static str {
        "edit_file(path, old_text, new_text)"
    }
    fn required_access(&self, args: &ToolArgs) -> Vec<(PathBuf, Access)> {
        arg_path(args)
            .map(|p| vec![(p, Access::Write)])
            .unwrap_or_default()
    }
    async fn run(&self, args: ToolArgs, cx: &ToolCx) -> Result<ToolOutput, ToolError> {
        let given = arg_path(&args)?;
        let old = arg_str(&args, "old_text")?;
        let new = arg_text(&args, "new_text")?;
        if new.chars().count() > MAX_WRITE_CHARS {
            return Err(ToolError::Invalid(format!(
                "new_text is longer than {MAX_WRITE_CHARS} characters."
            )));
        }
        sink(cx)?;
        let path = cx
            .gate
            .authorize(
                &given,
                Access::Write,
                &format!("to propose an edit to {}", base(&given)),
            )
            .await?;
        let meta = std::fs::metadata(&path).map_err(|e| io_err(&path, e))?;
        if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
            return Err(ToolError::Invalid(format!(
                "{} is not a text file I can edit.",
                base(&path)
            )));
        }
        let bytes = std::fs::read(&path).map_err(|e| io_err(&path, e))?;
        if is_binary(&bytes) {
            return Err(ToolError::Invalid(format!("{} is a binary file.", base(&path))));
        }
        let text = String::from_utf8_lossy(&bytes);
        let mut hits = text.match_indices(old);
        let Some((start, _)) = hits.next() else {
            return Err(ToolError::Invalid(format!(
                "old_text was not found in {}. Read the file again and copy the passage exactly.",
                base(&path)
            )));
        };
        if hits.next().is_some() {
            return Err(ToolError::Invalid(format!(
                "old_text appears more than once in {}. Include more surrounding lines so it is unique.",
                base(&path)
            )));
        }
        let (start, end) = (
            u32::try_from(start).map_err(|_| ToolError::Invalid("File is too large.".into()))?,
            u32::try_from(start + old.len()).map_err(|_| ToolError::Invalid("File is too large.".into()))?,
        );
        let shown = path.display().to_string();
        staged(
            cx,
            format!("Edit {}", base(&path)),
            FileOp::Edit {
                path: shown.clone(),
                base_hash: nexus_fs::hash_bytes(&bytes),
                edits: vec![TextEdit {
                    start,
                    end,
                    new_text: new.to_owned(),
                }],
            },
            serde_json::json!({"path": shown, "kind": "edit"}),
            format!("edit {shown}"),
            format!("Edit {}", base(&path)),
        )
    }
}

/// `move_file(path, to)`: propose moving or renaming.
pub struct MoveFile;

#[async_trait]
impl Tool for MoveFile {
    fn name(&self) -> &'static str {
        "move_file"
    }
    fn description(&self) -> &'static str {
        "Propose moving or renaming a file or folder. The user reviews and applies it."
    }
    fn signature(&self) -> &'static str {
        "move_file(path, to)"
    }
    fn required_access(&self, args: &ToolArgs) -> Vec<(PathBuf, Access)> {
        let mut v = Vec::new();
        if let Ok(p) = arg_path(args) {
            v.push((p, Access::Move));
        }
        if let Ok(t) = arg_str(args, "to") {
            v.push((PathBuf::from(t), Access::Create));
        }
        v
    }
    async fn run(&self, args: ToolArgs, cx: &ToolCx) -> Result<ToolOutput, ToolError> {
        let from = arg_path(&args)?;
        let to = PathBuf::from(arg_str(&args, "to")?);
        sink(cx)?;
        let from = cx
            .gate
            .authorize(&from, Access::Move, &format!("to propose moving {}", base(&from)))
            .await?;
        let to = cx
            .gate
            .authorize(
                &to,
                Access::Create,
                &format!("to propose moving a file to {}", base(&to)),
            )
            .await?;
        if !from.exists() {
            return Err(ToolError::NotFound(from.display().to_string()));
        }
        if to.exists() {
            return Err(ToolError::Invalid(format!("{} already exists.", to.display())));
        }
        let (f, t) = (from.display().to_string(), to.display().to_string());
        staged(
            cx,
            format!("Move {}", base(&from)),
            FileOp::Move {
                from: f.clone(),
                to: t.clone(),
            },
            serde_json::json!({"path": f, "to": t, "kind": "move"}),
            format!("move {f} to {t}"),
            format!("Move {}", base(&from)),
        )
    }
}

/// `trash_file(path)`: propose moving to the system Trash (never a hard delete).
pub struct TrashFile;

#[async_trait]
impl Tool for TrashFile {
    fn name(&self) -> &'static str {
        "trash_file"
    }
    fn description(&self) -> &'static str {
        "Propose moving a file or folder to the Trash. The user reviews and applies it."
    }
    fn signature(&self) -> &'static str {
        "trash_file(path)"
    }
    fn required_access(&self, args: &ToolArgs) -> Vec<(PathBuf, Access)> {
        arg_path(args)
            .map(|p| vec![(p, Access::Trash)])
            .unwrap_or_default()
    }
    async fn run(&self, args: ToolArgs, cx: &ToolCx) -> Result<ToolOutput, ToolError> {
        let given = arg_path(&args)?;
        sink(cx)?;
        let path = cx
            .gate
            .authorize(
                &given,
                Access::Trash,
                &format!("to propose trashing {}", base(&given)),
            )
            .await?;
        if !path.exists() {
            return Err(ToolError::NotFound(path.display().to_string()));
        }
        let shown = path.display().to_string();
        staged(
            cx,
            format!("Trash {}", base(&path)),
            FileOp::Trash { path: shown.clone() },
            serde_json::json!({"path": shown, "kind": "trash"}),
            format!("move {shown} to the Trash"),
            format!("Trash {}", base(&path)),
        )
    }
}

// ---- web tools (ADR-016) -------------------------------------------------------------------------

fn web(cx: &ToolCx) -> Result<&Arc<dyn WebSource>, ToolError> {
    cx.web.as_ref().ok_or(ToolError::Unavailable(
        "Web access (it is off or you are offline; the user can turn it on in Settings → Tools)",
    ))
}

/// `search_web(query)`: top results from a web search.
pub struct SearchWeb;

#[async_trait]
impl Tool for SearchWeb {
    fn name(&self) -> &'static str {
        "search_web"
    }
    fn description(&self) -> &'static str {
        "Search the web for current information. Returns titles, links and snippets."
    }
    fn signature(&self) -> &'static str {
        "search_web(query)"
    }
    fn required_access(&self, _args: &ToolArgs) -> Vec<(PathBuf, Access)> {
        Vec::new()
    }
    async fn run(&self, args: ToolArgs, cx: &ToolCx) -> Result<ToolOutput, ToolError> {
        let query = arg_str(&args, "query")?;
        let hits = web(cx)?
            .search(query, 6)
            .await
            .map_err(|e| ToolError::Invalid(format!("Web search failed: {e}")))?;
        if hits.is_empty() {
            return Ok(ToolOutput {
                llm_text: format!("No web results for “{query}”."),
                ui: None,
                summary: format!("Searched the web for “{query}” (no results)"),
            });
        }
        let mut text = String::new();
        for (i, h) in hits.iter().enumerate() {
            text.push_str(&format!(
                "{}. {}\n   {}\n   {}\n",
                i + 1,
                h.title,
                h.url,
                h.snippet
            ));
        }
        let (body, _) = truncate_to_tokens(&text, cx.budget_tokens);
        Ok(ToolOutput {
            llm_text: format!("<web_results query=\"{query}\">\n{body}</web_results>"),
            ui: Some(serde_json::json!({
                "query": query,
                "results": hits.iter().map(|h| serde_json::json!({"title": h.title, "url": h.url})).collect::<Vec<_>>()
            })),
            summary: format!("Searched the web for “{query}” ({} results)", hits.len()),
        })
    }
}

/// `fetch_url(url)`: read one web page as plain text.
pub struct FetchUrl;

#[async_trait]
impl Tool for FetchUrl {
    fn name(&self) -> &'static str {
        "fetch_url"
    }
    fn description(&self) -> &'static str {
        "Read one web page (a link from search_web or one the user gave) as plain text."
    }
    fn signature(&self) -> &'static str {
        "fetch_url(url)"
    }
    fn required_access(&self, _args: &ToolArgs) -> Vec<(PathBuf, Access)> {
        Vec::new()
    }
    async fn run(&self, args: ToolArgs, cx: &ToolCx) -> Result<ToolOutput, ToolError> {
        let url = arg_str(&args, "url")?;
        let max_chars = (f64::from(cx.budget_tokens) * 3.2) as usize;
        let page = web(cx)?
            .fetch(url, max_chars)
            .await
            .map_err(|e| ToolError::Invalid(format!("Could not read the page: {e}")))?;
        let (body, truncated) = truncate_to_tokens(&page.text, cx.budget_tokens);
        let note = if truncated { "\n[truncated]" } else { "" };
        Ok(ToolOutput {
            llm_text: format!(
                "<web_page url=\"{}\" title=\"{}\">\n{body}{note}\n</web_page>",
                page.url, page.title
            ),
            ui: Some(serde_json::json!({"url": page.url, "title": page.title})),
            summary: format!(
                "Read {}",
                if page.title.is_empty() {
                    &page.url
                } else {
                    &page.title
                }
            ),
        })
    }
}

/// The built-in tool set.
pub fn builtin_tools() -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(ReadFile),
        Arc::new(ListDir),
        Arc::new(SearchFiles),
        Arc::new(GrepText),
        Arc::new(FileInfo),
        Arc::new(ScanFolder),
        Arc::new(DocSearch),
        Arc::new(KbSearch),
        Arc::new(FindSymbol),
        Arc::new(Calculate),
        Arc::new(Now),
        Arc::new(WriteFile),
        Arc::new(EditFile),
        Arc::new(MoveFile),
        Arc::new(TrashFile),
        Arc::new(SearchWeb),
        Arc::new(FetchUrl),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_core::fs::{GrantLevel, GrantScope};
    use nexus_fs::{DenyList, MemoryAudit, NewGrant};
    use std::sync::Mutex;

    struct Scripted {
        answer: AskOutcome,
        engine: Arc<PermissionEngine>,
        asked: Mutex<Vec<PathBuf>>,
    }

    #[async_trait]
    impl PermissionAsker for Scripted {
        async fn ask(&self, _a: &str, path: &Path, _n: Access, _r: &str) -> AskOutcome {
            if let Ok(mut v) = self.asked.lock() {
                v.push(path.to_owned());
            }
            if self.answer == AskOutcome::Allowed {
                let _ = self.engine.add_grant(NewGrant {
                    root: path.to_owned(),
                    level: GrantLevel::Read,
                    recursive: true,
                    scope: GrantScope::Session,
                    expires_at: None,
                    label: None,
                });
            }
            self.answer
        }
    }

    fn engine() -> Arc<PermissionEngine> {
        Arc::new(PermissionEngine::new(
            DenyList::default(),
            Arc::new(MemoryAudit::default()),
        ))
    }

    fn grant(e: &PermissionEngine, root: &Path, recursive: bool) {
        e.add_grant(NewGrant {
            root: root.to_owned(),
            level: GrantLevel::Read,
            recursive,
            scope: GrantScope::Session,
            expires_at: None,
            label: None,
        })
        .unwrap();
    }

    fn cx(e: Arc<PermissionEngine>) -> ToolCx {
        ToolCx::new(Arc::new(EngineGatekeeper::new(e, "test", None)))
    }

    fn args(pairs: &[(&str, &str)]) -> ToolArgs {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), Value::String((*v).to_owned())))
            .collect()
    }

    fn fixture() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("notes.md"), "# Notes\nbuy milk\nCall Bob\n").unwrap();
        std::fs::create_dir_all(d.path().join("sub/deep")).unwrap();
        std::fs::write(d.path().join("sub/report.pdf"), [0u8, 1, 2]).unwrap();
        std::fs::write(d.path().join("sub/deep/todo.txt"), "milk again\n").unwrap();
        std::fs::create_dir_all(d.path().join("node_modules")).unwrap();
        std::fs::write(d.path().join("node_modules/skip.txt"), "milk").unwrap();
        d
    }

    #[tokio::test]
    async fn read_file_returns_tagged_text() {
        let d = fixture();
        let e = engine();
        grant(&e, d.path(), true);
        let out = ReadFile
            .run(
                args(&[("path", d.path().join("notes.md").to_str().unwrap())]),
                &cx(e),
            )
            .await
            .unwrap();
        assert!(out.llm_text.starts_with("<file path="));
        assert!(out.llm_text.contains("buy milk"));
        assert_eq!(out.summary, "Read notes.md");
    }

    #[tokio::test]
    async fn read_file_truncates_to_the_budget() {
        let d = fixture();
        let big = (0..2000)
            .map(|i| format!("line number {i}\n"))
            .collect::<String>();
        std::fs::write(d.path().join("big.txt"), big).unwrap();
        let e = engine();
        grant(&e, d.path(), true);
        let out = ReadFile
            .run(
                args(&[("path", d.path().join("big.txt").to_str().unwrap())]),
                &cx(e),
            )
            .await
            .unwrap();
        assert!(out.llm_text.contains("[truncated:"));
        assert!(estimate_tokens(&out.llm_text) < DEFAULT_TOOL_BUDGET + 60);
    }

    #[tokio::test]
    async fn binary_and_missing_files_give_model_friendly_errors() {
        let d = fixture();
        let e = engine();
        grant(&e, d.path(), true);
        let err = ReadFile
            .run(
                args(&[("path", d.path().join("sub/report.pdf").to_str().unwrap())]),
                &cx(e.clone()),
            )
            .await
            .unwrap_err();
        assert!(err.for_model().contains("binary"));
        let err = ReadFile
            .run(
                args(&[("path", d.path().join("nope.txt").to_str().unwrap())]),
                &cx(e),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::NotFound(_)));
    }

    #[tokio::test]
    async fn list_dir_sorts_folders_first_and_counts() {
        let d = fixture();
        let e = engine();
        grant(&e, d.path(), true);
        let out = ListDir
            .run(args(&[("path", d.path().to_str().unwrap())]), &cx(e))
            .await
            .unwrap();
        let body = &out.llm_text;
        assert!(body.find("sub/").unwrap() < body.find("notes.md").unwrap());
        assert!(out.summary.contains("(3 items)"), "{}", out.summary);
    }

    #[tokio::test]
    async fn list_dir_reports_how_many_entries_were_cut() {
        let d = tempfile::tempdir().unwrap();
        for i in 0..400 {
            std::fs::write(d.path().join(format!("file_{i:03}.txt")), "x").unwrap();
        }
        let e = engine();
        grant(&e, d.path(), true);
        let mut c = cx(e);
        c.budget_tokens = 200;
        let out = ListDir
            .run(args(&[("path", d.path().to_str().unwrap())]), &c)
            .await
            .unwrap();
        assert!(
            out.llm_text.contains("[truncated:") && out.llm_text.contains("more entries]"),
            "{}",
            out.llm_text
        );
    }

    #[tokio::test]
    async fn search_files_matches_names_and_skips_build_dirs() {
        let d = fixture();
        let e = engine();
        grant(&e, d.path(), true);
        let out = SearchFiles
            .run(
                args(&[("path", d.path().to_str().unwrap()), ("query", "*.txt")]),
                &cx(e.clone()),
            )
            .await
            .unwrap();
        assert!(out.llm_text.contains("todo.txt") && !out.llm_text.contains("skip.txt"));
        let out = SearchFiles
            .run(
                args(&[("path", d.path().to_str().unwrap()), ("query", "REPORT")]),
                &cx(e),
            )
            .await
            .unwrap();
        assert!(out.llm_text.contains("report.pdf"));
    }

    #[tokio::test]
    async fn grep_text_finds_lines_and_skips_binaries() {
        let d = fixture();
        let e = engine();
        grant(&e, d.path(), true);
        let out = GrepText
            .run(
                args(&[("path", d.path().to_str().unwrap()), ("query", "milk")]),
                &cx(e),
            )
            .await
            .unwrap();
        assert!(out.llm_text.contains("notes.md:2: buy milk"), "{}", out.llm_text);
        assert!(out.llm_text.contains("todo.txt:1: milk again"));
        assert!(!out.llm_text.contains("skip.txt"));
    }

    #[tokio::test]
    async fn walks_do_not_leak_past_a_non_recursive_grant() {
        let d = fixture();
        let e = engine();
        grant(&e, d.path(), false);
        let out = GrepText
            .run(
                args(&[("path", d.path().to_str().unwrap()), ("query", "milk")]),
                &cx(e),
            )
            .await
            .unwrap();
        assert!(
            out.llm_text.contains("notes.md") && !out.llm_text.contains("todo.txt"),
            "{}",
            out.llm_text
        );
    }

    #[tokio::test]
    async fn protected_children_are_skipped_in_walks() {
        let d = fixture();
        let e = Arc::new(PermissionEngine::new(
            DenyList::new([nexus_fs::DenyRule::user(d.path().join("sub"), "private")]),
            Arc::new(MemoryAudit::default()),
        ));
        grant(&e, d.path(), true);
        let out = GrepText
            .run(
                args(&[("path", d.path().to_str().unwrap()), ("query", "milk")]),
                &cx(e),
            )
            .await
            .unwrap();
        assert!(!out.llm_text.contains("todo.txt"));
    }

    #[tokio::test]
    async fn file_info_reports_size_and_kind() {
        let d = fixture();
        let e = engine();
        grant(&e, d.path(), true);
        let out = FileInfo
            .run(
                args(&[("path", d.path().join("notes.md").to_str().unwrap())]),
                &cx(e),
            )
            .await
            .unwrap();
        assert!(out.llm_text.contains("kind: file") && out.llm_text.contains("extension: md"));
    }

    #[test]
    fn calculator_is_exact_and_safe() {
        assert_eq!(evaluate("2 + 3 * 4").unwrap(), 14.0);
        assert_eq!(evaluate("(2 + 3) * 4").unwrap(), 20.0);
        assert_eq!(evaluate("2 ^ 3 ^ 2").unwrap(), 512.0);
        assert_eq!(evaluate("-3 + 10 % 4").unwrap(), -1.0);
        assert_eq!(evaluate("sqrt(16) + round(2.6)").unwrap(), 7.0);
        assert!((evaluate("1 / 3").unwrap() - 0.333_333).abs() < 1e-5);
        assert!(evaluate("1 / 0").is_err());
        assert!(evaluate("system(1)").is_err());
        assert!(evaluate("2 +").is_err());
        assert!(evaluate("foo").is_err());
        assert!(evaluate("sqrt(-1)").is_err());
        assert!(evaluate(&"1+".repeat(200)).is_err());
    }

    #[tokio::test]
    async fn calculate_and_now_need_no_files() {
        let c = cx(engine());
        let out = Calculate
            .run(args(&[("expression", "1200 * 0.15")]), &c)
            .await
            .unwrap();
        assert!(out.llm_text.contains("= 180"));
        assert!(
            Now.run(ToolArgs::new(), &c)
                .await
                .unwrap()
                .llm_text
                .contains("UTC")
        );
        assert!(Calculate.required_access(&ToolArgs::new()).is_empty());
    }

    #[tokio::test]
    async fn scan_folder_returns_facts_within_the_budget() {
        let d = fixture();
        let e = engine();
        grant(&e, d.path(), true);
        let out = ScanFolder
            .run(args(&[("path", d.path().to_str().unwrap())]), &cx(e.clone()))
            .await
            .unwrap();
        assert!(
            out.llm_text.contains("<facts>") && out.llm_text.contains("files: 4"),
            "{}",
            out.llm_text
        );
        assert!(out.summary.starts_with("Scanned ") && out.summary.contains("(4 files)"));
        let err = ScanFolder
            .run(
                args(&[("path", d.path().join("notes.md").to_str().unwrap())]),
                &cx(e),
            )
            .await
            .unwrap_err();
        assert!(err.for_model().contains("is a file"));
    }

    #[tokio::test]
    async fn doc_search_cites_pages_and_needs_an_attached_document() {
        let c = cx(engine());
        assert!(
            DocSearch
                .run(args(&[("query", "warranty")]), &c)
                .await
                .unwrap_err()
                .for_model()
                .contains("No document is attached")
        );
        let ex = nexus_analyze::extract::Extracted {
            kind: "pdf".into(),
            title: None,
            pages: vec![
                nexus_analyze::extract::Page {
                    number: 1,
                    text: "Shipping takes five days.".into(),
                },
                nexus_analyze::extract::Page {
                    number: 2,
                    text: "The warranty covers twenty-four months.".into(),
                },
            ],
            scanned_pages: vec![],
        };
        let mut c = cx(engine());
        c.docs
            .push(Arc::new(DocIndex::from_extracted("terms.pdf", &ex).unwrap()));
        let out = DocSearch
            .run(args(&[("query", "warranty months")]), &c)
            .await
            .unwrap();
        assert!(
            out.llm_text.contains("terms.pdf:") && out.llm_text.contains("p. "),
            "{}",
            out.llm_text
        );
        assert!(out.summary.contains("1 match"));
        let none = DocSearch.run(args(&[("query", "zebra")]), &c).await.unwrap();
        assert!(none.llm_text.contains("(no matches)"));
    }

    #[test]
    fn name_patterns() {
        assert!(name_matches("Report.PDF", "*.pdf"));
        assert!(name_matches("report_final.pdf", "report*.pdf"));
        assert!(!name_matches("report.txt", "*.pdf"));
        assert!(name_matches("budget2026.xlsx", "budget"));
    }

    #[tokio::test]
    async fn missing_grant_without_a_prompt_tells_the_model_to_ask() {
        let d = fixture();
        let err = ListDir
            .run(args(&[("path", d.path().to_str().unwrap())]), &cx(engine()))
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::NotGranted { .. }));
        assert!(err.for_model().contains("Ask the user to grant access"));
    }

    #[tokio::test]
    async fn allow_creates_the_grant_and_the_tool_proceeds() {
        let d = fixture();
        let e = engine();
        let asker = Arc::new(Scripted {
            answer: AskOutcome::Allowed,
            engine: e.clone(),
            asked: Mutex::new(vec![]),
        });
        let c = ToolCx::new(Arc::new(EngineGatekeeper::new(
            e,
            "folder-scout",
            Some(asker.clone()),
        )));
        let out = ListDir
            .run(args(&[("path", d.path().to_str().unwrap())]), &c)
            .await
            .unwrap();
        assert!(out.llm_text.contains("notes.md"));
        assert_eq!(asker.asked.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn deny_is_reported_without_reading_anything() {
        let d = fixture();
        let e = engine();
        let asker = Arc::new(Scripted {
            answer: AskOutcome::Denied,
            engine: e.clone(),
            asked: Mutex::new(vec![]),
        });
        let c = ToolCx::new(Arc::new(EngineGatekeeper::new(e, "nexus", Some(asker))));
        let err = ReadFile
            .run(args(&[("path", d.path().join("notes.md").to_str().unwrap())]), &c)
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::Declined(_)));
        assert!(err.for_model().contains("declined"));
    }

    #[tokio::test]
    async fn protected_paths_never_prompt() {
        let d = fixture();
        let e = Arc::new(PermissionEngine::new(
            DenyList::new([nexus_fs::DenyRule::user(d.path().to_owned(), "private")]),
            Arc::new(MemoryAudit::default()),
        ));
        let asker = Arc::new(Scripted {
            answer: AskOutcome::Allowed,
            engine: e.clone(),
            asked: Mutex::new(vec![]),
        });
        let c = ToolCx::new(Arc::new(EngineGatekeeper::new(e, "nexus", Some(asker.clone()))));
        let err = ListDir
            .run(args(&[("path", d.path().to_str().unwrap())]), &c)
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::Protected(_)));
        assert!(asker.asked.lock().unwrap().is_empty());
    }

    struct FakeCode;
    impl CodeSource for FakeCode {
        fn find(&self, query: &str, _max: usize) -> Result<String, String> {
            Ok(format!("function {query} (a.py:1)\n"))
        }
    }

    #[tokio::test]
    async fn find_symbol_needs_an_indexed_project() {
        let mut c = cx(engine());
        let err = FindSymbol.run(args(&[("query", "calc")]), &c).await.unwrap_err();
        assert!(matches!(err, ToolError::Invalid(m) if m.contains("indexed")));
        c.code = Some(Arc::new(FakeCode));
        let out = FindSymbol.run(args(&[("query", "calc")]), &c).await.unwrap();
        assert!(out.llm_text.contains("function calc (a.py:1)"));
        assert!(out.llm_text.starts_with("<tool_result name=\"find_symbol\""));
    }
    // ---- proposing and web tools ----

    #[derive(Default)]
    struct Staged(Mutex<Vec<(String, Vec<FileOp>)>>);

    impl ProposalSink for Staged {
        fn propose(&self, title: &str, ops: Vec<FileOp>) -> Result<String, String> {
            let mut v = self.0.lock().map_err(|e| e.to_string())?;
            v.push((title.to_owned(), ops));
            Ok(format!("cs{}", v.len()))
        }
    }

    fn write_grant(e: &PermissionEngine, root: &Path) {
        e.add_grant(NewGrant {
            root: root.to_owned(),
            level: GrantLevel::Full,
            recursive: true,
            scope: GrantScope::Session,
            expires_at: None,
            label: None,
        })
        .unwrap();
    }

    fn cx_with_sink(e: Arc<PermissionEngine>) -> (ToolCx, Arc<Staged>) {
        let staged = Arc::new(Staged::default());
        let mut c = cx(e);
        c.proposals = Some(staged.clone());
        (c, staged)
    }

    #[tokio::test]
    async fn write_file_stages_a_create_and_never_touches_disk() {
        let d = fixture();
        let e = engine();
        write_grant(&e, d.path());
        let (c, staged) = cx_with_sink(e);
        let target = d.path().join("new.txt");
        let out = WriteFile
            .run(
                args(&[("path", target.to_str().unwrap()), ("content", "hello")]),
                &c,
            )
            .await
            .unwrap();
        assert!(out.llm_text.contains("NOT written yet"));
        assert!(!target.exists());
        let ops = staged.0.lock().unwrap();
        assert!(matches!(&ops[0].1[0], FileOp::Create { contents, .. } if contents == "hello"));
    }

    struct AutoSink;

    impl ProposalSink for AutoSink {
        fn propose(&self, _title: &str, _ops: Vec<FileOp>) -> Result<String, String> {
            Ok("cs1".into())
        }
        fn auto_apply(&self, _id: &str) -> AutoApply {
            AutoApply::Applied
        }
    }

    #[tokio::test]
    async fn auto_applied_proposals_report_done_not_pending() {
        let d = fixture();
        let e = engine();
        write_grant(&e, d.path());
        let (mut c, _) = cx_with_sink(e);
        c.proposals = Some(Arc::new(AutoSink));
        let target = d.path().join("auto.txt");
        let out = WriteFile
            .run(args(&[("path", target.to_str().unwrap()), ("content", "x")]), &c)
            .await
            .unwrap();
        assert!(out.llm_text.starts_with("DONE:"));
        assert!(out.summary.starts_with("Applied:"));
        assert_eq!(out.ui.unwrap()["state"], "applied");
    }

    #[tokio::test]
    async fn write_file_on_an_existing_file_carries_the_base_hash() {
        let d = fixture();
        let e = engine();
        write_grant(&e, d.path());
        let (c, staged) = cx_with_sink(e);
        let target = d.path().join("notes.md");
        WriteFile
            .run(
                args(&[("path", target.to_str().unwrap()), ("content", "new")]),
                &c,
            )
            .await
            .unwrap();
        let ops = staged.0.lock().unwrap();
        let expect = nexus_fs::hash_file(&target).unwrap();
        assert!(matches!(&ops[0].1[0], FileOp::Write { base_hash: Some(h), .. } if *h == expect));
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            "# Notes\nbuy milk\nCall Bob\n"
        );
    }

    #[tokio::test]
    async fn write_file_needs_a_write_grant_and_a_sink() {
        let d = fixture();
        let e = engine();
        grant(&e, d.path(), true); // read-only
        let (c, staged) = cx_with_sink(e.clone());
        let err = WriteFile
            .run(
                args(&[
                    ("path", d.path().join("x.txt").to_str().unwrap()),
                    ("content", "a"),
                ]),
                &c,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::NotGranted { .. }), "{err}");
        assert!(staged.0.lock().unwrap().is_empty());
        let err = WriteFile
            .run(
                args(&[
                    ("path", d.path().join("x.txt").to_str().unwrap()),
                    ("content", "a"),
                ]),
                &cx(e),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::Unavailable(_)), "{err}");
    }

    #[tokio::test]
    async fn edit_file_replaces_one_unique_passage() {
        let d = fixture();
        let e = engine();
        write_grant(&e, d.path());
        let (c, staged) = cx_with_sink(e);
        let target = d.path().join("notes.md");
        let p = target.to_str().unwrap();
        EditFile
            .run(
                args(&[("path", p), ("old_text", "buy milk"), ("new_text", "buy tea")]),
                &c,
            )
            .await
            .unwrap();
        {
            let ops = staged.0.lock().unwrap();
            match &ops[0].1[0] {
                FileOp::Edit { edits, .. } => {
                    assert_eq!((edits[0].start, edits[0].end), (8, 16));
                    assert_eq!(edits[0].new_text, "buy tea");
                }
                other => panic!("{other:?}"),
            }
        }
        let err = EditFile
            .run(args(&[("path", p), ("old_text", "nope"), ("new_text", "x")]), &c)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[tokio::test]
    async fn edit_file_rejects_an_ambiguous_passage() {
        let d = fixture();
        std::fs::write(d.path().join("dup.txt"), "a\na\n").unwrap();
        let e = engine();
        write_grant(&e, d.path());
        let (c, _) = cx_with_sink(e);
        let err = EditFile
            .run(
                args(&[
                    ("path", d.path().join("dup.txt").to_str().unwrap()),
                    ("old_text", "a"),
                    ("new_text", "b"),
                ]),
                &c,
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("more than once"));
    }

    #[tokio::test]
    async fn move_and_trash_only_stage() {
        let d = fixture();
        let e = engine();
        write_grant(&e, d.path());
        let (c, staged) = cx_with_sink(e);
        let from = d.path().join("notes.md");
        let to = d.path().join("renamed.md");
        MoveFile
            .run(
                args(&[("path", from.to_str().unwrap()), ("to", to.to_str().unwrap())]),
                &c,
            )
            .await
            .unwrap();
        TrashFile
            .run(args(&[("path", from.to_str().unwrap())]), &c)
            .await
            .unwrap();
        assert!(from.exists() && !to.exists());
        assert_eq!(staged.0.lock().unwrap().len(), 2);
    }

    struct FakeWeb;

    #[async_trait]
    impl WebSource for FakeWeb {
        async fn search(&self, query: &str, _max: usize) -> Result<Vec<WebHit>, String> {
            Ok(vec![WebHit {
                title: format!("About {query}"),
                url: "https://example.org/a".into(),
                snippet: "A snippet.".into(),
            }])
        }
        async fn fetch(&self, url: &str, _max: usize) -> Result<WebPage, String> {
            Ok(WebPage {
                url: url.into(),
                title: "Example".into(),
                text: "Page text".into(),
            })
        }
    }

    #[tokio::test]
    async fn web_tools_are_unavailable_until_a_source_is_set() {
        let mut c = cx(engine());
        let err = SearchWeb.run(args(&[("query", "rust")]), &c).await.unwrap_err();
        assert!(matches!(err, ToolError::Unavailable(_)));
        c.web = Some(Arc::new(FakeWeb));
        let out = SearchWeb.run(args(&[("query", "rust")]), &c).await.unwrap();
        assert!(out.llm_text.contains("https://example.org/a"));
        let page = FetchUrl
            .run(args(&[("url", "https://example.org/a")]), &c)
            .await
            .unwrap();
        assert!(page.llm_text.contains("Page text"));
    }
}
