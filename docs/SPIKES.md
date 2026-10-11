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
