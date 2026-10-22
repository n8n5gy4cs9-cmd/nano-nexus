# Spikes and Measurements

Record every spike result here: the question, the evidence (a small table), and the decision.
Each spike is defined in [`PLAN.md` §3](PLAN.md#3-spikes-answer-early-results-go-into-docsspikesmd).
Entry format:

```
## S<n>: <title> (YYYY-MM-DD, llama.cpp <tag>, <model file>, <machine>)
Result: ✅ / ⚠️ / ❌
Evidence: | case | result | notes |
Decision: …   (update the ADR/catalog/doc if this changes anything)
```

## Environment (T00.08)
_Not yet recorded. Paste `scripts/doctor.sh` output plus the Mac model, chip, RAM, and macOS version._

## S1: Thinking toggle
_Pending (T01.04)._

## S2: JSON-schema constrained output
_Pending (T01.05)._

## S3: Quantization benchmark
_Pending (T01.07)._

## S4: tree-sitter 0.27 + grammar pins (2026-10-11, Linux container)
- Pins (workspace `Cargo.toml`): tree-sitter 0.27, python 0.25, javascript 0.25, typescript 0.23 (TS + TSX), rust 0.24, c-sharp 0.23, go 0.25, java 0.23. All load against tree-sitter 0.27 (they share `tree-sitter-language`); no patches needed.
- `code::lang::tests` parses a hand-written sample per language with zero error nodes. `.jsx` uses the JavaScript grammar (includes JSX).
- Pending 🍎: parse your own projects on the Mac (`T09.17` covers the real run). GDScript is not needed (P2).

## S5: PDF extraction quality
_Pending (T08.06)._

## S6: tauri-specta with Tauri 2.12 (2026-10-11, tauri 2.12.3, tauri-specta 2.0.0-rc.25, specta 2.0.0-rc.25, specta-typescript 0.0.12, Linux container)
**Result: ✅ works; ts-rs fallback (ADR-006) not needed.** `cargo test -p nano-nexus export_bindings` (and any debug launch) regenerates `apps/desktop/src/bindings.ts` with typed `commands.*`, `events.*` and all payload types.

Findings:
- **`serde_json::Value` crashes the exporter** (infinite recursion, stack overflow, even with a 64 MB stack). Never put `Value` in a command, event, or exported type. `settings_set` takes a typed `SettingValue` enum instead; `NotificationAction.args` must get the same treatment before notifications are exported (T03.12).
- **BigInt is forbidden by default** (`u64`/`i64`). We use `Builder::dangerously_cast_bigints_to_number()`, which is safe here because timestamps (ms) and byte counts stay far below 2^53.
- Event names: `#[tauri_specta(event_name = "runtime:status")]` gives the colon-style names from DATA-AND-IPC §3.
- The `macos-private-api` cargo feature must match `macOSPrivateApi` in the config on every platform, so it is enabled on macOS only (`[target.'cfg(target_os = "macos")'.dependencies]`).
- Linux builds need `libwebkit2gtk-4.1-dev` (and GTK3); not needed on the Mac.

Decision: keep tauri-specta; pin both `=2.0.0-rc.25` while it is a release candidate.

## S7: Windows build cross-compiled on the Mac
_Pending (T03.16)._

## S8: Windows Vulkan / CPU fallback
_Pending (T13.10)._

## S9: Cross-compiling llama.cpp for Windows (optional)
_Pending (T16.08)._

## Router eval (T06.13)
_Pending._

## Rename real run (T09.17)
_Pending._

## Performance budgets (T15.06)
_Pending._

## Assistant repair verification (2026-10-22)

- REFE reference: Pi's `packages/agent/src/agent-loop.ts` feeds tool results into subsequent
  decisions; `packages/coding-agent/examples/extensions/subagent/` isolates child tasks and
  propagates cancellation. NanoNexus ports these behaviors into its constrained router and
  single scheduler, preserving permission checks and journaled writes.
- Reproduced before fixing: `cargo test -p nexus-agent reads_a_file_then_answers` failed because
  the next decision lacked the read contents. `latest_write_status_survives_a_full_observation_budget`
  and `tool_status_has_priority_over_large_user_attachments` also failed before their fixes.
- The read → create regression uses real tools, asserts the proposal creates no file, applies
  the change through `Journal::apply`, and verifies the exact file contents.
- Planner/Reviewer consultations are text-only, sequential on the active model, with bounded
  input/output and parent cancellation. Tests cover dispatch, feedback to the parent, isolated
  request construction, disabled/recursive targets, truncation, and in-flight cancellation.
- Full `scripts/check.sh` passed for T06.14/T06.15, including the integrated Activity Observatory.
  Frontend dependencies were restored with `pnpm install --frozen-lockfile`; no dependency changes.
- Live-model evaluation remains unverified: no GGUF exists under `vendor/models`, and the three
  models registered in the local app database all refer to missing bundle paths. No model was
  downloaded and no user database rows were changed. T06.13/T18.02 remain open. Concurrent
  light/strong inference (T18.04) is not implemented by specialist consultation.

- T18.06 final verification: full `scripts/check.sh` passes (140 agent unit tests, 73 app service
  unit tests, frontend checks, all 22 built-in prompts). Added three structural golden cases
  each for Planner and Reviewer; `node --test scripts/eval-assistants.test.mjs` passes.
  `scripts/eval-assistants.sh --assistant planner` exits 2 with "no default model"; see Q-10.

## Live assistant execution repair (2026-10-22, T06.16)

- Owner supplied Gemma's existing Downloads location and requested only MiniCPM's default download.
  MiniCPM5 1B Opus-Fable5 V2 Q8 was downloaded once (1.1 GB), verified (SHA-256 prefix
  `fc3ee1eddd305c15`), and set as the development default. Gemma was used in place; no copies.
- A new ignored chat-service test reproduced the reported failure: real Gemma selected
  `write_file` with a path and no content; the file never existed. Required-argument validation
  alone caused a correction retry but the same field was missing again.
- A union of flat per-action schemas now enforces required arguments during decoding. The
  original end-to-end scenario passes on Gemma (~72 s for four turns) and MiniCPM (~13 s):
  Agent creates, reads/edits, consults Reviewer, and Duo creates; exact disk contents and
  successful tool traces are asserted. These writes go through the actual journal/apply service.
- Plain tool statuses are tagged consistently; failed decisions reach final-answer context.
  Conditional examples are skipped when empty. Temporary router diagnostics were removed.
- Full `scripts/check.sh` passes with models downloaded. Tests now receive an isolated vendor
  model root; a real local model previously polluted two model-discovery fixture counts.
- Gemma's separate inference smoke: streaming and schema JSON passed; initial cold readiness
  was 13.24 s, exceeding the existing 10 s target. That assertion was not weakened.
- Default-model golden evaluation remains incomplete quality work: first run 8/12 assistants
  met the 80% threshold; a captured repeat met it for 6/12. Bug Hunter, Scribe, Shell Sage and
  TL;DR failed both; Nexus and Planner also failed a case on the repeat. Typical failures are
  omitted facts, invented agenda detail, long answers, and incorrect off-by-one diagnosis.
  The golden checks were not weakened. MiniCPM can still echo internal markup in final wording
  despite correct file operations. Tool execution success is not a claim of perfect model output.
