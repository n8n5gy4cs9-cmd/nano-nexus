//! Claude Code transcripts: `~/.claude/projects/<slug>/<session>.jsonl` and `…/<session>/subagents/*.jsonl`
//! (OBSERVE.md §3). Keeps kinds, model, usage, tool names and the path of edit tools; drops all text.

use std::collections::{HashMap, HashSet};

use serde_json::Value;

use crate::model::{LineParser, Obs, ObsKind, Parsed, Role, Source, TokenCount};
use crate::timefmt::parse_rfc3339_ms;

/// Tools that change a file, and the input key that holds its path.
const EDIT_TOOLS: [(&str, &str); 5] = [
    ("Edit", "file_path"),
    ("Write", "file_path"),
    ("MultiEdit", "file_path"),
    ("NotebookEdit", "notebook_path"),
    ("str_replace_based_edit_tool", "path"),
];

const MAX_REMEMBERED: usize = 4000;

/// Per-file state: usage already counted per API message (a streamed message spans several lines).
#[derive(Default)]
pub struct ClaudeParser {
    usage: HashMap<String, TokenCount>,
    tools: HashSet<String>,
}

impl ClaudeParser {
    pub fn new() -> Self {
        Self::default()
    }
}

fn u(v: &Value, k: &str) -> u64 {
    v.get(k).and_then(Value::as_u64).unwrap_or(0)
}

impl LineParser for ClaudeParser {
    fn parse_line(&mut self, line: &str, now_ms: i64) -> Parsed {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            return Parsed {
                obs: vec![],
                bad: true,
            };
        };
        let Some(ty) = v.get("type").and_then(Value::as_str) else {
            return Parsed {
                obs: vec![],
                bad: true,
            };
        };
        // Lines without a session (e.g. housekeeping records) are fine, just not ours.
        let Some(session) = v.get("sessionId").and_then(Value::as_str) else {
            return Parsed::default();
        };
        if self.usage.len() > MAX_REMEMBERED {
            self.usage.clear();
        }
        if self.tools.len() > MAX_REMEMBERED {
            self.tools.clear();
        }
        let sidechain = v.get("isSidechain").and_then(Value::as_bool).unwrap_or(false);
        let (agent, parent) = match v.get("agentId").and_then(Value::as_str) {
            Some(a) if sidechain || a != session => (format!("{session}/{a}"), Some(session.to_string())),
            _ => (session.to_string(), None),
        };
        let at_ms = v
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(parse_rfc3339_ms)
            .unwrap_or(now_ms);
        let project = v.get("cwd").and_then(Value::as_str).unwrap_or("").to_string();
        let mk = |kind: ObsKind| Obs {
            source: Source::ClaudeCode,
            session: session.to_string(),
            agent: agent.clone(),
            parent: parent.clone(),
            project: project.clone(),
            at_ms,
            kind,
        };
        let mut obs = Vec::new();
        match ty {
            "assistant" => self.assistant(&v, &mk, &mut obs),
            "user" => user(&v, &mk, &mut obs),
            "system" => {
                let level = v.get("level").and_then(Value::as_str);
                let sub = v.get("subtype").and_then(Value::as_str);
                if level == Some("error") || sub == Some("api_error") {
                    obs.push(mk(ObsKind::Error));
                }
            }
            _ => {}
        }
        Parsed { obs, bad: false }
    }
}

