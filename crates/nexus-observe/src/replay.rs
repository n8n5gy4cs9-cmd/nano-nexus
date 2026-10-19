//! The replay buffer: one compact snapshot every few seconds for the last few hours, in RAM only
//! (OBSERVE.md §6). Frames are already privacy-reduced.

use std::collections::VecDeque;

use crate::aggregate::{Aggregator, Config};
use crate::model::{ActivitySnapshot, Obs};

/// Smallest spacing between frames.
const MIN_STEP_MS: i64 = 10_000;
/// Keep about this many frames per window, so a 48 h window uses a coarser step.
const TARGET_FRAMES: i64 = 2_000;

pub struct Replay {
    frames: VecDeque<ActivitySnapshot>,
    step_ms: i64,
    window_ms: i64,
}

impl Replay {
    pub fn new(window_ms: i64) -> Self {
        Self {
            frames: VecDeque::new(),
            step_ms: (window_ms / TARGET_FRAMES).max(MIN_STEP_MS),
            window_ms,
        }
    }

    pub fn step_ms(&self) -> i64 {
        self.step_ms
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    pub fn len(&self) -> usize {
        self.frames.len()
    }

    /// Store `snap` if a full step has passed since the last frame.
    pub fn push(&mut self, snap: ActivitySnapshot) {
        if self
            .frames
            .back()
            .is_some_and(|l| snap.at_ms - l.at_ms < self.step_ms)
        {
            return;
        }
        let cutoff = snap.at_ms - self.window_ms;
        self.frames.push_back(snap);
        while self.frames.front().is_some_and(|f| f.at_ms < cutoff) {
            self.frames.pop_front();
        }
    }

    /// The first and last frame times.
    pub fn range(&self) -> Option<(i64, i64)> {
        Some((self.frames.front()?.at_ms, self.frames.back()?.at_ms))
    }

    /// The newest frame at or before `t_ms` (the oldest one if `t_ms` is before them all).
    pub fn frame_at(&self, t_ms: i64) -> Option<&ActivitySnapshot> {
        let i = self.frames.partition_point(|f| f.at_ms <= t_ms);
        self.frames.get(i.saturating_sub(1))
    }

    /// Rebuild the past from observations that are already in hand (the tail of the logs read at
    /// start-up), so replay is useful right after launch. `obs` must be sorted by time.
    pub fn backfill(obs: &[Obs], cfg: &Config, now_ms: i64) -> Replay {
        let mut r = Replay::new(cfg.window_ms);
        let Some(first) = obs.first() else { return r };
        let mut agg = Aggregator::new(cfg.clone());
        let step = r.step_ms;
        let mut t = first.at_ms.max(now_ms - cfg.window_ms) / step * step;
        let mut i = 0;
        while t < now_ms {
            while i < obs.len() && obs[i].at_ms <= t {
                agg.ingest(obs[i].clone());
                i += 1;
            }
            r.push(agg.snapshot(t));
            t += step;
        }
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ActivitySource, ObsKind, Role};

    fn snap(at: i64) -> ActivitySnapshot {
        Aggregator::new(Config::default()).snapshot(at)
    }

    #[test]
    fn frames_are_spaced_trimmed_and_found_by_time() {
        let mut r = Replay::new(60_000 * 60);
        for s in 0..400 {
            r.push(snap(s * 1000 * 10 + 1_000_000));
        }
        // 10 s steps, 1 h window = 360 frames.
        assert!(r.len() <= 361 && r.len() >= 359, "{}", r.len());
        let (a, b) = r.range().unwrap();
        assert!(b - a <= 3_600_000);
        assert_eq!(r.frame_at(b + 99_999).unwrap().at_ms, b);
        assert_eq!(r.frame_at(a - 5).unwrap().at_ms, a, "before the first = first");
        let mid = r.frame_at(a + 25_000).unwrap().at_ms;
        assert_eq!(mid, a + 20_000);
        let mut r2 = Replay::new(60_000 * 60);
        r2.push(snap(0));
        r2.push(snap(3_000));
        assert_eq!(r2.len(), 1, "too close to the last frame");
    }

    #[test]
    fn long_windows_use_a_coarser_step() {
        assert_eq!(Replay::new(6 * 3_600_000).step_ms(), 10_800);
        assert_eq!(Replay::new(3_600_000).step_ms(), 10_000);
        assert_eq!(Replay::new(48 * 3_600_000).step_ms(), 86_400);
    }

    #[test]
    fn backfill_rebuilds_history_from_observations() {
        let now = 1_800_000_000_000;
        let mk = |at, kind| Obs {
            source: ActivitySource::Codex,
            session: "s".into(),
            agent: "s".into(),
            parent: None,
            project: "/p".into(),
            at_ms: at,
            kind,
        };
        let obs = vec![
            mk(now - 3_000_000, ObsKind::Message(Role::User)),
            mk(now - 2_000_000, ObsKind::End),
        ];
        let r = Replay::backfill(&obs, &Config::default(), now);
        assert!(!r.is_empty());
        let early = r.frame_at(now - 2_900_000).unwrap();
        assert_eq!(early.totals.active, 1);
        let late = r.frame_at(now - 1_000_000).unwrap();
        assert_eq!(late.totals.active, 0);
        assert_eq!(late.totals.agents, 1);
    }
}
