//! NanoNexus' own activity as observations (OBSERVE.md §1). The app service calls these from the places
//! where a chat turn, a tool call or a job starts and ends. Nothing here takes text, only ids and names.

use crate::model::{ActivitySource, Obs, ObsKind, Role, TokenTotals};

/// The project label under which all NanoNexus agents appear.
pub const PROJECT: &str = "NanoNexus";

fn base(agent: &str, parent: Option<&str>, at_ms: i64, kind: ObsKind) -> Obs {
    Obs {
        source: ActivitySource::NanoNexus,
        session: parent.unwrap_or(agent).to_string(),
        agent: agent.to_string(),
        parent: parent.map(str::to_string),
        project: PROJECT.to_string(),
        at_ms,
        kind,
    }
}

/// A chat turn started with `model` (the agent is the conversation, named after its assistant).
pub fn chat_started(conversation: &str, assistant: &str, model: &str, at_ms: i64) -> Vec<Obs> {
    let a = format!("chat-{conversation}");
    vec![
        base(&a, None, at_ms, ObsKind::Title(format!("Chat · {assistant}"))),
        base(&a, None, at_ms, ObsKind::Model(model.to_string())),
        base(&a, None, at_ms, ObsKind::Message(Role::User)),
    ]
}

/// A chunk of generated tokens (prompt tokens once, then output tokens).
pub fn chat_tokens(conversation: &str, tokens: TokenTotals, at_ms: i64) -> Obs {
    base(
        &format!("chat-{conversation}"),
        None,
        at_ms,
        ObsKind::Tokens(tokens),
    )
}

/// The chat turn finished (`ok = false`: it failed).
pub fn chat_finished(conversation: &str, ok: bool, at_ms: i64) -> Obs {
    let kind = if ok { ObsKind::End } else { ObsKind::Error };
    base(&format!("chat-{conversation}"), None, at_ms, kind)
}

/// An assistant tool call began (`files` only for tools that propose file changes).
pub fn tool_started(conversation: &str, call_id: &str, tool: &str, files: Vec<String>, at_ms: i64) -> Obs {
    base(
        &format!("chat-{conversation}"),
        None,
        at_ms,
        ObsKind::ToolStart {
            id: call_id.to_string(),
            tool: tool.to_string(),
            files,
        },
    )
}

pub fn tool_finished(conversation: &str, call_id: &str, ok: bool, at_ms: i64) -> Obs {
    base(
        &format!("chat-{conversation}"),
        None,
        at_ms,
        ObsKind::ToolEnd {
            id: call_id.to_string(),
            ok,
        },
    )
}

/// A background job (scan, download, index, rename batch…) is running: heartbeat while it makes progress.
pub fn job_progress(job: &str, kind: &str, at_ms: i64) -> Vec<Obs> {
    let a = format!("job-{job}");
    vec![
        base(&a, None, at_ms, ObsKind::Start { model: None }),
        base(&a, None, at_ms, ObsKind::Title(format!("Job · {kind}"))),
        base(
            &a,
            None,
            at_ms,
            ObsKind::ToolStart {
                id: job.to_string(),
                tool: kind.to_string(),
                files: Vec::new(),
            },
        ),
    ]
}

pub fn job_finished(job: &str, ok: bool, at_ms: i64) -> Vec<Obs> {
    let a = format!("job-{job}");
    let mut v = vec![base(
        &a,
        None,
        at_ms,
        ObsKind::ToolEnd {
            id: job.to_string(),
            ok,
        },
    )];
    v.push(base(
        &a,
        None,
        at_ms,
        if ok { ObsKind::End } else { ObsKind::Error },
    ));
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aggregate::{Aggregator, Config};
    use crate::model::AgentStatus;

    #[test]
    fn a_chat_turn_runs_then_goes_idle() {
        let t0 = 1_800_000_000_000;
        let mut g = Aggregator::new(Config::default());
        for o in chat_started("c1", "Nexus", "Qwen3-4B", t0) {
            g.ingest(o);
        }
        g.ingest(tool_started("c1", "call1", "read_file", vec![], t0 + 100));
        g.ingest(tool_finished("c1", "call1", true, t0 + 200));
        g.ingest(chat_tokens(
            "c1",
            TokenTotals {
                input: 300,
                output: 40,
                ..Default::default()
            },
            t0 + 300,
        ));
        let s = g.snapshot(t0 + 1_000);
        assert_eq!(s.agents[0].status, AgentStatus::Running);
        assert_eq!(s.agents[0].source, ActivitySource::NanoNexus);
        assert_eq!(s.agents[0].label, "Chat · Nexus");
        assert_eq!(s.totals.new_tokens, 340);
        g.ingest(chat_finished("c1", true, t0 + 2_000));
        assert_eq!(g.snapshot(t0 + 3_000).agents[0].status, AgentStatus::Idle);
    }

    #[test]
    fn jobs_end_idle_or_errored() {
        let t0 = 1_800_000_000_000;
        let mut g = Aggregator::new(Config::default());
        for o in job_progress("j1", "scan", t0) {
            g.ingest(o);
        }
        assert_eq!(g.snapshot(t0 + 10).agents[0].tool.as_deref(), Some("scan"));
        for o in job_finished("j1", false, t0 + 50) {
            g.ingest(o);
        }
        assert_eq!(g.snapshot(t0 + 60).agents[0].status, AgentStatus::Errored);
    }
}
