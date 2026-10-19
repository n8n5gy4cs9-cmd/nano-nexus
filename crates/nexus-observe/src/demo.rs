//! A synthetic, deterministic dataset: 3 projects, 19 agents, a stalled one, an errored one and a file
//! conflict. Used by tests, the UI's "Show demo" button and screenshots, so no real project ever appears
//! in them (T19.10).

use crate::model::{Obs, ObsKind, Role, Source, TokenCount};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mood {
    Running,
    Idle,
    Stalled,
    Errored,
}

struct Spec {
    project: &'static str,
    session: &'static str,
    /// Some(parent session id) for subagents.
    parent: Option<&'static str>,
    model: &'static str,
    mood: Mood,
    /// Edits this file in every cycle (two running agents share one: the conflict).
    file: Option<&'static str>,
    title: &'static str,
}

const CART: &str = "/work/web-storefront/src/cart/Cart.tsx";

const fn s(
    project: &'static str,
    session: &'static str,
    parent: Option<&'static str>,
    model: &'static str,
    mood: Mood,
    file: Option<&'static str>,
    title: &'static str,
) -> Spec {
    Spec {
        project,
        session,
        parent,
        model,
        mood,
        file,
        title,
    }
}

const SPECS: [Spec; 19] = [
    // atlas-api: 7 agents
    s(
        "/work/atlas-api",
        "atl-main",
        None,
        "claude-opus-5-5",
        Mood::Running,
        None,
        "Claude Code",
    ),
    s(
        "/work/atlas-api",
        "atl-mig",
        Some("atl-main"),
        "claude-sonnet-5-5",
        Mood::Running,
        Some("/work/atlas-api/db/migrations/0042.sql"),
        "Write migrations",
    ),
    s(
        "/work/atlas-api",
        "atl-review",
        Some("atl-main"),
        "claude-haiku-5-5",
        Mood::Idle,
        None,
        "Review auth flow",
    ),
    s(
        "/work/atlas-api",
        "atl-openapi",
        Some("atl-main"),
        "claude-haiku-5-5",
        Mood::Idle,
        None,
        "Generate OpenAPI",
    ),
    s(
        "/work/atlas-api",
        "atl-n1",
        Some("atl-main"),
        "claude-sonnet-5-5",
        Mood::Idle,
        None,
        "Fix N+1 queries",
    ),
    s(
        "/work/atlas-api",
        "atl-plan",
        Some("atl-main"),
        "claude-opus-5-5",
        Mood::Idle,
        None,
        "Plan auth refactor",
    ),
    s(
        "/work/atlas-api",
        "atl-rate",
        Some("atl-main"),
        "claude-sonnet-5-5",
        Mood::Idle,
        None,
        "Add rate limiting",
    ),
    // web-storefront: 8 agents
    s(
        "/work/web-storefront",
        "web-main",
        None,
        "claude-opus-5-5",
        Mood::Idle,
        None,
        "Claude Code",
    ),
    s(
        "/work/web-storefront",
        "web-cart",
        Some("web-main"),
        "claude-sonnet-5-5",
        Mood::Running,
        Some(CART),
        "Update cart totals",
    ),
    s(
        "/work/web-storefront",
        "web-cart2",
        Some("web-main"),
        "claude-sonnet-5-5",
        Mood::Running,
        Some(CART),
        "Cart edge cases",
    ),
    s(
        "/work/web-storefront",
        "web-css",
        Some("web-main"),
        "claude-haiku-5-5",
        Mood::Idle,
        None,
        "Rename CSS classes",
    ),
    s(
        "/work/web-storefront",
        "web-prod",
        Some("web-main"),
        "claude-sonnet-5-5",
        Mood::Idle,
        None,
        "Write product tests",
    ),
    s(
        "/work/web-storefront",
        "web-refactor",
        Some("web-main"),
        "claude-sonnet-5-5",
        Mood::Stalled,
        None,
        "Refactor image loader",
    ),
    s(
        "/work/web-storefront",
        "web-bundle",
        Some("web-main"),
        "claude-opus-5-5",
        Mood::Idle,
        None,
        "Audit bundle size",
    ),
    s(
        "/work/web-storefront",
        "web-checkout",
        Some("web-main"),
        "claude-sonnet-5-5",
        Mood::Idle,
        None,
        "Plan checkout flow",
    ),
    // mobile-app: 4 agents
    s(
        "/work/mobile-app",
        "mob-main",
        None,
        "claude-sonnet-5-5",
        Mood::Idle,
        None,
        "Claude Code",
    ),
    s(
        "/work/mobile-app",
        "mob-keyboard",
        Some("mob-main"),
        "claude-haiku-5-5",
        Mood::Errored,
        None,
        "Fix keyboard bug",
    ),
    s(
        "/work/mobile-app",
        "mob-onboard",
        Some("mob-main"),
        "claude-sonnet-5-5",
        Mood::Idle,
        None,
        "Update onboarding",
    ),
    s(
        "/work/mobile-app",
        "mob-push",
        Some("mob-main"),
        "claude-haiku-5-5",
        Mood::Idle,
        None,
        "Plan push notifications",
    ),
];

