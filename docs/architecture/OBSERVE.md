# Activity Observatory (`crates/nexus-observe`)

> A live, read-only graph of what is running on this computer: NanoNexus' own work, and (opt-in)
> Claude Code and Codex sessions. ADR-017. UI: **Activity** page. Spec: `docs/milestones/M19-activity.md`.

## 1. Data flow
```
 ~/.claude/projects/**.jsonl ─┐                       ┌─► Snapshot (graph, 2 Hz) ─► UI canvas
 ~/.codex/sessions/**.jsonl  ─┼─► Tailer ─► Parser ─► Aggregator ─┤
 NanoNexus service events    ─┘  (offsets)  (Obs)     (state)    ├─► Replay ring (RAM)
                                                                 └─► Alerts (stalled/errored/conflict)
```
`Parser` turns one JSONL line into zero or more `Obs` (observations). It is the **only** code that sees
raw log text and returns nothing but the fields below; everything else is dropped.

## 2. Types (all `serde` + `specta::Type`)
```rust
enum Source { NanoNexus, ClaudeCode, Codex }
enum Status { Running, Idle, Stalled, Errored }          // derived, never stored in logs
struct Obs {                      // one observation, privacy-reduced
    source: Source, session: String, agent: String,      // agent == session for the main agent
    parent: Option<String>,       // subagent -> parent agent id
    project: String,              // cwd (display: last two path components)
    at_ms: i64,
    kind: ObsKind,
}
enum ObsKind {
    Start { model: Option<String> },
    Model(String),
    Tokens { input: u64, output: u64, cache_read: u64, cache_write: u64 }, // a delta for one message
    ToolStart { tool: String, file: Option<String> },    // `file` only for edit/write tools
    ToolEnd { tool: String, ok: bool },
    Message { role: Role },                               // user | assistant (no text)
    Error,                                                // tool error, API error, aborted turn
    End,
}
```
What is **never** in `Obs`: prompt/assistant text, bash commands, tool inputs other than the single
`file` path of an edit tool, tool results, file contents, env, API keys. Task titles in the graph are
the tool-free label `"<Model short name> · <N> msgs"` unless the source supplies a title field that is
metadata (Claude Code `summary` records are *not* used, they contain text).

## 3. Source formats (observed; tolerate drift)
**Claude Code** (`~/.claude/projects/<slug>/<sessionId>.jsonl`, subagents in `<sessionId>/subagents/agent-*.jsonl`
with `isSidechain: true`). Lines are objects with `type` (`user|assistant|system|attachment|…`), `timestamp`,
`sessionId`, `cwd`, `agentId?`. `assistant.message.{model,id,usage,content[]}`; `content[]` blocks of
`type: tool_use {id,name,input}` (take `name`, and `input.file_path` iff name ∈ edit tools) and
user-side `tool_result {tool_use_id,is_error}`. **One API message can appear on several lines (streaming):
count `usage` once per `message.id`** (keep the max per field).
**Codex** (`~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`, `$CODEX_HOME` honoured). Lines `{timestamp,type,payload}`:
`session_meta` (id, cwd), `turn_context` (model), `response_item` with `payload.type` = `function_call`
(name, call_id) / `function_call_output` / `message` / `reasoning`, `event_msg` with `payload.type` =
`token_count` (`info.total_token_usage` is cumulative: take deltas), `agent_message`, `error`, `turn_aborted`.
Roots: `CLAUDE_CONFIG_DIR` (default `~/.claude`) and `CODEX_HOME` (default `~/.codex`).

## 4. Tailer
- Discovery every 5 s: only files with `mtime` within `window_hours` (default 6); cap 400 files.
- Per-file state `{offset, partial_line, inode/len}`. Read only appended bytes; on first sight of a file,
  read the last ≤ 2 MB to rebuild recent state (so a restart is not blind). A shrinking file resets.
- A line > 1 MB is skipped. A half-written last line is kept in `partial_line` until its newline arrives.
- Unparseable lines increment `drift_lines`; the UI shows a notice when `drift_lines/total > 5 %`.

## 5. Aggregator rules
- **Status:** `Errored` if the last obs is `Error` and < 2 min old (or an open tool ended `ok=false` with no later obs);
  `Running` if any obs in the last 20 s **or** a tool is open and the last obs is < 5 min old;
  `Stalled` if a tool is open or the last message was from the user and nothing arrived for ≥ 5 min (and < 60 min);
  otherwise `Idle`. Agents idle for > `window_hours` drop off the graph.
- **Tokens:** totals are per agent, per project, per source; "new tokens" = input + output + cache_write (cache reads shown separately).
- **File conflict:** two *different agents* (any source) have an open or recent (≤ 2 min) edit-tool obs on the same
  canonical path → one `Conflict { path_label, agents[] }`, cleared 2 min after the last obs. Same agent twice is not a conflict.
  Paths are compared after lexical normalisation (case-insensitive on Windows/macOS) and are shown as the last two components.
- **Graph:** `root` (source or "All sources") → `projects` → `agents` (subagents nested under their parent agent).

## 6. Snapshot (UI) and replay
`Snapshot { at_ms, totals{tasks,tasks_done,events,active,agents,uptime_ms,new_tokens}, sources[], projects[], agents[], conflicts[], drift }`
plus per-agent `{id,label,source,project,parent,model,status,tool,tokens,started_ms,last_ms,errors}`.
Replay keeps one compact frame per 10 s for `window_hours` (≈ 2 200 frames × ~2 KB at 20 agents) and answers `frame_at(t)`.
Particle streams are a **UI effect** driven by `events_per_s` on each edge; the crate only exposes rates.

## 7. Privacy and safety tests (T19.09)
1. Canary test: a fixture full of strings like `SECRET_PROMPT`, `rm -rf /`, `sk-ant-…` must not appear in any
   `Snapshot`, `Obs` debug output, log line (tracing capture), or replay frame.
2. Roots are canonicalised; a symlink pointing outside is not followed.
3. Read-only: the crate opens files with `OpenOptions::new().read(true)` only; no `write` calls on those roots.
4. Desktop-only: `nexus-share` rejects `observe_*` RPCs (test).
