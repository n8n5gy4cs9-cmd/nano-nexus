//! The app-facing wrapper: one lock around an [`Observer`] plus the pieces the service layer would
//! otherwise have to write (view building, alert texts and their rate limits). Keeping it here means
//! it is covered by `cargo test -p nexus-observe`, and the Tauri glue stays a few lines.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::model::{ActivitySnapshot, Obs};
use crate::observer::{
    Attention, AttentionKind, Observer, ObserverConfig, default_claude_root, default_codex_root,
};

/// What the Activity page renders.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct ObserveView {
    pub snapshot: ActivitySnapshot,
    /// Replay range (first and last frame), when there is history.
    pub replay_from: Option<i64>,
    pub replay_to: Option<i64>,
    /// Synthetic data is showing.
    pub demo: bool,
    /// The agent folders exist on this computer (so the UI can offer to turn them on).
    pub claude_found: bool,
    pub codex_found: bool,
}

/// A notification the service should show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Alert {
    pub title: String,
    pub body: String,
}

/// The switches from Settings → Activity.
#[derive(Clone, Debug)]
pub struct Switches {
    pub nanonexus: bool,
    pub claude_code: bool,
    pub codex: bool,
    pub window_hours: u32,
    pub alerts: bool,
}

impl Switches {
    fn config(&self, demo: bool) -> ObserverConfig {
        ObserverConfig {
            window_hours: self.window_hours,
            nanonexus: self.nanonexus,
            claude_root: self.claude_code.then(default_claude_root).flatten(),
            codex_root: self.codex.then(default_codex_root).flatten(),
            demo,
        }
    }
}

struct Inner {
    observer: Observer,
    switches: Switches,
    demo: bool,
    /// subject → last alert time, and the time of the last alert of any kind.
    last_alert: HashMap<String, i64>,
    last_any: i64,
    last_view: Option<ObserveView>,
}

/// Same subject, same kind: at most once per this long.
const SAME_ALERT_MS: i64 = 10 * 60_000;
/// Any alert: at most one per this long (a burst becomes one notification).
const ANY_ALERT_MS: i64 = 30_000;

pub struct Hub {
    inner: Mutex<Option<Inner>>,
}

impl Default for Hub {
    fn default() -> Self {
        Self {
            inner: Mutex::new(None),
        }
    }
}

fn found(root: Option<std::path::PathBuf>, sub: &str) -> bool {
    root.is_some_and(|r| r.join(sub).is_dir())
}

impl Hub {
    fn with<R>(&self, switches: &Switches, f: impl FnOnce(&mut Inner) -> R) -> Option<R> {
        let mut g = self.inner.lock().ok()?;
        let inner = g.get_or_insert_with(|| Inner {
            observer: Observer::new(switches.config(false)),
            switches: switches.clone(),
            demo: false,
            last_alert: HashMap::new(),
            last_any: 0,
            last_view: None,
        });
        Some(f(inner))
    }

    /// Settings changed (or first use).
    pub fn apply(&self, switches: &Switches) {
        self.with(switches, |i| {
            i.switches = switches.clone();
            i.observer.apply_config(switches.config(i.demo));
        });
    }

    pub fn set_demo(&self, switches: &Switches, on: bool) {
        self.with(switches, |i| {
            i.switches = switches.clone();
            i.demo = on;
            i.observer.apply_config(switches.config(on));
        });
    }

    /// Something happened inside NanoNexus.
    pub fn record(&self, switches: &Switches, obs: impl IntoIterator<Item = Obs>) {
        self.with(switches, |i| i.observer.record_self_all(obs));
    }

    /// Whether anything outside NanoNexus is being read or shown (the poll loop sleeps otherwise).
    pub fn external_active(&self) -> bool {
        self.inner
            .lock()
            .ok()
            .and_then(|g| {
                g.as_ref()
                    .map(|i| i.demo || i.switches.claude_code || i.switches.codex)
            })
            .unwrap_or(false)
    }

    /// Read new events and build the view. Alerts come back only when switched on and not repeated.
    pub fn poll(&self, switches: &Switches, now_ms: i64) -> (ObserveView, Vec<Alert>) {
        self.with(switches, |i| {
            i.switches = switches.clone();
            let (snapshot, attention) = i.observer.poll(now_ms);
            let range = i.observer.replay_range();
            let view = ObserveView {
                snapshot,
                replay_from: range.map(|r| r.0),
                replay_to: range.map(|r| r.1),
                demo: i.demo,
                claude_found: found(default_claude_root(), "projects"),
                codex_found: found(default_codex_root(), "sessions"),
            };
            i.last_view = Some(view.clone());
            let alerts = if switches.alerts && !i.demo {
                alerts_for(attention, now_ms, &mut i.last_alert, &mut i.last_any)
            } else {
                Vec::new()
            };
            (view, alerts)
        })
        .unwrap_or_else(|| {
            let empty = crate::aggregate::Aggregator::new(Default::default()).snapshot(now_ms);
            (
                ObserveView {
                    snapshot: empty,
                    replay_from: None,
                    replay_to: None,
                    demo: false,
                    claude_found: false,
                    codex_found: false,
                },
                Vec::new(),
            )
        })
    }

