//! A read-only MCP server over stdio (T19.17): lets an agent ask "what else is running?" or "is anyone
//! else editing files here?". Newline-delimited JSON-RPC 2.0, protocol revision 2024-11-05. The tools
//! return the same privacy-reduced data as the Activity page: names, counts and times, never text.

use serde_json::{Value, json};

use crate::model::{ActivitySnapshot, AgentStatus};
use crate::observer::Observer;

const PROTOCOL: &str = "2024-11-05";

fn tools() -> Value {
    let ro = json!({"readOnlyHint": true, "destructiveHint": false, "openWorldHint": false});
    json!([
        {
            "name": "summary",
            "description": "Totals of everything running on this computer: agents, running now, tokens, conflicts.",
            "inputSchema": {"type": "object", "properties": {}},
            "annotations": ro
        },
        {
            "name": "list_agents",
            "description": "Agents (Claude Code, Codex, NanoNexus) seen recently, with model, status, current tool and token counts.",
            "inputSchema": {"type": "object", "properties": {
                "status": {"type": "string", "enum": ["running", "idle", "stalled", "errored"]},
                "source": {"type": "string", "enum": ["nano_nexus", "claude_code", "codex"]}
            }},
            "annotations": ro
        },
        {
            "name": "get_agent",
            "description": "One agent by id (as returned by list_agents).",
            "inputSchema": {"type": "object", "properties": {"id": {"type": "string"}}, "required": ["id"]},
            "annotations": ro
        },
        {
            "name": "list_conflicts",
            "description": "Files that more than one agent edited in the last two minutes.",
            "inputSchema": {"type": "object", "properties": {}},
            "annotations": ro
        }
    ])
}

fn status_name(s: AgentStatus) -> &'static str {
    match s {
        AgentStatus::Running => "running",
        AgentStatus::Idle => "idle",
        AgentStatus::Stalled => "stalled",
        AgentStatus::Errored => "errored",
    }
}

fn agent_json(a: &crate::model::AgentView, now: i64) -> Value {
    json!({
        "id": a.id, "label": a.label, "source": a.source.id(), "project": a.project_id,
        "model": a.model, "status": status_name(a.status), "tool": a.tool,
        "tokens": {"input": a.tokens.input, "output": a.tokens.output,
                   "cache_read": a.tokens.cache_read, "cache_write": a.tokens.cache_write},
        "run_seconds": (now - a.started_ms).max(0) / 1000,
        "idle_seconds": (now - a.last_ms).max(0) / 1000,
        "errors": a.errors,
    })
}

fn call_tool(name: &str, args: &Value, snap: &ActivitySnapshot) -> Result<Value, String> {
    let now = snap.at_ms;
    match name {
        "summary" => Ok(json!({
            "agents": snap.totals.agents, "running": snap.totals.active,
            "projects": snap.projects.len(), "new_tokens": snap.totals.new_tokens,
            "conflicts": snap.conflicts.len(),
            "stalled": snap.agents.iter().filter(|a| a.status == AgentStatus::Stalled).count(),
            "errored": snap.agents.iter().filter(|a| a.status == AgentStatus::Errored).count(),
        })),
        "list_agents" => {
            let status = args.get("status").and_then(Value::as_str);
            let source = args.get("source").and_then(Value::as_str);
            let list: Vec<Value> = snap
                .agents
                .iter()
                .filter(|a| status.is_none_or(|s| status_name(a.status) == s))
                .filter(|a| source.is_none_or(|s| a.source.id() == s))
                .map(|a| agent_json(a, now))
                .collect();
            Ok(Value::Array(list))
        }
        "get_agent" => {
            let id = args
                .get("id")
                .and_then(Value::as_str)
                .ok_or("`id` is required")?;
            snap.agents
                .iter()
                .find(|a| a.id == id)
                .map(|a| agent_json(a, now))
                .ok_or_else(|| format!("no agent `{id}`"))
        }
        "list_conflicts" => Ok(Value::Array(
            snap.conflicts
                .iter()
                .map(|c| json!({"file": c.path_label, "agents": c.agents, "seconds": (now - c.since_ms).max(0) / 1000}))
                .collect(),
        )),
        other => Err(format!("unknown tool `{other}`")),
    }
}

