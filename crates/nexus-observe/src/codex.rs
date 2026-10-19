//! Codex session files: `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` (OBSERVE.md §3).
//! Keeps kinds, model, token deltas, tool names and the files named in `apply_patch` headers.

use serde_json::Value;

use crate::model::{ActivitySource, LineParser, Obs, ObsKind, Parsed, Role, TokenTotals};
use crate::timefmt::parse_rfc3339_ms;

/// Tool names that edit files by patch.
const PATCH_TOOLS: [&str; 2] = ["apply_patch", "applypatch"];

/// Per-file state: session identity and the cumulative token totals seen so far.
pub struct CodexParser {
    session: String,
    parent: Option<String>,
    project: String,
    last_total: TokenTotals,
}

impl CodexParser {
    /// `fallback_session` is used until a `session_meta` line names the real id (it may be out of the
    /// tail window): the uuid in the file name.
    pub fn new(fallback_session: &str) -> Self {
        Self {
            session: fallback_session.to_string(),
            parent: None,
            project: String::new(),
            last_total: TokenTotals::default(),
        }
    }
}

/// `*** Update File: path` style headers of a patch: the paths only, never the patch body.
pub fn patch_files(patch: &str) -> Vec<String> {
    const HEADS: [&str; 4] = [
        "*** Update File: ",
        "*** Add File: ",
        "*** Delete File: ",
        "*** Move to: ",
    ];
    patch
        .lines()
        .filter_map(|l| HEADS.iter().find_map(|h| l.strip_prefix(h)))
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect()
}

fn n(v: &Value, k: &str) -> u64 {
    v.get(k).and_then(Value::as_u64).unwrap_or(0)
}

