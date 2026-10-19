# M19: Activity Observatory (live agent dashboard)

**Goal:** an **Activity** page that shows, as a live graph, everything that is running: NanoNexus' own
chats/jobs/pipelines and (opt-in) Claude Code and Codex sessions on this computer: model, tokens, current
tool, run time, stalled/errored agents, same-file conflicts, with replay of the last few hours.

**Spec:** ADR-017, `docs/architecture/OBSERVE.md`. **Prereqs:** M03 (shell), M11 (notifications). Independent of M12-M18.
**Exit demo:** open Activity → demo data shows 3 projects / 19 agents with animated streams → enable Claude Code
in Settings → start a Claude Code session in a terminal → its node appears within 5 s, tokens climb, the current tool
shows; start a second agent editing the same file → a red "FILE CONFLICT" chip; scrub the replay back 30 min; toggle dark/light.

## Scope
- Crate `nexus-observe`: model, Claude Code + Codex parsers, tailer, aggregator, replay, self-source, demo generator.
- App: `service::observe` (poller, 2 Hz `observe:snapshot` event, commands `observe_snapshot`, `observe_replay`, `observe_set_source`),
  settings `observe.*`, "needs attention" notifications (stalled / errored / conflict; rate-limited).
- UI: `features/activity` (stats strip, canvas graph with pan/zoom/fit, particle streams, legend, detail panel, replay bar,
  empty and consent states, light/dark, reduced-motion), nav item, palette entry.
- Extras: terminal view (`cargo run -p nexus-observe --example top`) and a read-only MCP stdio server (`examples/mcp.rs`).

## Design notes
- Graph layout is deterministic (radial: root → projects on a ring → agents on small rings around each project; stable
  angles from a hash of the id so nodes do not jump). Pure TS in `lib/activity-layout.ts` with unit tests.
- Canvas 2D, `devicePixelRatio`-aware, one `requestAnimationFrame` loop that pauses when the page is hidden; particle count
  capped (≈ 1 500) and scaled down when frame time > 20 ms. Colours come from CSS tokens (no raw hex).
- The Rust side never ticks faster than 2 Hz; the UI interpolates.

## Steps
1. Docs/ADR/tasks. 2. Crate + types. 3. Parsers with fixtures. 4. Tailer. 5. Aggregator + replay + privacy tests.
6. Self-source + demo. 7. App service/commands. 8. UI. 9. Terminal + MCP. 10. Docs + exit check on the Mac.

## Exit criteria
- [ ] Exit demo passes with real Claude Code and Codex sessions (🍎).
- [ ] Privacy canary test green; sources off = no file under `~/.claude` / `~/.codex` is opened (test with a fake home).
- [ ] 20 agents at 60 fps on the Mac (≤ 30 % of one core idle-ish), page hidden = ~0 CPU.

## Pitfalls
- Streaming duplicates in Claude Code logs: dedupe usage by `message.id`.
- Huge old logs: never read whole files; tail from `len - 2 MB`.
- Windows paths: case-insensitive compare, `\` separators; `CLAUDE_CONFIG_DIR` may be set.
- Log formats change between releases: skip unknown lines, count drift, never panic (no `unwrap`).