impl ClaudeParser {
    fn assistant(&mut self, v: &Value, mk: &dyn Fn(ObsKind) -> Obs, out: &mut Vec<Obs>) {
        if v.get("isApiErrorMessage").and_then(Value::as_bool) == Some(true) {
            out.push(mk(ObsKind::Error));
            return;
        }
        let Some(m) = v.get("message") else { return };
        if let Some(model) = m.get("model").and_then(Value::as_str)
            && !model.is_empty()
            && !model.starts_with('<')
        {
            out.push(mk(ObsKind::Model(model.to_string())));
        }
        let msg_id = m.get("id").and_then(Value::as_str).unwrap_or("");
        let first_sight = msg_id.is_empty() || !self.usage.contains_key(msg_id);
        if first_sight {
            out.push(mk(ObsKind::Message(Role::Assistant)));
            self.usage.entry(msg_id.to_string()).or_default();
        }
        if let Some(usage) = m.get("usage") {
            let now = TokenCount {
                input: u(usage, "input_tokens"),
                output: u(usage, "output_tokens"),
                cache_read: u(usage, "cache_read_input_tokens"),
                cache_write: u(usage, "cache_creation_input_tokens"),
            };
            // A streamed message repeats its usage on every line: count each field once, by its maximum.
            let seen = self.usage.entry(msg_id.to_string()).or_default();
            let delta = TokenCount {
                input: now.input.saturating_sub(seen.input),
                output: now.output.saturating_sub(seen.output),
                cache_read: now.cache_read.saturating_sub(seen.cache_read),
                cache_write: now.cache_write.saturating_sub(seen.cache_write),
            };
            seen.input = seen.input.max(now.input);
            seen.output = seen.output.max(now.output);
            seen.cache_read = seen.cache_read.max(now.cache_read);
            seen.cache_write = seen.cache_write.max(now.cache_write);
            if delta != TokenCount::default() {
                out.push(mk(ObsKind::Tokens(delta)));
            }
        }
        if let Some(blocks) = m.get("content").and_then(Value::as_array) {
            for b in blocks {
                if b.get("type").and_then(Value::as_str) != Some("tool_use") {
                    continue;
                }
                let id = b.get("id").and_then(Value::as_str).unwrap_or("");
                if !id.is_empty() && !self.tools.insert(id.to_string()) {
                    continue;
                }
                let tool = b.get("name").and_then(Value::as_str).unwrap_or("tool");
                let files = EDIT_TOOLS
                    .iter()
                    .find(|(n, _)| *n == tool)
                    .and_then(|(_, key)| b.get("input")?.get(*key)?.as_str())
                    .map(|p| vec![p.to_string()])
                    .unwrap_or_default();
                out.push(mk(ObsKind::ToolStart {
                    id: id.to_string(),
                    tool: tool.to_string(),
                    files,
                }));
            }
        }
        if matches!(
            m.get("stop_reason").and_then(Value::as_str),
            Some("end_turn" | "stop_sequence" | "max_tokens" | "refusal")
        ) {
            out.push(mk(ObsKind::End));
        }
    }
}

