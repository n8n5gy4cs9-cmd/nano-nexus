//! Folds observations into agent state and renders a [`ActivitySnapshot`] (OBSERVE.md §5, §6).

use std::collections::{BTreeMap, HashMap, VecDeque};

use crate::model::{
    ActivitySnapshot, ActivitySource, ActivityTotals, AgentStatus, AgentView, ConflictView, LogDrift, Obs,
    ObsKind, ProjectView, SourceView, TokenTotals, base_name, short_model, short_path,
};

/// Timing rules. Defaults are the ones in OBSERVE.md §5.
#[derive(Clone, Debug)]
pub struct Config {
    /// Agents silent for longer than this drop off the graph.
    pub window_ms: i64,
    /// In a turn with no open tool and no news for this long: stalled.
    pub stall_ms: i64,
    /// Same, while a tool call is open (builds and tests are slow).
    pub stall_tool_ms: i64,
    /// A stalled agent silent for this long counts as abandoned (idle).
    pub abandon_ms: i64,
    /// Two agents on one file within this window: conflict.
    pub conflict_ms: i64,
    /// An error shows as `Errored` for this long.
    pub error_ms: i64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            window_ms: 6 * 3_600_000,
            stall_ms: 180_000,
            stall_tool_ms: 600_000,
            abandon_ms: 3_600_000,
            conflict_ms: 120_000,
            error_ms: 120_000,
        }
    }
}

struct OpenTool {
    tool: String,
    files: Vec<String>,
}

struct AgentState {
    source: ActivitySource,
    agent: String,
    parent: Option<String>,
    project: String,
    model: Option<String>,
    title: Option<String>,
    started_ms: i64,
    last_ms: i64,
    tokens: TokenTotals,
    events: u64,
    messages: u32,
    errors: u32,
    in_turn: bool,
    errored_at: Option<i64>,
    open: BTreeMap<String, OpenTool>,
    /// normalised path → (display label, last touch)
    edits: HashMap<String, (String, i64)>,
    recent: VecDeque<i64>,
}

impl AgentState {
    fn id(&self) -> String {
        format!("{}:{}", self.source.id(), self.agent)
    }

    fn project_id(&self) -> String {
        format!("{}:{}", self.source.id(), self.project)
    }

    fn label(&self) -> String {
        if let Some(t) = &self.title {
            return t.clone();
        }
        let base = self
            .model
            .as_deref()
            .map(short_model)
            .unwrap_or_else(|| self.source.label().to_string());
        let id = self.agent.rsplit('/').next().unwrap_or(&self.agent);
        let tail: String = id
            .chars()
            .rev()
            .take(4)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        if self.parent.is_some() {
            format!("Subagent · {base} · {tail}")
        } else {
            format!("{base} · {tail}")
        }
    }

    fn status(&self, now: i64, cfg: &Config) -> AgentStatus {
        if self.errored_at.is_some_and(|e| now - e < cfg.error_ms) {
            return AgentStatus::Errored;
        }
        if !self.in_turn {
            return AgentStatus::Idle;
        }
        let age = now - self.last_ms;
        let limit = if self.open.is_empty() {
            cfg.stall_ms
        } else {
            cfg.stall_tool_ms
        };
        if age < limit {
            AgentStatus::Running
        } else if age < cfg.abandon_ms {
            AgentStatus::Stalled
        } else {
            AgentStatus::Idle
        }
    }

    fn rate(&self, now: i64) -> f32 {
        let n = self
            .recent
            .iter()
            .filter(|t| now - **t <= 10_000 && **t <= now)
            .count();
        n as f32 / 10.0
    }
}

/// Lexically normalise a path for comparison: separators, `.`/`..`, case on case-insensitive systems.
fn path_key(project: &str, path: &str) -> String {
    let p = path.replace('\\', "/");
    let is_abs = p.starts_with('/') || p.as_bytes().get(1) == Some(&b':');
    let joined = if is_abs || project.is_empty() {
        p
    } else {
        format!("{}/{}", project.replace('\\', "/"), p)
    };
    let mut parts: Vec<&str> = Vec::new();
    for c in joined.split('/') {
        match c {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            c => parts.push(c),
        }
    }
    let key = parts.join("/");
    if cfg!(any(windows, target_os = "macos")) {
        key.to_lowercase()
    } else {
        key
    }
}