fn ok(id: &Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn err(id: &Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

/// Handle one JSON-RPC message. `None` for notifications (no reply).
pub fn handle(observer: &mut Observer, msg: &Value, now_ms: i64) -> Option<Value> {
    let method = msg.get("method").and_then(Value::as_str)?;
    let id = msg.get("id")?; // no id = notification
    Some(match method {
        "initialize" => ok(
            id,
            json!({
                "protocolVersion": PROTOCOL,
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "nanonexus-activity", "version": env!("CARGO_PKG_VERSION")},
                "instructions": "Read-only view of agents running on this computer (Claude Code, Codex, NanoNexus). Names, counts and times only."
            }),
        ),
        "ping" => ok(id, json!({})),
        "tools/list" => ok(id, json!({"tools": tools()})),
        "tools/call" => {
            let params = msg.get("params").cloned().unwrap_or(Value::Null);
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            let (snap, _) = observer.poll(now_ms);
            match call_tool(name, &args, &snap) {
                Ok(v) => ok(
                    id,
                    json!({"content": [{"type": "text", "text": v.to_string()}], "isError": false}),
                ),
                Err(e) => ok(
                    id,
                    json!({"content": [{"type": "text", "text": e}], "isError": true}),
                ),
            }
        }
        _ => err(id, -32601, "method not found"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::observer::ObserverConfig;

    const NOW: i64 = 1_800_000_000_000;

    fn demo() -> Observer {
        Observer::new(ObserverConfig {
            window_hours: 6,
            demo: true,
            ..Default::default()
        })
    }

    fn call(o: &mut Observer, name: &str, args: Value) -> Value {
        let msg =
            json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":name,"arguments":args}});
        let r = handle(o, &msg, NOW).expect("reply");
        assert_eq!(r["result"]["isError"], false, "{r}");
        serde_json::from_str(r["result"]["content"][0]["text"].as_str().expect("text")).expect("json")
    }

    #[test]
    fn handshake_and_tool_list() {
        let mut o = demo();
        let init = handle(
            &mut o,
            &json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
            NOW,
        )
        .expect("reply");
        assert_eq!(init["result"]["protocolVersion"], PROTOCOL);
        assert!(
            handle(
                &mut o,
                &json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
                NOW
            )
            .is_none()
        );
        let list = handle(
            &mut o,
            &json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
            NOW,
        )
        .expect("reply");
        let names: Vec<&str> = list["result"]["tools"]
            .as_array()
            .expect("tools")
            .iter()
            .filter_map(|t| t["name"].as_str())
            .collect();
        assert_eq!(names, ["summary", "list_agents", "get_agent", "list_conflicts"]);
        assert!(
            list["result"]["tools"]
                .as_array()
                .expect("tools")
                .iter()
                .all(|t| t["annotations"]["readOnlyHint"] == true)
        );
    }

    #[test]
    fn tools_answer_from_the_demo() {
        let mut o = demo();
        let s = call(&mut o, "summary", json!({}));
        assert_eq!(s["agents"], 19);
        assert_eq!(s["running"], 4);
        assert_eq!(s["conflicts"], 1);
        let running = call(&mut o, "list_agents", json!({"status": "running"}));
        assert_eq!(running.as_array().expect("list").len(), 4);
        let id = running[0]["id"].as_str().expect("id").to_string();
        let one = call(&mut o, "get_agent", json!({"id": id}));
        assert_eq!(one["status"], "running");
        let c = call(&mut o, "list_conflicts", json!({}));
        assert_eq!(c[0]["file"], "cart/Cart.tsx");
    }

    #[test]
    fn errors_are_reported_not_thrown() {
        let mut o = demo();
        let r = handle(&mut o, &json!({"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"get_agent","arguments":{"id":"nope"}}}), NOW).expect("reply");
        assert_eq!(r["result"]["isError"], true);
        let r = handle(&mut o, &json!({"jsonrpc":"2.0","id":10,"method":"nope"}), NOW).expect("reply");
        assert_eq!(r["error"]["code"], -32601);
    }

    #[test]
    fn replies_carry_no_free_text_from_logs() {
        let mut o = demo();
        let all = call(&mut o, "list_agents", json!({})).to_string();
        assert!(!all.contains("prompt") && !all.contains("command"));
    }
}
