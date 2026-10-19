//! A plain-text view of a [`ActivitySnapshot`], for the terminal (`examples/top.rs`) and for tests (T19.16).

use std::fmt::Write;

use crate::model::{ActivitySnapshot, AgentStatus, AgentView};

/// `4.2M`, `13.5k`, `812`.
pub fn fmt_tokens(n: u64) -> String {
    match n {
        0..=999 => n.to_string(),
        1_000..=999_999 => format!("{:.1}k", n as f64 / 1e3),
        _ => format!("{:.1}M", n as f64 / 1e6),
    }
}

/// `3h 00m`, `12m 05s`, `42s`.
pub fn fmt_duration(ms: i64) -> String {
    let s = (ms / 1000).max(0);
    if s >= 3600 {
        format!("{}h {:02}m", s / 3600, (s % 3600) / 60)
    } else if s >= 60 {
        format!("{}m {:02}s", s / 60, s % 60)
    } else {
        format!("{s}s")
    }
}

fn glyph(s: AgentStatus) -> &'static str {
    match s {
        AgentStatus::Running => "●",
        AgentStatus::Idle => "○",
        AgentStatus::Stalled => "◌",
        AgentStatus::Errored => "✖",
    }
}

fn status_word(s: AgentStatus) -> &'static str {
    match s {
        AgentStatus::Running => "running",
        AgentStatus::Idle => "idle",
        AgentStatus::Stalled => "stalled",
        AgentStatus::Errored => "errored",
    }
}

fn row(a: &AgentView, now_ms: i64) -> String {
    format!(
        "  {} {:<30} {:<8} {:<12} {:>7} tok  {:>7}  {}",
        glyph(a.status),
        a.label,
        status_word(a.status),
        a.tool.as_deref().unwrap_or("-"),
        fmt_tokens(a.tokens.new_tokens()),
        fmt_duration(now_ms - a.started_ms),
        if a.errors > 0 {
            format!("{} err", a.errors)
        } else {
            String::new()
        }
    )
}

/// Header, one block per project, conflicts last.
pub fn table(s: &ActivitySnapshot) -> String {
    let mut o = String::new();
    let t = &s.totals;
    let _ = writeln!(
        o,
        "NANONEXUS ACTIVITY · {} projects · {} agents · {} running · {} tokens · up {}",
        s.projects.len(),
        t.agents,
        t.active,
        fmt_tokens(t.new_tokens),
        fmt_duration(t.uptime_ms)
    );
    for src in s.sources.iter().filter(|x| x.enabled || x.agents > 0) {
        let _ = writeln!(
            o,
            "  {}: {} agents, {} running",
            src.label, src.agents, src.running
        );
    }
    for p in &s.projects {
        let _ = writeln!(
            o,
            "\n{} [{}] · {} agents · {} running",
            p.label,
            s.sources
                .iter()
                .find(|x| x.source == p.source)
                .map_or("", |x| x.label.as_str()),
            p.agents,
            p.running
        );
        for a in s.agents.iter().filter(|a| a.project_id == p.id) {
            let _ = writeln!(o, "{}", row(a, s.at_ms));
        }
    }
    for c in &s.conflicts {
        let _ = writeln!(
            o,
            "\n⚠ FILE CONFLICT {} ← {} agents",
            c.path_label,
            c.agents.len()
        );
    }
    if s.drift.lines > 100 && s.drift.bad * 20 > s.drift.lines {
        let _ = writeln!(
            o,
            "\n(log format may have changed: {} of {} lines unreadable)",
            s.drift.bad, s.drift.lines
        );
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aggregate::{Aggregator, Config};
    use crate::demo::DemoFeed;

    #[test]
    fn formats() {
        assert_eq!(fmt_tokens(812), "812");
        assert_eq!(fmt_tokens(13_500), "13.5k");
        assert_eq!(fmt_tokens(4_200_000), "4.2M");
        assert_eq!(fmt_duration(10_800_000), "3h 00m");
        assert_eq!(fmt_duration(725_000), "12m 05s");
        assert_eq!(fmt_duration(-5), "0s");
    }

    #[test]
    fn demo_table_lists_projects_agents_and_the_conflict() {
        let now = 1_800_000_000_000;
        let mut g = Aggregator::new(Config::default());
        for o in DemoFeed::new(7, now).history(now) {
            g.ingest(o);
        }
        let mut f = DemoFeed::new(7, now);
        let _ = f.history(now);
        for o in f.tick(now + 2_000) {
            g.ingest(o);
        }
        let t = table(&g.snapshot(now + 2_000));
        assert!(t.contains("3 projects · 19 agents"), "{t}");
        assert!(t.contains("work/atlas-api"));
        assert!(t.contains("FILE CONFLICT cart/Cart.tsx"));
        assert!(t.contains("✖") && t.contains("◌"));
    }
}
