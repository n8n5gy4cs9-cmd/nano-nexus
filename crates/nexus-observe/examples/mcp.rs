//! Read-only MCP server (stdio) for the Activity Observatory (T19.17).
//!
//! Add to Claude Code:  claude mcp add nanonexus-activity -- cargo run -q -p nexus-observe --example mcp
//! (or point it at a built binary). It reads `~/.claude` and `~/.codex` like the Activity page does,
//! and answers with names, counts and times only. `--demo` serves synthetic data.

use std::io::{BufRead, Write};

use nexus_observe::mcp::handle;
use nexus_observe::observer::{default_claude_root, default_codex_root};
use nexus_observe::{Observer, ObserverConfig};

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

fn main() {
    let demo = std::env::args().any(|a| a == "--demo");
    let mut observer = Observer::new(ObserverConfig {
        window_hours: 6,
        nanonexus: false,
        claude_root: default_claude_root(),
        codex_root: default_codex_root(),
        demo,
    });
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(msg) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        if let Some(reply) = handle(&mut observer, &msg, now_ms())
            && (writeln!(out, "{reply}").is_err() || out.flush().is_err())
        {
            break;
        }
    }
}
