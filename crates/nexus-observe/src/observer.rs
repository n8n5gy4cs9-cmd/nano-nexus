//! The facade the app uses: owns the tailer, aggregator and replay buffer, and turns state changes into
//! "needs attention" events (OBSERVE.md §1, §5).

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crate::aggregate::{Aggregator, Config};
use crate::demo::DemoFeed;
use crate::model::{ActivitySnapshot, ActivitySource, AgentStatus, Obs};
use crate::replay::Replay;
use crate::tail::{Root, Tailer};

/// What to watch. Roots are the agents' config folders (`~/.claude`, `~/.codex`); `None` = switched off.
#[derive(Clone, Debug, Default)]
pub struct ObserverConfig {
    pub window_hours: u32,
    pub nanonexus: bool,
    pub claude_root: Option<PathBuf>,
    pub codex_root: Option<PathBuf>,
    /// Show synthetic data only; real sources are not read.
    pub demo: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttentionKind {
    Stalled,
    Errored,
    Conflict,
}

/// Something worth telling the owner about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attention {
    pub kind: AttentionKind,
    pub source: ActivitySource,
    /// Agent label or file label.
    pub subject: String,
    pub project: String,
}

/// `$CLAUDE_CONFIG_DIR` or `~/.claude`.
pub fn default_claude_root() -> Option<PathBuf> {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| home().map(|h| h.join(".claude")))
}

/// `$CODEX_HOME` or `~/.codex`.
pub fn default_codex_root() -> Option<PathBuf> {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| home().map(|h| h.join(".codex")))
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

pub struct Observer {
    cfg: ObserverConfig,
    tailer: Tailer,
    agg: Aggregator,
    replay: Replay,
    pending: Vec<Obs>,
    demo: Option<DemoFeed>,
    demo_started: bool,
    prev_status: HashMap<String, AgentStatus>,
    prev_conflicts: HashSet<String>,
    primed: bool,
}

fn window_ms(cfg: &ObserverConfig) -> i64 {
    i64::from(cfg.window_hours.clamp(1, 48)) * 3_600_000
}

fn roots(cfg: &ObserverConfig) -> Vec<Root> {
    let mut v = Vec::new();
    if cfg.demo {
        return v;
    }
    if let Some(r) = &cfg.claude_root {
        v.push(Root {
            source: ActivitySource::ClaudeCode,
            dir: r.join("projects"),
        });
    }
    if let Some(r) = &cfg.codex_root {
        v.push(Root {
            source: ActivitySource::Codex,
            dir: r.join("sessions"),
        });
    }
    v
}

impl Observer {
    pub fn new(cfg: ObserverConfig) -> Self {
        let w = window_ms(&cfg);
        let mut o = Self {
            tailer: Tailer::new(roots(&cfg), w),
            agg: Aggregator::new(Config {
                window_ms: w,
                ..Config::default()
            }),
            replay: Replay::new(w),
            pending: Vec::new(),
            demo: None,
            demo_started: false,
            prev_status: HashMap::new(),
            prev_conflicts: HashSet::new(),
            primed: false,
            cfg: ObserverConfig::default(),
        };
        o.apply_config(cfg);
        o
    }

    pub fn config(&self) -> &ObserverConfig {
        &self.cfg
    }

    /// Switch sources on or off. State of a source that was switched off is forgotten at once.
    pub fn apply_config(&mut self, cfg: ObserverConfig) {
        let w = window_ms(&cfg);
        let window_changed = w != self.agg.config().window_ms;
        if window_changed {
            self.agg.set_config(Config {
                window_ms: w,
                ..Config::default()
            });
            self.replay = Replay::new(w);
            self.tailer.set_window_ms(w);
        }
        let demo_changed = cfg.demo != self.cfg.demo;
        let claude_on = cfg.claude_root.is_some() && !cfg.demo;
        let codex_on = cfg.codex_root.is_some() && !cfg.demo;
        if !claude_on {
            self.agg.clear_source(ActivitySource::ClaudeCode);
        }
        if !codex_on {
            self.agg.clear_source(ActivitySource::Codex);
        }
        if !cfg.nanonexus {
            self.agg.clear_source(ActivitySource::NanoNexus);
        }
        if demo_changed {
            // Demo data replaces everything; leaving demo mode starts clean.
            for s in ActivitySource::ALL {
                self.agg.clear_source(s);
            }
            self.replay = Replay::new(w);
            self.demo = cfg.demo.then(|| DemoFeed::new(42, 0));
            self.demo_started = false;
            self.primed = false;
        }
        self.agg
            .set_enabled(ActivitySource::NanoNexus, cfg.nanonexus || cfg.demo);
        self.agg
            .set_enabled(ActivitySource::ClaudeCode, claude_on || cfg.demo);
        self.agg.set_enabled(ActivitySource::Codex, codex_on || cfg.demo);
        self.tailer.set_roots(roots(&cfg));
        self.cfg = cfg;
    }