/// Folds [`Obs`] into per-agent state.
pub struct Aggregator {
    cfg: Config,
    agents: BTreeMap<String, AgentState>,
    enabled: [bool; 3],
    pub drift: LogDrift,
}

impl Aggregator {
    pub fn new(cfg: Config) -> Self {
        Self {
            cfg,
            agents: BTreeMap::new(),
            enabled: [true; 3],
            drift: LogDrift::default(),
        }
    }

    pub fn config(&self) -> &Config {
        &self.cfg
    }

    pub fn set_config(&mut self, cfg: Config) {
        self.cfg = cfg;
    }

    /// Which sources the UI shows as switched on (display only).
    pub fn set_enabled(&mut self, source: ActivitySource, on: bool) {
        self.enabled[source as usize] = on;
    }

    pub fn is_empty(&self) -> bool {
        self.agents.is_empty()
    }

    /// Forget a whole source (it was switched off).
    pub fn clear_source(&mut self, source: ActivitySource) {
        self.agents.retain(|_, a| a.source != source);
    }

    pub fn ingest(&mut self, o: Obs) {
        let id = format!("{}:{}", o.source.id(), o.agent);
        let st = self.agents.entry(id).or_insert_with(|| AgentState {
            source: o.source,
            agent: o.agent.clone(),
            parent: o.parent.clone(),
            project: o.project.clone(),
            model: None,
            title: None,
            started_ms: o.at_ms,
            last_ms: o.at_ms,
            tokens: TokenTotals::default(),
            events: 0,
            messages: 0,
            errors: 0,
            in_turn: false,
            errored_at: None,
            open: BTreeMap::new(),
            edits: HashMap::new(),
            recent: VecDeque::new(),
        });
        st.started_ms = st.started_ms.min(o.at_ms);
        st.last_ms = st.last_ms.max(o.at_ms);
        st.events += 1;
        st.recent.push_back(o.at_ms);
        while st.recent.len() > 256 {
            st.recent.pop_front();
        }
        if st.project.is_empty() && !o.project.is_empty() {
            st.project = o.project.clone();
        }
        if st.parent.is_none() {
            st.parent = o.parent.clone();
        }
        match o.kind {
            ObsKind::Start { model } => {
                if model.is_some() {
                    st.model = model;
                }
            }
            ObsKind::Model(m) => st.model = Some(m),
            ObsKind::Title(t) => st.title = Some(t),
            ObsKind::Tokens(t) => st.tokens.add(&t),
            ObsKind::ToolStart { id, tool, files } => {
                st.in_turn = true;
                st.errored_at = None;
                for f in &files {
                    let key = path_key(&st.project, f);
                    st.edits.insert(key, (short_path(f), o.at_ms));
                }
                if st.edits.len() > 64 {
                    let cutoff = o.at_ms - 600_000;
                    st.edits.retain(|_, (_, at)| *at >= cutoff);
                }
                st.open.insert(id, OpenTool { tool, files });
            }
            ObsKind::ToolEnd { id, ok } => {
                st.open.remove(&id);
                if !ok {
                    st.errors += 1;
                }
            }
            ObsKind::Message(_) => {
                st.messages += 1;
                st.errored_at = None;
                st.in_turn = true;
            }
            ObsKind::Error => {
                st.errors += 1;
                st.errored_at = Some(o.at_ms);
                st.in_turn = false;
                st.open.clear();
            }
            ObsKind::End => {
                st.in_turn = false;
                st.open.clear();
            }
        }
    }

    /// Drop agents that have been silent for longer than the window.
    pub fn gc(&mut self, now_ms: i64) {
        let cutoff = now_ms - self.cfg.window_ms;
        self.agents.retain(|_, a| a.last_ms >= cutoff);
    }