impl LineParser for CodexParser {
    fn parse_line(&mut self, line: &str, now_ms: i64) -> Parsed {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            return Parsed {
                obs: vec![],
                bad: true,
            };
        };
        let (Some(ty), Some(p)) = (v.get("type").and_then(Value::as_str), v.get("payload")) else {
            return Parsed {
                obs: vec![],
                bad: true,
            };
        };
        let at_ms = v
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(parse_rfc3339_ms)
            .unwrap_or(now_ms);
        let pty = p.get("type").and_then(Value::as_str).unwrap_or("");
        let mut kinds: Vec<ObsKind> = Vec::new();
        match ty {
            "session_meta" => {
                if let Some(id) = p.get("id").and_then(Value::as_str) {
                    self.session = id.to_string();
                }
                if let Some(cwd) = p.get("cwd").and_then(Value::as_str) {
                    self.project = cwd.to_string();
                }
                self.parent = p
                    .pointer("/source/subagent/thread_spawn/parent_thread_id")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                kinds.push(ObsKind::Start { model: None });
            }
            "turn_context" => {
                if let Some(cwd) = p.get("cwd").and_then(Value::as_str) {
                    self.project = cwd.to_string();
                }
                if let Some(m) = p.get("model").and_then(Value::as_str) {
                    kinds.push(ObsKind::Model(m.to_string()));
                }
            }
            "response_item" => match pty {
                "function_call" | "custom_tool_call" | "local_shell_call" => {
                    let tool =
                        p.get("name")
                            .and_then(Value::as_str)
                            .unwrap_or(if pty == "local_shell_call" {
                                "shell"
                            } else {
                                "tool"
                            });
                    let id = p
                        .get("call_id")
                        .or_else(|| p.get("id"))
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    let files = if PATCH_TOOLS.contains(&tool) {
                        // `input` (custom tool) or the JSON string in `arguments`: read only for the headers.
                        let text = p
                            .get("input")
                            .and_then(Value::as_str)
                            .or_else(|| p.get("arguments").and_then(Value::as_str))
                            .unwrap_or("");
                        let text = serde_json::from_str::<Value>(text)
                            .ok()
                            .and_then(|a| {
                                a.get("input")
                                    .or_else(|| a.get("patch"))
                                    .and_then(Value::as_str)
                                    .map(str::to_string)
                            })
                            .unwrap_or_else(|| text.to_string());
                        patch_files(&text)
                    } else {
                        Vec::new()
                    };
                    kinds.push(ObsKind::ToolStart {
                        id: id.to_string(),
                        tool: tool.to_string(),
                        files,
                    });
                }
                "function_call_output" | "custom_tool_call_output" => {
                    let id = p.get("call_id").and_then(Value::as_str).unwrap_or("");
                    let out = p.get("output").and_then(Value::as_str).unwrap_or("");
                    // Shell outputs are JSON with `metadata.exit_code`; read only that number.
                    let ok = serde_json::from_str::<Value>(out)
                        .ok()
                        .and_then(|o| o.pointer("/metadata/exit_code").and_then(Value::as_i64))
                        .is_none_or(|c| c == 0);
                    kinds.push(ObsKind::ToolEnd {
                        id: id.to_string(),
                        ok,
                    });
                }
                "message" => match p.get("role").and_then(Value::as_str) {
                    Some("assistant") => kinds.push(ObsKind::Message(Role::Assistant)),
                    Some("user") => {}
                    _ => {}
                },
                _ => {}
            },
            "event_msg" => match pty {
                "token_count" => {
                    if let Some(t) = p.pointer("/info/total_token_usage") {
                        let cached = n(t, "cached_input_tokens");
                        let total = TokenTotals {
                            input: n(t, "input_tokens").saturating_sub(cached),
                            output: n(t, "output_tokens"),
                            cache_read: cached,
                            cache_write: 0,
                        };
                        let prev = self.last_total;
                        // A smaller total means the counter restarted: take it as the delta.
                        let restarted = total.output < prev.output;
                        let base = if restarted { TokenTotals::default() } else { prev };
                        let delta = TokenTotals {
                            input: total.input.saturating_sub(base.input),
                            output: total.output.saturating_sub(base.output),
                            cache_read: total.cache_read.saturating_sub(base.cache_read),
                            cache_write: 0,
                        };
                        self.last_total = total;
                        if delta != TokenTotals::default() {
                            kinds.push(ObsKind::Tokens(delta));
                        }
                    }
                }
                "user_message" | "task_started" | "turn_started" => {
                    kinds.push(ObsKind::Message(Role::User));
                }
                "agent_message" => kinds.push(ObsKind::Message(Role::Assistant)),
                "error" | "stream_error" => kinds.push(ObsKind::Error),
                "task_complete" | "turn_complete" | "turn_aborted" | "shutdown_complete" => {
                    kinds.push(ObsKind::End);
                }
                _ => {}
            },
            _ => {}
        }
        let obs = kinds
            .into_iter()
            .map(|kind| Obs {
                source: ActivitySource::Codex,
                session: self.session.clone(),
                agent: self.session.clone(),
                parent: self.parent.clone(),
                project: self.project.clone(),
                at_ms,
                kind,
            })
            .collect();
        Parsed { obs, bad: false }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(p: &mut CodexParser, line: &str) -> Vec<ObsKind> {
        let r = p.parse_line(line, 0);
        assert!(!r.bad, "{line}");
        r.obs.into_iter().map(|o| o.kind).collect()
    }

    #[test]
    fn session_meta_names_the_session_and_project() {
        let mut p = CodexParser::new("file-uuid");
        let r = p.parse_line(
            r#"{"timestamp":"2026-10-19T19:28:50.000Z","type":"session_meta","payload":{"id":"abc","cwd":"/home/me/app","instructions":"SECRET"}}"#,
            0,
        );
        let o = &r.obs[0];
        assert_eq!((o.session.as_str(), o.project.as_str()), ("abc", "/home/me/app"));
        assert_eq!(o.at_ms, 1_792_438_130_000);
        assert!(!format!("{:?}", r.obs).contains("SECRET"));
    }