    /// Queue an observation from inside the app. Dropped when NanoNexus' own activity is off.
    pub fn record_self(&mut self, o: Obs) {
        if self.cfg.nanonexus && !self.cfg.demo {
            self.pending.push(o);
            if self.pending.len() > 10_000 {
                self.pending.drain(..5_000);
            }
        }
    }

    pub fn record_self_all(&mut self, v: impl IntoIterator<Item = Obs>) {
        for o in v {
            self.record_self(o);
        }
    }

    /// Read new events, update the picture, return it with anything that needs attention.
    pub fn poll(&mut self, now_ms: i64) -> (ActivitySnapshot, Vec<Attention>) {
        let mut obs: Vec<Obs> = Vec::new();
        if let Some(d) = self.demo.as_mut() {
            if !self.demo_started {
                *d = DemoFeed::new(42, now_ms);
                obs.extend(d.history(now_ms));
                self.demo_started = true;
            }
            obs.extend(d.tick(now_ms));
        } else {
            obs.extend(self.tailer.poll(now_ms));
        }
        obs.append(&mut self.pending);
        obs.retain(|o| o.at_ms <= now_ms + 60_000);
        obs.sort_by_key(|o| o.at_ms);
        if self.replay.is_empty() && !obs.is_empty() {
            self.replay = Replay::backfill(&obs, self.agg.config(), now_ms);
        }
        for o in obs {
            self.agg.ingest(o);
        }
        self.agg.gc(now_ms);
        self.agg.drift = self.tailer.drift;
        let snap = self.agg.snapshot(now_ms);
        self.replay.push(snap.clone());
        let attention = self.attention(&snap);
        (snap, attention)
    }

    /// Replay frame for `t_ms` and the available range.
    pub fn frame_at(&self, t_ms: i64) -> Option<ActivitySnapshot> {
        self.replay.frame_at(t_ms).cloned()
    }

    /// Files currently being followed (0 when every external source is off).
    pub fn tracked_files(&self) -> usize {
        self.tailer.tracked_files()
    }

    pub fn replay_range(&self) -> Option<(i64, i64)> {
        self.replay.range()
    }

    fn attention(&mut self, snap: &ActivitySnapshot) -> Vec<Attention> {
        let project_of = |id: &str| -> String {
            snap.projects
                .iter()
                .find(|p| p.id == id)
                .map(|p| p.label.clone())
                .unwrap_or_default()
        };
        let mut out = Vec::new();
        let mut status = HashMap::new();
        for a in &snap.agents {
            status.insert(a.id.clone(), a.status);
            let was = self.prev_status.get(&a.id).copied();
            let kind = match (was, a.status) {
                (Some(w), AgentStatus::Stalled) if w != AgentStatus::Stalled => Some(AttentionKind::Stalled),
                (Some(w), AgentStatus::Errored) if w != AgentStatus::Errored => Some(AttentionKind::Errored),
                _ => None,
            };
            if let Some(kind) = kind
                && self.primed
            {
                out.push(Attention {
                    kind,
                    source: a.source,
                    subject: a.label.clone(),
                    project: project_of(&a.project_id),
                });
            }
        }
        let mut conflicts = HashSet::new();
        for c in &snap.conflicts {
            let key = format!("{}|{}", c.path_label, c.agents.join(","));
            if !self.prev_conflicts.contains(&key) && self.primed {
                let src = snap
                    .agents
                    .iter()
                    .find(|a| c.agents.first() == Some(&a.id))
                    .map(|a| (a.source, project_of(&a.project_id)));
                out.push(Attention {
                    kind: AttentionKind::Conflict,
                    source: src.as_ref().map_or(ActivitySource::ClaudeCode, |s| s.0),
                    subject: c.path_label.clone(),
                    project: src.map(|s| s.1).unwrap_or_default(),
                });
            }
            conflicts.insert(key);
        }
        self.prev_status = status;
        self.prev_conflicts = conflicts;
        self.primed = true;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ObsKind, Role};
    use crate::selfsrc;