const TOOLS: [&str; 6] = ["Read", "Grep", "Bash", "Edit", "Glob", "Write"];

/// xorshift64: deterministic and dependency-free.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

fn obs(spec: &Spec, at_ms: i64, kind: ObsKind) -> Obs {
    Obs {
        source: Source::ClaudeCode,
        session: spec.parent.unwrap_or(spec.session).to_string(),
        agent: spec.session.to_string(),
        parent: spec.parent.map(str::to_string),
        project: spec.project.to_string(),
        at_ms,
        kind,
    }
}

/// Endless fake activity. [`DemoFeed::history`] once, then [`DemoFeed::tick`] on every poll.
pub struct DemoFeed {
    rng: Rng,
    /// Per agent: next cycle time and the id of its open tool.
    next_at: Vec<i64>,
    open: Vec<Option<String>>,
    counter: u64,
    last_error_at: i64,
}

impl DemoFeed {
    pub fn new(seed: u64, now_ms: i64) -> Self {
        Self {
            rng: Rng(seed.max(1)),
            next_at: vec![now_ms; SPECS.len()],
            open: vec![None; SPECS.len()],
            counter: 0,
            last_error_at: 0,
        }
    }

    /// Three hours of past activity ending at `now_ms`.
    pub fn history(&mut self, now_ms: i64) -> Vec<Obs> {
        let mut out = Vec::new();
        for (i, sp) in SPECS.iter().enumerate() {
            let life = 20 * 60_000 + self.rng.below(160 * 60_000) as i64;
            let start = now_ms - life;
            out.push(obs(
                sp,
                start,
                ObsKind::Start {
                    model: Some(sp.model.to_string()),
                },
            ));
            out.push(obs(sp, start, ObsKind::Model(sp.model.to_string())));
            out.push(obs(sp, start, ObsKind::Title(sp.title.to_string())));
            // When this agent stopped producing events.
            let end = match sp.mood {
                Mood::Running => now_ms,
                Mood::Idle => now_ms - 8 * 60_000 - self.rng.below(60 * 60_000) as i64,
                Mood::Stalled => now_ms - 5 * 60_000,
                Mood::Errored => now_ms - 30_000,
            }
            .max(start + 60_000);
            let mut t = start;
            while t < end - 20_000 {
                self.cycle(i, t, &mut out, false);
                t += 20_000 + self.rng.below(70_000) as i64;
            }
            match sp.mood {
                Mood::Idle => out.push(obs(sp, end, ObsKind::End)),
                Mood::Stalled => {
                    out.push(obs(sp, end, ObsKind::Message(Role::User)));
                }
                Mood::Errored => out.push(obs(sp, end, ObsKind::Error)),
                Mood::Running => {}
            }
            self.next_at[i] = now_ms;
        }
        out.sort_by_key(|o| o.at_ms);
        out
    }

