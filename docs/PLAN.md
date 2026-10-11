# NanoNexus: Master Plan and Roadmap

> **How to use this plan:** pick the lowest unchecked task in [`tasks.md`](../tasks.md), read its
> milestone doc in [`docs/milestones/`](milestones), implement, verify, tick the box. Never start a
> milestone before the previous milestone's exit criteria pass (spikes excepted).

## 1. Milestones at a glance

| # | Milestone | Outcome (demo) | Depends on | Size |
|---|---|---|---|---|
| **M00** | [Bootstrap](milestones/M00-bootstrap.md) | Workspace compiles, CI-style checks pass, `doctor.sh` green | — | S |
| **M01** | [Runtime and model pipeline](milestones/M01-runtime-model.md) | `llama-server` built from the pinned commit; model fetched and verified; spikes S1–S3 answered | M00 | M |
| **M02** | [Inference crate `nexus-llm`](milestones/M02-nexus-llm.md) | `cargo run --example chat` streams a reply from the owned server; JSON-schema call validated | M01 | L |
| **M03** | [App shell and design system](milestones/M03-app-shell.md) | Pretty empty app: sidebar, title bar, themes, runtime pill, notifications plumbing, walking-skeleton `.app` | M02 | L |
| **M04** | [Chat MVP](milestones/M04-chat.md) ★ | **First daily-usable build:** streaming chat, history, stop/regenerate, thinking toggle | M03 | L |
| **M05** | [Permissions and change sets](milestones/M05-permissions.md) | Grant folders; permission prompts; change set preview/apply/undo; audit; macOS TCC status | M04 | L |
| **M06** | [Assistants, prompts, tools](milestones/M06-assistants.md) | 18 assistants in a gallery; builder; router tools reading granted files | M05 | L |
| **M07** | [Model manager and downloads](milestones/M07-models.md) | Download view: catalog, HF search, resumable verified downloads, activate | M04 (M06 nice) | M |
| **M08** | [Analyzer and reports](milestones/M08-analyzer.md) | Folder Scout report with treemap and AI summary; DocLens PDF summary and Q&A; other pipelines | M06 | XL |
| **M09** | [Rename Forge](milestones/M09-rename-forge.md) | Your old Godot game renamed safely with preview, undo, and Godot verify | M08 | XL |
| **M10** | [Code editor](milestones/M10-editor.md) | Tabs, highlighting, save via journal, AI actions with diff accept | M05, M06 | M |
| **M11** | [Notifications, jobs, watches](milestones/M11-notifications-jobs.md) | Jobs panel, per-kind notification settings, scheduled folder watches | M08 | M |
| **M12** | [Bundling and release](milestones/M12-bundling.md) | `scripts/build-app.sh` → signed, verified `NanoNexus.app` that works offline after being moved | M04+ (final after M11) | M |
| **M13** | [Polish](milestones/M13-polish.md) | ⌘K palette, onboarding, shortcuts, Quick Ask, tray, a11y, perf, assistant evals | all | L |
| **M14** | [Optional extensions](milestones/M14-optional.md) | Linux/Windows builds, custom endpoint, embeddings RAG, vision, voice, notarization | M12 | — |

Sizes: S ≈ ½ day · M ≈ 1–2 days · L ≈ 3–5 days · XL ≈ 1–2 weeks (for one focused developer + AI).

## 2. Dependency graph

```
M00 ─► M01 ─► M02 ─► M03 ─► M04★ ─► M05 ─► M06 ─► M08 ─► M09
                              │        │      │      └──► M11
                              │        └──────┴─► M10
                              └─► M07 (can run in parallel with M05/M06)
                              └─► M12 walking skeleton early (in M03), final hardening after M11
everything ─► M13 ─► M14 (optional)
```

## 3. Spikes (answer early; results go into `docs/SPIKES.md`)

| ID | Question | When | How | Decision it drives |
|---|---|---|---|---|
| **S1** | Does `chat_template_kwargs.enable_thinking=false` disable thinking for the bundled GGUF? Does `--reasoning-format deepseek` split thoughts into `reasoning_content`? | M01 | `curl` against the built server with both values; inspect output | Whether the "Think harder" toggle is supported (catalog `supports_thinking_toggle`) |
| **S2** | Does `response_format` with the OpenAI `json_schema` shape constrain output on b11552, including `pattern` and `enum`? With thinking off and on? | M01 | `curl` with `schemas/llm/rename-proposals.schema.json`; 20 runs | ADR-003 shape; whether to force thinking off |
| **S3** | Q8_0 vs Q4_K_M on your Mac: tok/s, TTFT, RAM, quality on 5 prompts | M01 | `scripts/bench-model.sh` (written in M01) | ADR-008 default quant |
| **S4** | Does `tree-sitter-gdscript 6.1` load with `tree-sitter 0.27` (ABI)? Does it parse Godot 3 *and* 4 syntax from your game? | before M09 | Tiny test binary parsing your project | Grammar pin or patch |
| **S5** | `pdf-extract` quality on 3 of your PDFs (text, columns, scanned) | before M08 | Compare against `pdftotext` output | PDF extractor choice and fallback |
| **S6** | `tauri-specta 2.0.0-rc.25` compiles with `tauri 2.12`? | M03 | Minimal command + export | ADR-006 primary vs fallback |

## 4. Definition of Done (every task)

1. Code compiles: `cargo build --workspace` and `pnpm -C apps/desktop build` (once they exist).
2. Checks pass: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
   `cargo test --workspace`, `pnpm -C apps/desktop lint && pnpm -C apps/desktop typecheck`.
3. New logic has tests (Rust unit tests at minimum; UI logic via Vitest where it's pure).
4. No TODOs without a task ID (`// TODO(T08.12): …`).
5. `tasks.md` box ticked, with the date and a short note if anything deviated.
6. Docs updated if behavior or contracts changed (architecture docs are contracts).
7. Commit message: `T04.07: stream chat deltas over Channel` (task ID first).

## 5. Verification commands (copy-paste)

```bash
scripts/doctor.sh                                   # toolchain
scripts/progress.sh                                 # what's done / next
cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
cargo test -p nexus-llm -- --ignored                # real-model smoke (Mac, after M01)
pnpm -C apps/desktop lint && pnpm -C apps/desktop typecheck && pnpm -C apps/desktop test
pnpm -C apps/desktop tauri dev                      # run
scripts/build-app.sh                                # bundle (M12)
```

## 6. Risk burn-down order

1. **Runtime and bundling** (M01, M03 walking skeleton): the riskiest platform work goes first.
2. **Structured output with this model** (S1/S2): the core reliability assumption.
3. **Permission engine correctness** (M05): safety-critical, property-tested.
4. **Rename correctness** (M09): Godot adapter, tested on a copy of the real game first.

## 7. Working agreements for AI agents
See [`AGENTS.md`](../AGENTS.md) and [`docs/agent-playbook/`](agent-playbook). In short: one
task per commit, smallest change that passes the DoD, never weaken the permission engine, never
commit `vendor/` or `*.gguf`, and ask in `docs/QUESTIONS.md` when blocked.