    /// The last view without reading anything.
    pub fn cached(&self) -> Option<ObserveView> {
        self.inner
            .lock()
            .ok()
            .and_then(|g| g.as_ref().and_then(|i| i.last_view.clone()))
    }

    pub fn frame_at(&self, switches: &Switches, t_ms: i64) -> Option<ActivitySnapshot> {
        self.with(switches, |i| i.observer.frame_at(t_ms)).flatten()
    }
}

fn alerts_for(
    attention: Vec<Attention>,
    now_ms: i64,
    last: &mut HashMap<String, i64>,
    last_any: &mut i64,
) -> Vec<Alert> {
    let mut out = Vec::new();
    for a in attention {
        let key = format!("{:?}|{}|{}", a.kind, a.source.id(), a.subject);
        if last.get(&key).is_some_and(|t| now_ms - t < SAME_ALERT_MS) {
            continue;
        }
        if now_ms - *last_any < ANY_ALERT_MS {
            continue;
        }
        let place = if a.project.is_empty() {
            a.source.label().to_string()
        } else {
            format!("{} · {}", a.source.label(), a.project)
        };
        let (title, body) = match a.kind {
            AttentionKind::Stalled => (
                format!("{} may be stuck", a.subject),
                format!("{place}: no activity for a few minutes while it was working."),
            ),
            AttentionKind::Errored => (
                format!("{} hit an error", a.subject),
                format!("{place}: the agent reported an error."),
            ),
            AttentionKind::Conflict => (
                format!("Two agents are editing {}", a.subject),
                format!("{place}: more than one agent touched this file in the last two minutes."),
            ),
        };
        last.insert(key, now_ms);
        *last_any = now_ms;
        out.push(Alert { title, body });
    }
    last.retain(|_, t| now_ms - *t < SAME_ALERT_MS);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selfsrc;

    const NOW: i64 = 1_800_000_000_000;

    fn sw() -> Switches {
        Switches {
            nanonexus: true,
            claude_code: false,
            codex: false,
            window_hours: 6,
            alerts: true,
        }
    }

    #[test]
    fn view_alerts_and_cache() {
        let hub = Hub::default();
        let s = sw();
        assert!(!hub.external_active());
        hub.record(&s, selfsrc::chat_started("c1", "Nexus", "m", NOW));
        let (v, a) = hub.poll(&s, NOW + 1_000);
        assert_eq!(v.snapshot.totals.agents, 1);
        assert!(a.is_empty());
        let (_, a) = hub.poll(&s, NOW + 200_000);
        assert_eq!(a.len(), 1);
        assert!(a[0].title.contains("may be stuck"), "{a:?}");
        assert!(hub.cached().is_some());
        assert!(hub.frame_at(&s, NOW + 100_000).is_some());
    }

    #[test]
    fn alerts_can_be_switched_off_and_demo_never_alerts() {
        let hub = Hub::default();
        let mut s = sw();
        s.alerts = false;
        hub.record(&s, selfsrc::chat_started("c1", "Nexus", "m", NOW));
        hub.poll(&s, NOW + 1_000);
        assert!(hub.poll(&s, NOW + 200_000).1.is_empty());
        hub.set_demo(&sw(), true);
        assert!(hub.external_active());
        let (v, a) = hub.poll(&sw(), NOW + 300_000);
        assert!(v.demo && v.snapshot.totals.agents == 19);
        assert!(a.is_empty());
    }

    #[test]
    fn repeated_and_bursty_alerts_are_limited() {
        let mut last = HashMap::new();
        let mut any = 0;
        let att = |s: &str| Attention {
            kind: AttentionKind::Stalled,
            source: crate::model::ActivitySource::Codex,
            subject: s.into(),
            project: "p".into(),
        };
        assert_eq!(
            alerts_for(vec![att("a"), att("b")], NOW, &mut last, &mut any).len(),
            1
        );
        assert_eq!(
            alerts_for(vec![att("b")], NOW + 40_000, &mut last, &mut any).len(),
            1
        );
        assert!(alerts_for(vec![att("b")], NOW + 100_000, &mut last, &mut any).is_empty());
        assert_eq!(
            alerts_for(vec![att("b")], NOW + 700_000, &mut last, &mut any).len(),
            1
        );
    }
}