    /// Events since the previous call, up to `now_ms`.
    pub fn tick(&mut self, now_ms: i64) -> Vec<Obs> {
        let mut out = Vec::new();
        for (i, sp) in SPECS.iter().enumerate() {
            match sp.mood {
                Mood::Running => {
                    while self.next_at[i] <= now_ms {
                        let t = self.next_at[i];
                        self.cycle(i, t, &mut out, true);
                        self.next_at[i] += 700 + self.rng.below(1800) as i64;
                    }
                }
                Mood::Errored if now_ms - self.last_error_at > 90_000 => {
                    self.last_error_at = now_ms;
                    out.push(obs(sp, now_ms, ObsKind::Error));
                }
                _ => {}
            }
        }
        out
    }

    /// One tool cycle: close the previous tool, open a new one, count some tokens.
    fn cycle(&mut self, i: usize, t: i64, out: &mut Vec<Obs>, keep_open: bool) {
        let sp = &SPECS[i];
        if let Some(id) = self.open[i].take() {
            out.push(obs(sp, t, ObsKind::ToolEnd { id, ok: true }));
        }
        self.counter += 1;
        let id = format!("demo-{}", self.counter);
        let tool = TOOLS[self.rng.below(TOOLS.len() as u64) as usize];
        let editing = sp.file.is_some() && self.rng.below(2) == 0;
        let (tool, files) = match (editing, sp.file) {
            (true, Some(f)) => ("Edit", vec![f.to_string()]),
            _ => (tool, Vec::new()),
        };
        out.push(obs(sp, t, ObsKind::Message(Role::Assistant)));
        out.push(obs(
            sp,
            t + 1,
            ObsKind::Tokens(TokenCount {
                input: 40 + self.rng.below(400),
                output: 150 + self.rng.below(900),
                cache_read: 20_000 + self.rng.below(60_000),
                cache_write: 800 + self.rng.below(5_000),
            }),
        ));
        out.push(obs(
            sp,
            t + 2,
            ObsKind::ToolStart {
                id: id.clone(),
                tool: tool.to_string(),
                files,
            },
        ));
        if keep_open {
            self.open[i] = Some(id);
        } else {
            out.push(obs(sp, t + 900, ObsKind::ToolEnd { id, ok: true }));
        }
    }

    pub fn agent_count() -> usize {
        SPECS.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aggregate::{Aggregator, Config};
    use crate::model::Status;

    const NOW: i64 = 1_800_000_000_000;

    fn run(seconds: i64) -> (Aggregator, i64) {
        let mut feed = DemoFeed::new(7, NOW);
        let mut g = Aggregator::new(Config::default());
        for o in feed.history(NOW) {
            g.ingest(o);
        }
        let mut t = NOW;
        while t < NOW + seconds * 1000 {
            t += 1000;
            for o in feed.tick(t) {
                g.ingest(o);
            }
        }
        (g, t)
    }

    #[test]
    fn matches_the_advertised_shape() {
        let (g, t) = run(5);
        let s = g.snapshot(t);
        assert_eq!(s.totals.agents, 19);
        assert_eq!(s.projects.len(), 3);
        assert_eq!(
            s.totals.active,
            4,
            "{:?}",
            s.agents.iter().map(|a| (&a.id, a.status)).collect::<Vec<_>>()
        );
        assert_eq!(s.agents.iter().filter(|a| a.status == Status::Stalled).count(), 1);
        assert_eq!(s.agents.iter().filter(|a| a.status == Status::Errored).count(), 1);
        assert_eq!(s.conflicts.len(), 1);
        assert_eq!(s.conflicts[0].path_label, "cart/Cart.tsx");
        assert!(s.totals.new_tokens > 100_000);
        assert_eq!(s.sources[Source::ClaudeCode as usize].agents, 19);
    }

    #[test]
    fn stays_alive_for_minutes_and_is_deterministic() {
        let (g, t) = run(240);
        let s = g.snapshot(t);
        assert_eq!(s.totals.active, 4);
        assert_eq!(
            s.agents.iter().filter(|a| a.status == Status::Errored).count(),
            1,
            "errors are re-raised"
        );
        let (g2, t2) = run(240);
        assert_eq!(g2.snapshot(t2).totals, s.totals);
        assert_eq!(DemoFeed::agent_count(), 19);
    }
}