    pub fn snapshot(&self, now_ms: i64) -> ActivitySnapshot {
        let cutoff = now_ms - self.cfg.window_ms;
        let visible: Vec<&AgentState> = self
            .agents
            .values()
            .filter(|a| a.last_ms >= cutoff && a.started_ms <= now_ms + 60_000)
            .collect();
        let mut agents: Vec<AgentView> = Vec::with_capacity(visible.len());
        for a in &visible {
            let status = a.status(now_ms, &self.cfg);
            let tool = a.open.values().last().map(|t| t.tool.clone());
            let parent = a
                .parent
                .as_ref()
                .map(|p| format!("{}:{}", a.source.id(), p))
                .filter(|pid| self.agents.contains_key(pid));
            agents.push(AgentView {
                id: a.id(),
                label: a.label(),
                source: a.source,
                project_id: a.project_id(),
                parent,
                model: a.model.clone(),
                status,
                tool,
                tokens: a.tokens,
                started_ms: a.started_ms,
                last_ms: a.last_ms,
                events: a.events,
                messages: a.messages,
                errors: a.errors,
                events_per_s: a.rate(now_ms),
            });
        }
        let mut projects: BTreeMap<String, ProjectView> = BTreeMap::new();
        for (a, v) in visible.iter().zip(&agents) {
            let p = projects
                .entry(v.project_id.clone())
                .or_insert_with(|| ProjectView {
                    id: v.project_id.clone(),
                    label: if a.project.is_empty() {
                        "unknown project".into()
                    } else {
                        let l = short_path(&a.project);
                        if l.is_empty() { base_name(&a.project) } else { l }
                    },
                    source: a.source,
                    agents: 0,
                    running: 0,
                    new_tokens: 0,
                    last_ms: 0,
                });
            p.agents += 1;
            if v.status == AgentStatus::Running {
                p.running += 1;
            }
            p.new_tokens += v.tokens.new_tokens();
            p.last_ms = p.last_ms.max(v.last_ms);
        }
        let sources = ActivitySource::ALL
            .iter()
            .map(|s| {
                let mine: Vec<&AgentView> = agents.iter().filter(|a| a.source == *s).collect();
                SourceView {
                    source: *s,
                    label: s.label().to_string(),
                    enabled: self.enabled[*s as usize],
                    agents: mine.len() as u32,
                    running: mine.iter().filter(|a| a.status == AgentStatus::Running).count() as u32,
                    last_ms: mine.iter().map(|a| a.last_ms).max(),
                }
            })
            .collect();
        let totals = ActivityTotals {
            tasks: agents.len() as u32,
            tasks_done: agents.iter().filter(|a| a.status == AgentStatus::Idle).count() as u32,
            events: agents.iter().map(|a| a.events).sum(),
            active: agents.iter().filter(|a| a.status == AgentStatus::Running).count() as u32,
            agents: agents.len() as u32,
            uptime_ms: agents
                .iter()
                .map(|a| a.started_ms)
                .min()
                .map_or(0, |s| (now_ms - s).max(0)),
            new_tokens: agents.iter().map(|a| a.tokens.new_tokens()).sum(),
            cache_read_tokens: agents.iter().map(|a| a.tokens.cache_read).sum(),
        };
        ActivitySnapshot {
            at_ms: now_ms,
            totals,
            sources,
            projects: projects.into_values().collect(),
            conflicts: self.conflicts(&visible, now_ms),
            agents,
            drift: self.drift,
        }
    }