    #[test]
    fn token_counts_are_cumulative_deltas_with_cache_split() {
        let mut p = CodexParser::new("f");
        let a = kinds(
            &mut p,
            r#"{"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":1000,"cached_input_tokens":600,"output_tokens":50}}}}"#,
        );
        assert_eq!(
            a,
            vec![ObsKind::Tokens(TokenTotals {
                input: 400,
                output: 50,
                cache_read: 600,
                cache_write: 0
            })]
        );
        let b = kinds(
            &mut p,
            r#"{"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":1500,"cached_input_tokens":900,"output_tokens":80}}}}"#,
        );
        assert_eq!(
            b,
            vec![ObsKind::Tokens(TokenTotals {
                input: 200,
                output: 30,
                cache_read: 300,
                cache_write: 0
            })]
        );
        // `info: null` (rate-limit only update) is ignored.
        assert!(
            kinds(
                &mut p,
                r#"{"type":"event_msg","payload":{"type":"token_count","info":null}}"#
            )
            .is_empty()
        );
    }

    #[test]
    fn apply_patch_exposes_only_file_names() {
        let mut p = CodexParser::new("f");
        let k = kinds(
            &mut p,
            r#"{"type":"response_item","payload":{"type":"custom_tool_call","name":"apply_patch","call_id":"c1","input":"*** Begin Patch\n*** Update File: src/a.rs\n@@\n-SECRET\n+x\n*** Add File: b.txt\n+y\n*** End Patch"}}"#,
        );
        assert_eq!(
            k,
            vec![ObsKind::ToolStart {
                id: "c1".into(),
                tool: "apply_patch".into(),
                files: vec!["src/a.rs".into(), "b.txt".into()]
            }]
        );
    }

    #[test]
    fn shell_calls_carry_no_command_and_exit_codes_decide_ok() {
        let mut p = CodexParser::new("f");
        let k = kinds(
            &mut p,
            r#"{"type":"response_item","payload":{"type":"function_call","name":"shell","call_id":"c2","arguments":"{\"command\":[\"rm\",\"-rf\",\"SECRET\"]}"}}"#,
        );
        assert!(!format!("{k:?}").contains("SECRET"));
        let bad = kinds(
            &mut p,
            r#"{"type":"response_item","payload":{"type":"function_call_output","call_id":"c2","output":"{\"output\":\"x\",\"metadata\":{\"exit_code\":2}}"}}"#,
        );
        assert_eq!(
            bad,
            vec![ObsKind::ToolEnd {
                id: "c2".into(),
                ok: false
            }]
        );
        let good = kinds(
            &mut p,
            r#"{"type":"response_item","payload":{"type":"function_call_output","call_id":"c3","output":"plain text"}}"#,
        );
        assert_eq!(
            good,
            vec![ObsKind::ToolEnd {
                id: "c3".into(),
                ok: true
            }]
        );
    }

    #[test]
    fn turn_lifecycle_and_errors() {
        let mut p = CodexParser::new("f");
        assert_eq!(
            kinds(
                &mut p,
                r#"{"type":"turn_context","payload":{"model":"gpt-5-codex","cwd":"/x"}}"#
            ),
            vec![ObsKind::Model("gpt-5-codex".into())]
        );
        assert_eq!(
            kinds(
                &mut p,
                r#"{"type":"event_msg","payload":{"type":"user_message","message":"hi"}}"#
            ),
            vec![ObsKind::Message(Role::User)]
        );
        assert_eq!(
            kinds(
                &mut p,
                r#"{"type":"event_msg","payload":{"type":"error","message":"x"}}"#
            ),
            vec![ObsKind::Error]
        );
        assert_eq!(
            kinds(
                &mut p,
                r#"{"type":"event_msg","payload":{"type":"task_complete"}}"#
            ),
            vec![ObsKind::End]
        );
        assert!(p.parse_line("{broken", 0).bad);
        assert!(p.parse_line(r#"{"type":"x"}"#, 0).bad, "no payload");
    }

    #[test]
    fn subagent_sessions_point_to_their_parent() {
        let mut p = CodexParser::new("f");
        let r = p.parse_line(
            r#"{"type":"session_meta","payload":{"id":"child","cwd":"/x","source":{"subagent":{"thread_spawn":{"parent_thread_id":"root"}}}}}"#,
            0,
        );
        assert_eq!(r.obs[0].parent.as_deref(), Some("root"));
    }
}
