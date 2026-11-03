//! Engine identity and resume rules for coding sessions (UP.083).
//!
//! NanoNexus conversation ids and an external runtime's own session ids are different things and are
//! stored in different fields. An external id is only ever offered back to the engine and profile that
//! issued it. Resuming never replays a write: applied change sets are remembered per turn.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

/// Who runs the coding loop.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Engine {
    /// The built-in router and tools with the Journal.
    NanoNexus,
    /// An optional private runtime, named for display (for example `claude-code`).
    External { name: String },
}

/// A stored session and the exact engine/profile it belongs to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct SessionRef {
    /// NanoNexus conversation id. Always present.
    pub conversation_id: String,
    pub engine: Engine,
    /// Provider profile id used for this session (`local` or a cloud profile).
    pub provider_id: String,
    /// Model name at the time, for display.
    pub model: String,
    /// The external runtime's own session id. Never a NanoNexus id; `None` for the built-in engine.
    pub external_session_id: Option<String>,
    /// Hash of the settings that make an external session incompatible when changed.
    pub profile_fingerprint: String,
}

/// What resuming may do now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resume {
    /// Continue with the stored history (built-in engine) or the external id.
    Continue,
    /// Keep the visible history, but the external session cannot be resumed; start a new one.
    StartNewKeepHistory(String),
}

/// Decide how to resume `stored` with the engine and profile now selected.
pub fn resume(stored: &SessionRef, engine: &Engine, provider_id: &str, fingerprint: &str) -> Resume {
    if &stored.engine != engine {
        return Resume::StartNewKeepHistory(format!(
            "This conversation ran on {}; {} cannot continue its session.",
            engine_label(&stored.engine),
            engine_label(engine)
        ));
    }
    if let Engine::External { .. } = engine {
        if stored.external_session_id.is_none() {
            return Resume::StartNewKeepHistory(
                "The runtime did not give this conversation a session id.".into(),
            );
        }
        if stored.provider_id != provider_id {
            return Resume::StartNewKeepHistory(
                "The provider profile changed since this session started.".into(),
            );
        }
        if stored.profile_fingerprint != fingerprint {
            return Resume::StartNewKeepHistory(
                "The model or runtime settings changed since this session started.".into(),
            );
        }
    }
    Resume::Continue
}

fn engine_label(e: &Engine) -> String {
    match e {
        Engine::NanoNexus => "NanoNexus".into(),
        Engine::External { name } => name.clone(),
    }
}

/// Short text for the UI so the user always sees what is running.
pub fn identity_label(s: &SessionRef) -> String {
    let model = if s.model.is_empty() {
        String::new()
    } else {
        format!(" · {}", s.model)
    };
    format!("{} · {}{}", engine_label(&s.engine), s.provider_id, model)
}

/// Remembers which change sets were already applied for a turn, so a resumed or retried turn cannot
/// apply the same write twice.
#[derive(Debug, Default)]
pub struct AppliedLedger(HashSet<(String, String)>);

impl AppliedLedger {
    /// True the first time `(turn, change_set)` is seen; false for every repeat.
    pub fn first_time(&mut self, turn_id: &str, change_set_id: &str) -> bool {
        self.0.insert((turn_id.to_owned(), change_set_id.to_owned()))
    }

    /// Rebuild from persisted records after a restart.
    pub fn from_records<'a>(records: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        Self(
            records
                .into_iter()
                .map(|(t, c)| (t.to_owned(), c.to_owned()))
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn external(fp: &str, provider: &str) -> SessionRef {
        SessionRef {
            conversation_id: "c1".into(),
            engine: Engine::External {
                name: "claude-code".into(),
            },
            provider_id: provider.into(),
            model: "m".into(),
            external_session_id: Some("ext-9".into()),
            profile_fingerprint: fp.into(),
        }
    }

    #[test]
    fn external_ids_resume_only_for_the_same_engine_profile_and_settings() {
        let s = external("fp1", "acme");
        let eng = Engine::External {
            name: "claude-code".into(),
        };
        assert_eq!(resume(&s, &eng, "acme", "fp1"), Resume::Continue);
        assert!(matches!(
            resume(&s, &eng, "other", "fp1"),
            Resume::StartNewKeepHistory(_)
        ));
        assert!(matches!(
            resume(&s, &eng, "acme", "fp2"),
            Resume::StartNewKeepHistory(_)
        ));
        assert!(matches!(
            resume(&s, &Engine::NanoNexus, "local", "x"),
            Resume::StartNewKeepHistory(_)
        ));
    }

    #[test]
    fn builtin_engine_always_continues_from_stored_history() {
        let s = SessionRef {
            conversation_id: "c1".into(),
            engine: Engine::NanoNexus,
            provider_id: "local".into(),
            model: String::new(),
            external_session_id: None,
            profile_fingerprint: String::new(),
        };
        assert_eq!(resume(&s, &Engine::NanoNexus, "local", ""), Resume::Continue);
        assert_eq!(identity_label(&s), "NanoNexus · local");
    }

    #[test]
    fn a_resumed_turn_cannot_apply_the_same_change_twice() {
        let mut l = AppliedLedger::default();
        assert!(l.first_time("t1", "cs1"));
        assert!(!l.first_time("t1", "cs1"));
        let mut r = AppliedLedger::from_records([("t1", "cs1")]);
        assert!(!r.first_time("t1", "cs1"));
        assert!(r.first_time("t2", "cs1"));
    }
}