fn user(v: &Value, mk: &dyn Fn(ObsKind) -> Obs, out: &mut Vec<Obs>) {
    let meta = v.get("isMeta").and_then(Value::as_bool) == Some(true);
    let Some(content) = v.get("message").and_then(|m| m.get("content")) else {
        return;
    };
    let mut human = false;
    let mut interrupted = false;
    let mut note_text = |t: &str, human: &mut bool| {
        if t.starts_with("[Request interrupted") {
            interrupted = true;
        } else {
            *human = true;
        }
    };
    match content {
        Value::String(t) => note_text(t, &mut human),
        Value::Array(blocks) => {
            for b in blocks {
                match b.get("type").and_then(Value::as_str) {
                    Some("tool_result") => {
                        let id = b.get("tool_use_id").and_then(Value::as_str).unwrap_or("");
                        let is_err = b.get("is_error").and_then(Value::as_bool) == Some(true);
                        out.push(mk(ObsKind::ToolEnd {
                            id: id.to_string(),
                            ok: !is_err,
                        }));
                    }
                    Some("text") => {
                        note_text(b.get("text").and_then(Value::as_str).unwrap_or(""), &mut human);
                    }
                    Some("image" | "document") => human = true,
                    _ => {}
                }
            }
        }
        _ => {}
    }
    if interrupted {
        out.push(mk(ObsKind::End));
    } else if human && !meta {
        out.push(mk(ObsKind::Message(Role::User)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: &str = r#""sessionId":"s1","cwd":"/home/me/app","timestamp":"2026-10-19T19:28:50.000Z""#;

    fn parse(p: &mut ClaudeParser, body: &str) -> Vec<ObsKind> {
        let line = format!("{{{S},{body}}}");
        let r = p.parse_line(&line, 0);
        assert!(!r.bad);
        r.obs.into_iter().map(|o| o.kind).collect()
    }

    #[test]
    fn streamed_usage_is_counted_once_per_message() {
        let mut p = ClaudeParser::new();
        let body = r#""type":"assistant","message":{"id":"m1","model":"claude-opus-5-5","content":[{"type":"text","text":"hi"}],"stop_reason":null,"usage":{"input_tokens":2,"output_tokens":10,"cache_read_input_tokens":100,"cache_creation_input_tokens":5}}"#;
        let first = parse(&mut p, body);
        assert!(first.contains(&ObsKind::Tokens(TokenCount {
            input: 2,
            output: 10,
            cache_read: 100,
            cache_write: 5
        })));
        assert!(first.contains(&ObsKind::Message(Role::Assistant)));
        let again = parse(&mut p, body);
        assert!(
            !again.iter().any(|k| matches!(k, ObsKind::Tokens(_))),
            "same usage again adds nothing: {again:?}"
        );
        assert!(!again.contains(&ObsKind::Message(Role::Assistant)));
        // The final line has a larger output count: only the difference is added.
        let last = parse(
            &mut p,
            r#""type":"assistant","message":{"id":"m1","model":"claude-opus-5-5","content":[],"stop_reason":"end_turn","usage":{"input_tokens":2,"output_tokens":25,"cache_read_input_tokens":100,"cache_creation_input_tokens":5}}"#,
        );
        assert!(last.contains(&ObsKind::Tokens(TokenCount {
            output: 15,
            ..Default::default()
        })));
        assert!(last.contains(&ObsKind::End));
    }

    #[test]
    fn edit_tools_carry_their_path_and_others_do_not() {
        let mut p = ClaudeParser::new();
        let kinds = parse(
            &mut p,
            r#""type":"assistant","message":{"id":"m2","content":[{"type":"tool_use","id":"t1","name":"Edit","input":{"file_path":"/home/me/app/src/a.rs","old_string":"SECRET","new_string":"X"}},{"type":"tool_use","id":"t2","name":"Bash","input":{"command":"rm -rf SECRET"}}],"stop_reason":"tool_use"}"#,
        );
        assert!(kinds.contains(&ObsKind::ToolStart {
            id: "t1".into(),
            tool: "Edit".into(),
            files: vec!["/home/me/app/src/a.rs".into()]
        }));
        assert!(kinds.contains(&ObsKind::ToolStart {
            id: "t2".into(),
            tool: "Bash".into(),
            files: vec![]
        }));
        assert!(!format!("{kinds:?}").contains("SECRET"));
    }

    #[test]
    fn tool_results_close_tools_and_flag_errors() {
        let mut p = ClaudeParser::new();
        let kinds = parse(
            &mut p,
            r#""type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","is_error":true,"content":"boom"}]}"#,
        );
        assert_eq!(
            kinds,
            vec![ObsKind::ToolEnd {
                id: "t1".into(),
                ok: false
            }]
        );
    }

    #[test]
    fn human_messages_and_interrupts() {
        let mut p = ClaudeParser::new();
        assert_eq!(
            parse(&mut p, r#""type":"user","message":{"content":"do the thing"}"#),
            vec![ObsKind::Message(Role::User)]
        );
        assert_eq!(
            parse(
                &mut p,
                r#""type":"user","message":{"content":[{"type":"text","text":"[Request interrupted by user]"}]}"#
            ),
            vec![ObsKind::End]
        );
        assert!(parse(&mut p, r#""type":"user","isMeta":true,"message":{"content":"x"}"#).is_empty());
    }

    #[test]
    fn subagent_lines_get_a_parent() {
        let mut p = ClaudeParser::new();
        let line = format!(
            "{{{S},\"type\":\"assistant\",\"isSidechain\":true,\"agentId\":\"a9\",\"message\":{{\"id\":\"m3\",\"content\":[]}}}}"
        );
        let r = p.parse_line(&line, 0);
        let o = &r.obs[0];
        assert_eq!(o.agent, "s1/a9");
        assert_eq!(o.parent.as_deref(), Some("s1"));
        assert_eq!(o.session, "s1");
        assert_eq!(o.at_ms, 1_792_438_130_000);
    }

    #[test]
    fn errors_and_garbage() {
        let mut p = ClaudeParser::new();
        assert_eq!(
            parse(
                &mut p,
                r#""type":"assistant","isApiErrorMessage":true,"message":{"content":[]}"#
            ),
            vec![ObsKind::Error]
        );
        assert_eq!(
            parse(&mut p, r#""type":"system","level":"error""#),
            vec![ObsKind::Error]
        );
        assert!(p.parse_line("not json", 0).bad);
        assert!(p.parse_line(r#"{"no_type":1}"#, 0).bad);
        // Housekeeping lines are neither errors nor drift.
        let r = p.parse_line(r#"{"type":"last-prompt","lastPrompt":"secret"}"#, 0);
        assert!(!r.bad && r.obs.is_empty());
    }
}
