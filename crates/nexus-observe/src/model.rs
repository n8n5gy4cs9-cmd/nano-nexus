//! Observation and snapshot types (OBSERVE.md §2, §6). Everything here is already privacy-reduced.

use serde::{Deserialize, Serialize};
use specta::Type;

/// Where an agent runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ActivitySource {
    NanoNexus,
    ClaudeCode,
    Codex,
}

impl ActivitySource {
    pub const ALL: [ActivitySource; 3] = [
        ActivitySource::NanoNexus,
        ActivitySource::ClaudeCode,
        ActivitySource::Codex,
    ];

    /// Stable lowercase id used in ids and logs.
    pub fn id(self) -> &'static str {
        match self {
            ActivitySource::NanoNexus => "nanonexus",
            ActivitySource::ClaudeCode => "claude_code",
            ActivitySource::Codex => "codex",
        }
    }

    /// Display name.
    pub fn label(self) -> &'static str {
        match self {
            ActivitySource::NanoNexus => "NanoNexus",
            ActivitySource::ClaudeCode => "Claude Code",
            ActivitySource::Codex => "Codex",
        }
    }
}

/// Derived from timing, never read from a log.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    Running,
    Idle,
    Stalled,
    Errored,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    User,
    Assistant,
}

/// Token counts of one message (a delta) or of an agent (a total).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct TokenTotals {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

impl TokenTotals {
    /// Tokens the model actually had to process or produce (cache reads are shown separately).
    pub fn new_tokens(&self) -> u64 {
        self.input + self.output + self.cache_write
    }

    pub fn add(&mut self, o: &TokenTotals) {
        self.input += o.input;
        self.output += o.output;
        self.cache_read += o.cache_read;
        self.cache_write += o.cache_write;
    }
}

/// What happened. Carries no prompt, reply, command or file text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ObsKind {
    /// An agent or session appeared.
    Start {
        model: Option<String>,
    },
    Model(String),
    /// A display name. Only NanoNexus' own source (and the demo) sets one: transcript parsers never do,
    /// because the titles in Claude Code / Codex files are made of the user's text.
    Title(String),
    Tokens(TokenTotals),
    /// A tool call began. `files` is only filled for edit/write tools (the conflict signal).
    ToolStart {
        id: String,
        tool: String,
        files: Vec<String>,
    },
    ToolEnd {
        id: String,
        ok: bool,
    },
    Message(Role),
    /// An API error, aborted turn, or other failure of the agent itself.
    Error,
    /// The agent finished its turn and waits for input.
    End,
}

/// One privacy-reduced observation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Obs {
    pub source: ActivitySource,
    /// Session id (shared by an agent and its subagents).
    pub session: String,
    /// Agent id, unique inside the source; equals `session` for the main agent.
    pub agent: String,
    pub parent: Option<String>,
    /// Working directory / project root as the tool reported it.
    pub project: String,
    pub at_ms: i64,
    pub kind: ObsKind,
}

/// The result of parsing one transcript line.
#[derive(Debug, Default)]
pub struct Parsed {
    pub obs: Vec<Obs>,
    /// The line was not a JSON object of the expected shape (format drift).
    pub bad: bool,
}

/// Turns raw transcript lines into [`Obs`]. One instance per file: it carries the small amount of state
/// (usage already counted, cumulative token totals) needed to emit deltas.
pub trait LineParser: Send {
    fn parse_line(&mut self, line: &str, now_ms: i64) -> Parsed;
}

