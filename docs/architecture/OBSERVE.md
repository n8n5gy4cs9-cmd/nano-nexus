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
enum ActivitySource { NanoNexus, ClaudeCode, Codex }     // exported to TS with the `Activity*` prefix
enum AgentStatus { Running, Idle, Stalled, Errored }     // derived, never stored in logs
struct Obs {                      // one observation, privacy-reduced
    source: ActivitySource, session: String, agent: String,      // agent == session for the main agent
    parent: Option<String>,       // subagent -> parent agent id
    project: String,              // cwd (display: last two path components)
    at_ms: i64,
    kind: ObsKind,
}
enum ObsKind {
    Start { model: Option<String> },
    Model(String),
    Title(String),                // display name; only NanoNexus' own source and the demo set it
    Tokens(TokenTotals),                                   // a delta: input, output, cache_read, cache_write
    ToolStart { id: String, tool: String, files: Vec<String> }, // `files` only for edit/write tools
    ToolEnd { id: String, ok: bool },
    Message { role: Role },                               // user | assistant (no text)
    Error,                                                // tool error, API error, aborted turn
    End,
}
```
What is **never** in `Obs`: prompt/assistant text, bash commands, tool inputs other than the single
`file` path of an edit tool, tool results, file contents, env, API keys. Task titles in the graph are
`"<Model short name> · <last 4 of the id>"` (subagents: `Subagent · …`). Only NanoNexus' own source sets a
`Title` (the assistant or job name). Claude Code `summary` records and Codex titles are *not* used: they contain text.

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
- **Status** (derived from timing only):
  `Errored` if the last `Error` is < 2 min old and nothing progressed since (any later message, tool start clears it);
  else `Idle` if the agent is not in a turn (a turn starts with a user/assistant message or a tool start and ends with `End` or `Error`);
  else, in a turn: silent for < 3 min (< 10 min while a tool is open) = `Running`; then `Stalled` up to 60 min of silence; after that `Idle`.
  A failed *tool call* (`ToolEnd ok=false`) only raises the agent's `errors` counter, it does not make the agent `Errored`.
- **Dashboard numbers:** `tasks` = visible agents, `tasks_done` = those that are `Idle`, `active` = `Running`, `events` = observations
  counted, `uptime` = now − start of the oldest visible agent, `new_tokens` = input + output + cache_write (cache reads separate).
- **File conflict:** two *different agents* (any source) have an open or recent (≤ 2 min) edit-tool touch on the same path → one
  conflict, gone 2 min after the last touch. Relative paths (Codex patches) resolve against the agent's cwd; paths are normalised
  lexically (`.`/`..`, separators, case-insensitive on Windows/macOS) and shown as the last two components.
- **Graph:** hub per source → projects → agents. Layout is deterministic (golden-angle placement in order of first appearance), so
  nodes do not jump when others appear.
- **Alerts** (`Observer::attention`): transitions into `Stalled`/`Errored` and new conflicts, never on the first poll; `Hub` limits to one per
  subject per 10 min and one per 30 s overall.

## 6. Snapshot (UI) and replay
`ActivitySnapshot { at_ms, totals{tasks,tasks_done,events,active,agents,uptime_ms,new_tokens,cache_read_tokens}, sources[], projects[], agents[], conflicts[], drift }`
plus per-agent `{id,label,source,project,parent,model,status,tool,tokens,started_ms,last_ms,errors}`.
Replay keeps one compact frame per 10 s for `window_hours` (≈ 2 200 frames × ~2 KB at 20 agents) and answers `frame_at(t)`.
Particle streams are a **UI effect** driven by `events_per_s` on each edge; the crate only exposes rates.

## 7. Where things live
`nexus-observe`: `claude.rs`, `codex.rs` (parsers) · `tail.rs` · `aggregate.rs` · `replay.rs` · `observer.rs` (tail + aggregate + replay + attention) ·
`hub.rs` (one lock, view, alert limits: what the app calls) · `selfsrc.rs` · `demo.rs` · `render.rs` (terminal table) · `mcp.rs`.
App: `service/observe.rs` (settings → `Hub`, 2 s poll loop only while the page is open or a source is on, notifications), `commands/observe.rs`
(`observe_snapshot|replay|demo|watch`, event `observe:snapshot`). UI: `features/activity/*`, `lib/activity-layout.ts`, `lib/activity-particles.ts`.

## 8. Privacy and safety tests (T19.09)
1. Canary test: a fixture full of strings like `SECRET_PROMPT`, `rm -rf /`, `sk-ant-…` must not appear in any
   `Snapshot`, `Obs` debug output, log line (tracing capture), or replay frame.
2. Roots are canonicalised; a symlink pointing outside is not followed.
3. Read-only: the crate opens files with `OpenOptions::new().read(true)` only; no `write` calls on those roots.
4. Desktop-only: `nexus-share` rejects `observe_*` RPCs (test).