    /// Files that two different agents touched within the conflict window (or hold open right now).
    fn conflicts(&self, visible: &[&AgentState], now_ms: i64) -> Vec<ConflictView> {
        // key → (label, [(agent id, touched)])
        let mut by_file: BTreeMap<String, (String, Vec<(String, i64)>)> = BTreeMap::new();
        for a in visible {
            let mut mine: HashMap<String, (String, i64)> = HashMap::new();
            for (k, (label, at)) in &a.edits {
                if now_ms - *at <= self.cfg.conflict_ms {
                    mine.insert(k.clone(), (label.clone(), *at));
                }
            }
            for t in a.open.values() {
                for f in &t.files {
                    mine.insert(path_key(&a.project, f), (short_path(f), now_ms));
                }
            }
            for (k, (label, at)) in mine {
                let e = by_file.entry(k).or_insert_with(|| (label, Vec::new()));
                e.1.push((a.id(), at));
            }
        }
        by_file
            .into_values()
            .filter(|(_, who)| who.len() >= 2)
            .map(|(label, who)| ConflictView {
                path_label: label,
                since_ms: who.iter().map(|(_, at)| *at).min().unwrap_or(now_ms),
                agents: who.into_iter().map(|(id, _)| id).collect(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Role;

    const T0: i64 = 1_800_000_000_000;

    fn obs(agent: &str, at: i64, kind: ObsKind) -> Obs {
        Obs {
            source: ActivitySource::ClaudeCode,
            session: agent.split('/').next().unwrap_or(agent).into(),
            agent: agent.into(),
            parent: agent.split_once('/').map(|(s, _)| s.to_string()),
            project: "/home/me/app".into(),
            at_ms: at,
            kind,
        }
    }

    fn tool(id: &str, name: &str, files: &[&str]) -> ObsKind {
        ObsKind::ToolStart {
            id: id.into(),
            tool: name.into(),
            files: files.iter().map(|f| f.to_string()).collect(),
        }
    }

    fn status_of(agg: &Aggregator, now: i64, id: &str) -> AgentStatus {
        agg.snapshot(now)
            .agents
            .iter()
            .find(|a| a.id == id)
            .map(|a| a.status)
            .expect("agent present")
    }

    #[test]
    fn running_then_stalled_then_idle() {
        let mut g = Aggregator::new(Config::default());
        g.ingest(obs("s1", T0, ObsKind::Message(Role::User)));
        assert_eq!(status_of(&g, T0 + 5_000, "claude_code:s1"), AgentStatus::Running);
        assert_eq!(
            status_of(&g, T0 + 170_000, "claude_code:s1"),
            AgentStatus::Running
        );
        assert_eq!(
            status_of(&g, T0 + 190_000, "claude_code:s1"),
            AgentStatus::Stalled
        );
        assert_eq!(status_of(&g, T0 + 3_700_000, "claude_code:s1"), AgentStatus::Idle);
        g.ingest(obs("s1", T0 + 4_000, ObsKind::End));
        assert_eq!(status_of(&g, T0 + 190_000, "claude_code:s1"), AgentStatus::Idle);
    }

    #[test]
    fn open_tools_get_a_longer_leash() {
        let mut g = Aggregator::new(Config::default());
        g.ingest(obs("s1", T0, tool("t1", "Bash", &[])));
        assert_eq!(
            status_of(&g, T0 + 300_000, "claude_code:s1"),
            AgentStatus::Running
        );
        assert_eq!(
            status_of(&g, T0 + 700_000, "claude_code:s1"),
            AgentStatus::Stalled
        );
        g.ingest(obs(
            "s1",
            T0 + 10_000,
            ObsKind::ToolEnd {
                id: "t1".into(),
                ok: true,
            },
        ));
        // Tool closed at +10 s: the 3-minute leash now applies.
        assert_eq!(
            status_of(&g, T0 + 200_000, "claude_code:s1"),
            AgentStatus::Stalled
        );
    }

    #[test]
    fn errors_show_then_fade_and_clear_on_progress() {
        let mut g = Aggregator::new(Config::default());
        g.ingest(obs("s1", T0, ObsKind::Message(Role::User)));
        g.ingest(obs("s1", T0 + 1_000, ObsKind::Error));
        assert_eq!(status_of(&g, T0 + 5_000, "claude_code:s1"), AgentStatus::Errored);
        assert_eq!(status_of(&g, T0 + 200_000, "claude_code:s1"), AgentStatus::Idle);
        g.ingest(obs("s1", T0 + 6_000, ObsKind::Message(Role::Assistant)));
        assert_eq!(status_of(&g, T0 + 7_000, "claude_code:s1"), AgentStatus::Running);
        let snap = g.snapshot(T0 + 7_000);
        assert_eq!(snap.agents[0].errors, 1);
    }

    #[test]
    fn failed_tools_count_but_do_not_mark_the_agent_errored() {
        let mut g = Aggregator::new(Config::default());
        g.ingest(obs("s1", T0, tool("t1", "Bash", &[])));
        g.ingest(obs(
            "s1",
            T0 + 1_000,
            ObsKind::ToolEnd {
                id: "t1".into(),
                ok: false,
            },
        ));
        let s = g.snapshot(T0 + 2_000);
        assert_eq!(s.agents[0].errors, 1);
        assert_eq!(s.agents[0].status, AgentStatus::Running);
    }

    #[test]
    fn two_agents_on_one_file_conflict_and_it_expires() {
        let mut g = Aggregator::new(Config::default());
        g.ingest(obs("a", T0, tool("t1", "Edit", &["/home/me/app/src/x.rs"])));
        g.ingest(obs(
            "a",
            T0 + 100,
            ObsKind::ToolEnd {
                id: "t1".into(),
                ok: true,
            },
        ));
        g.ingest(obs(
            "b",
            T0 + 5_000,
            tool("t2", "Write", &["/home/me/app/src/../src/x.rs"]),
        ));
        let s = g.snapshot(T0 + 6_000);
        assert_eq!(s.conflicts.len(), 1, "{:?}", s.conflicts);
        assert_eq!(s.conflicts[0].path_label, "src/x.rs");
        assert_eq!(s.conflicts[0].agents.len(), 2);
        // `a`'s touch aged out of the window and `b` is alone on the file.
        assert_eq!(g.snapshot(T0 + 600_000).conflicts.len(), 0, "a's touch is old");
    }

    #[test]
    fn one_agent_editing_twice_is_not_a_conflict() {
        let mut g = Aggregator::new(Config::default());
        g.ingest(obs("a", T0, tool("t1", "Edit", &["/x/y.rs"])));
        g.ingest(obs("a", T0 + 10, tool("t2", "Edit", &["/x/y.rs"])));
        assert!(g.snapshot(T0 + 20).conflicts.is_empty());
    }

    #[test]
    fn relative_patch_paths_resolve_against_the_project() {
        let mut g = Aggregator::new(Config::default());
        g.ingest(obs("a", T0, tool("t1", "apply_patch", &["src/x.rs"])));
        let mut b = obs("b", T0 + 10, tool("t2", "Edit", &["/home/me/app/src/x.rs"]));
        b.source = ActivitySource::Codex;
        g.ingest(b);
        assert_eq!(g.snapshot(T0 + 20).conflicts.len(), 1, "across sources too");
    }

    #[test]
    fn totals_projects_and_subagent_nesting() {
        let mut g = Aggregator::new(Config::default());
        g.ingest(obs("s1", T0, ObsKind::Model("claude-opus-5-5".into())));
        g.ingest(obs(
            "s1",
            T0 + 10,
            ObsKind::Tokens(TokenTotals {
                input: 1,
                output: 2,
                cache_read: 50,
                cache_write: 3,
            }),
        ));
        g.ingest(obs("s1/a1", T0 + 20, ObsKind::Message(Role::Assistant)));
        let s = g.snapshot(T0 + 1_000);
        assert_eq!(s.totals.agents, 2);
        assert_eq!(s.totals.new_tokens, 6);
        assert_eq!(s.totals.cache_read_tokens, 50);
        assert_eq!(s.totals.uptime_ms, 1_000);
        assert_eq!(s.projects.len(), 1);
        assert_eq!(s.projects[0].label, "me/app");
        let sub = s.agents.iter().find(|a| a.id == "claude_code:s1/a1").unwrap();
        assert_eq!(sub.parent.as_deref(), Some("claude_code:s1"));
        assert!(sub.label.starts_with("Subagent"));
        let main = s.agents.iter().find(|a| a.id == "claude_code:s1").unwrap();
        assert_eq!(main.label, "Opus 5.5 · s1");
        assert!(sub.label.ends_with("· a1"), "{}", sub.label);
        assert!(main.events_per_s > 0.0);
    }

    #[test]
    fn old_agents_leave_the_graph() {
        let mut g = Aggregator::new(Config::default());
        g.ingest(obs("s1", T0, ObsKind::Message(Role::User)));
        assert_eq!(g.snapshot(T0 + 7 * 3_600_000).agents.len(), 0);
        g.gc(T0 + 7 * 3_600_000);
        assert!(g.is_empty());
    }
}