/// A transcript line the parser could not make sense of (format drift).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct LogDrift {
    pub lines: u64,
    pub bad: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct ActivityTotals {
    pub tasks: u32,
    pub tasks_done: u32,
    pub events: u64,
    pub active: u32,
    pub agents: u32,
    /// Milliseconds since the oldest visible agent started.
    pub uptime_ms: i64,
    pub new_tokens: u64,
    pub cache_read_tokens: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct AgentView {
    /// `source:agent`, unique across sources.
    pub id: String,
    pub label: String,
    pub source: ActivitySource,
    pub project_id: String,
    pub parent: Option<String>,
    pub model: Option<String>,
    pub status: AgentStatus,
    /// The tool it is inside right now.
    pub tool: Option<String>,
    pub tokens: TokenTotals,
    pub started_ms: i64,
    pub last_ms: i64,
    pub events: u64,
    pub messages: u32,
    /// Failed tool calls plus agent errors.
    pub errors: u32,
    /// Events per second over the last 10 s: drives the particle streams.
    pub events_per_s: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct ProjectView {
    /// `source:cwd`.
    pub id: String,
    /// Last two path components.
    pub label: String,
    pub source: ActivitySource,
    pub agents: u32,
    pub running: u32,
    pub new_tokens: u64,
    pub last_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct SourceView {
    pub source: ActivitySource,
    pub label: String,
    pub enabled: bool,
    pub agents: u32,
    pub running: u32,
    pub last_ms: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct ConflictView {
    /// Last two components of the file path.
    pub path_label: String,
    /// Ids (`source:agent`) of the agents touching the file.
    pub agents: Vec<String>,
    pub since_ms: i64,
}

/// The whole picture at one moment (also the replay frame).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct ActivitySnapshot {
    pub at_ms: i64,
    pub totals: ActivityTotals,
    pub sources: Vec<SourceView>,
    pub projects: Vec<ProjectView>,
    pub agents: Vec<AgentView>,
    pub conflicts: Vec<ConflictView>,
    pub drift: LogDrift,
}

/// Last two components of a path, for display (`/home/me/code/app` → `code/app`).
pub fn short_path(p: &str) -> String {
    let parts: Vec<&str> = p
        .split(['/', '\\'])
        .filter(|s| !s.is_empty() && !s.ends_with(':'))
        .collect();
    match parts.len() {
        0 => p.to_string(),
        1 => parts[0].to_string(),
        n => format!("{}/{}", parts[n - 2], parts[n - 1]),
    }
}

/// Last component of a path.
pub fn base_name(p: &str) -> String {
    p.split(['/', '\\'])
        .rfind(|s| !s.is_empty())
        .unwrap_or(p)
        .to_string()
}

/// `claude-sonnet-5-5` → `Sonnet 5.5`, `gpt-5-codex` → `gpt-5-codex`: a short model label.
pub fn short_model(m: &str) -> String {
    let m = m.trim();
    for fam in ["opus", "sonnet", "haiku", "fable"] {
        if let Some(rest) = m
            .strip_prefix("claude-")
            .and_then(|r| r.split_once(fam))
            .map(|(_, tail)| tail)
        {
            let nums: Vec<&str> = rest
                .split(['-', '.'])
                .filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()) && s.len() <= 2)
                .take(2)
                .collect();
            let mut name = fam.to_string();
            if let Some(c) = name.get_mut(0..1) {
                c.make_ascii_uppercase();
            }
            return if nums.is_empty() {
                name
            } else {
                format!("{name} {}", nums.join("."))
            };
        }
    }
    m.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_paths() {
        assert_eq!(short_path("/home/me/code/app"), "code/app");
        assert_eq!(short_path("C:\\Users\\me\\proj"), "me/proj");
        assert_eq!(short_path("/"), "/");
        assert_eq!(short_path("repo"), "repo");
        assert_eq!(base_name("/a/b/c.rs"), "c.rs");
    }

    #[test]
    fn short_model_names() {
        assert_eq!(short_model("claude-sonnet-5-5"), "Sonnet 5.5");
        assert_eq!(short_model("claude-opus-4-1-20250805"), "Opus 4.1");
        assert_eq!(short_model("claude-haiku-5-5"), "Haiku 5.5");
        assert_eq!(short_model("gpt-5-codex"), "gpt-5-codex");
    }

    #[test]
    fn new_tokens_exclude_cache_reads() {
        let t = TokenTotals {
            input: 2,
            output: 3,
            cache_read: 100,
            cache_write: 5,
        };
        assert_eq!(t.new_tokens(), 10);
    }
}