    const NOW: i64 = 1_800_000_000_000;

    fn cfg() -> ObserverConfig {
        ObserverConfig {
            window_hours: 6,
            nanonexus: true,
            ..Default::default()
        }
    }

    #[test]
    fn self_activity_appears_and_switching_off_forgets_it() {
        let mut o = Observer::new(cfg());
        o.record_self_all(selfsrc::chat_started("c1", "Nexus", "m", NOW));
        let (s, _) = o.poll(NOW + 100);
        assert_eq!(s.totals.agents, 1);
        o.apply_config(ObserverConfig {
            nanonexus: false,
            ..cfg()
        });
        o.record_self(selfsrc::chat_finished("c1", true, NOW + 200));
        let (s, _) = o.poll(NOW + 300);
        assert_eq!(s.totals.agents, 0, "off = nothing recorded, nothing shown");
    }

    #[test]
    fn attention_fires_on_transitions_only_and_not_on_the_first_poll() {
        let mut o = Observer::new(cfg());
        o.record_self_all(selfsrc::chat_started("c1", "Nexus", "m", NOW));
        let (_, att) = o.poll(NOW + 1_000);
        assert!(att.is_empty());
        let (_, att) = o.poll(NOW + 200_000);
        assert_eq!(att.len(), 1);
        assert_eq!(att[0].kind, AttentionKind::Stalled);
        let (_, att) = o.poll(NOW + 210_000);
        assert!(att.is_empty(), "still stalled: no repeat");
        o.record_self(selfsrc::chat_finished("c1", false, NOW + 220_000));
        let (_, att) = o.poll(NOW + 221_000);
        assert_eq!(att[0].kind, AttentionKind::Errored);
    }

    #[test]
    fn conflicts_are_reported_once() {
        let mut o = Observer::new(cfg());
        o.poll(NOW);
        o.record_self(selfsrc::tool_started(
            "a",
            "1",
            "propose_write",
            vec!["/x/f.txt".into()],
            NOW + 10,
        ));
        o.record_self(selfsrc::tool_started(
            "b",
            "2",
            "propose_write",
            vec!["/x/f.txt".into()],
            NOW + 20,
        ));
        let (s, att) = o.poll(NOW + 30);
        assert_eq!(s.conflicts.len(), 1);
        assert_eq!(
            att.iter().filter(|a| a.kind == AttentionKind::Conflict).count(),
            1
        );
        let (_, att) = o.poll(NOW + 40);
        assert!(att.is_empty());
    }

    #[test]
    fn demo_mode_shows_synthetic_data_and_ignores_real_sources() {
        let mut o = Observer::new(ObserverConfig {
            demo: true,
            claude_root: Some("/definitely/not/here".into()),
            ..cfg()
        });
        let (s, _) = o.poll(NOW);
        assert_eq!(s.totals.agents, 19);
        assert!(o.replay_range().is_some());
        assert!(o.frame_at(NOW - 3_600_000).is_some());
        o.apply_config(cfg());
        let (s, _) = o.poll(NOW + 1_000);
        assert_eq!(s.totals.agents, 0, "leaving demo clears it");
    }

    #[test]
    fn replay_is_rebuilt_from_the_first_batch() {
        let mut o = Observer::new(cfg());
        o.record_self(Obs {
            source: ActivitySource::NanoNexus,
            session: "s".into(),
            agent: "s".into(),
            parent: None,
            project: "NanoNexus".into(),
            at_ms: NOW - 1_800_000,
            kind: ObsKind::Message(Role::User),
        });
        o.poll(NOW);
        let (a, b) = o.replay_range().unwrap();
        assert!(b - a >= 1_700_000, "history reaches back: {}", b - a);
    }
}
