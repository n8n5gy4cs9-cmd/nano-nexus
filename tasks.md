# NanoNexus: Tasks

> **How to use:** work top to bottom. `[ ]` = to do, `[x]` = done. When you finish a task: tick it,
> add `(YYYY-MM-DD)` and a short note if anything deviated, then **commit and push to `main`**
> (`git commit -m "T04.07: …" && git push origin main`). Spec for each milestone: `docs/milestones/`.
> Run `scripts/progress.sh` for a summary. `scripts/progress.sh --write` refreshes the block below.

<!-- progress:start -->
**Progress:** 63/229 tasks done (27%) · **Next:** T00.08 🍎 Run `scripts/doctor.sh` and `scripts/check.sh` on the Mac; record the environment in `docs/SPIKES.md` — ⏳ Mac-only; owner to run (skipped in the Linux container)
<!-- progress:end -->

Legend: 🔬 spike (answer a question, record in `docs/SPIKES.md`) · 🍎 needs the Mac · 🪟 needs Windows (VM or PC) · ⭐ milestone exit check

---

## P: Planning (this repo's first commit)
- [x] **TP.01** PRD with features, acceptance criteria, risks (`docs/PRD.md`) (2026-10-11)
- [x] **TP.02** Architecture docs: overview, inference contract, bundling, permissions, assistants, analyzer, model manager, data/IPC, LAN share (2026-10-11)
- [x] **TP.03** UI/visual design system and screen specs (`docs/design/UI-DESIGN.md`) (2026-10-11)
- [x] **TP.04** ADR log with 16 decisions (`docs/DECISIONS.md`) (2026-10-11)
- [x] **TP.05** Master plan + 17 milestone specs (`docs/PLAN.md`, `docs/milestones/`) (2026-10-11)
- [x] **TP.06** This task list (2026-10-11)
- [x] **TP.07** 18 built-in assistants with researched system prompts (`assistants/`) (2026-10-11)
- [x] **TP.08** Internal pipeline prompts (`prompts/internal/`) and JSON Schemas (`schemas/`) (2026-10-11)
- [x] **TP.09** Model catalog (`catalog/models.json`) + bundle profiles (`bundles/`) (2026-10-11)
- [x] **TP.10** AI agent handbook (`AGENTS.md`, `CLAUDE.md`, `docs/agent-playbook/`, `.claude/skills/`) (2026-10-11)
- [x] **TP.11** Helper scripts: `doctor.sh`, `progress.sh`, `check.sh`, `setup-runtime.sh`, `fetch-model.mjs` (2026-10-11)
- [x] **TP.12** User guide `guide.html` (builds, offline bundles, Windows from Mac, LAN share) (2026-10-11)
- [x] **TP.13** `docs/SPIKES.md` + `docs/QUESTIONS.md` skeletons (2026-10-11)

## M00: Bootstrap · [spec](docs/milestones/M00-bootstrap.md)
- [x] **T00.01** Root Cargo workspace (resolver 3, edition 2024, rust-version 1.90), rust-toolchain.toml, rustfmt.toml, clippy.toml (2026-10-11)
- [x] **T00.02** Create library crates `nexus-{core,store,llm,models,fs,analyze,agent,share}` (empty, compiling, workspace lints) (2026-10-11)
- [x] **T00.03** `nexus-core::ids` (UUIDv7 newtypes) + tests (2026-10-11)
- [x] **T00.04** `nexus-core::error` (ErrorCode, NexusError, IpcError) + serialization tests (2026-10-11)
- [x] **T00.05** `nexus-core::paths` (AppPaths, ensure_dirs, portable mode) + tests (2026-10-11)
- [x] **T00.06** `nexus-core::settings` (all keys + defaults, incl. share.*) + `events` types + tests (2026-10-11)
- [x] **T00.07** Root `package.json`, `pnpm-workspace.yaml`, `.nvmrc`, `.editorconfig` (2026-10-11)
- [ ] **T00.08** 🍎 Run `scripts/doctor.sh` and `scripts/check.sh` on the Mac; record the environment in `docs/SPIKES.md` — ⏳ Mac-only; owner to run (skipped in the Linux container)
- [x] **T00.09** (optional) GitHub Actions: `scripts/check.sh` on macos-14 (2026-10-11)
- [x] ⭐ **T00.10** Exit check: `scripts/check.sh` green; `cargo doc -p nexus-core` has no warnings (2026-10-11)

## M01: Runtime and model pipeline · [spec](docs/milestones/M01-runtime-model.md)
- [ ] **T01.01** 🍎 Run `scripts/setup-runtime.sh`; fix any issue; confirm `runtime.lock.json`, arm64-only, system-libs-only
- [ ] **T01.02** 🍎 Run `node scripts/fetch-model.mjs` (default model) + `--verify`; confirm `vendor/models/index.json`
- [x] **T01.03** Write `scripts/smoke-runtime.sh` (random port + token, health, auth check 200/401, SSE stream, JSON schema call, clean kill) — written + syntax-checked only; owner must run on the Mac (2026-10-11)
- [ ] **T01.04** 🍎🔬 **S1** thinking toggle (`enable_thinking`, `--reasoning-format deepseek`, `--reasoning off`) → SPIKES.md
- [ ] **T01.05** 🍎🔬 **S2** JSON-schema shapes, `pattern` + `enum`, thinking on/off, 20 runs → SPIKES.md
- [x] **T01.06** Write `scripts/bench-model.sh` + `scripts/bench-prompts.txt` (TTFT, tok/s, peak RSS) — written + syntax-checked only; owner must run on the Mac (2026-10-11)
- [ ] **T01.07** 🍎🔬 **S3** Q8_0 vs Q4_K_M (+ official MiniCPM5-1B) → SPIKES.md; update ADR-008/catalog default if needed
- [ ] ⭐ **T01.08** Exit check: smoke script passes 3× with no orphan `llama-server`

## M02: Inference crate `nexus-llm` · [spec](docs/milestones/M02-nexus-llm.md) · [contract](docs/architecture/INFERENCE-CONTRACT.md)
- [x] **T02.01** Types (`ChatMessage`, `ChatRequest`, `ResponseFormat`, `StreamEvent`, `Usage`, `Timings`, `ModelsResponse`) (2026-10-11)
- [x] **T02.02** SSE decoder (CRLF, partial lines, UTF-8 splits, `[DONE]`) + exhaustive tests (2026-10-11)
- [x] **T02.03** ThinkSplitter (`<think>` fallback, split tags) + tests (2026-10-11)
- [x] **T02.04** `LlmClient` (health, models w/ auth, tokenize, chat, chat_stream, error mapping) + axum mock-server tests (2026-10-11)
- [x] **T02.05** Port probe + token + `ServerProcess::spawn` (env_clear, LLAMA_API_KEY, process group, log ring + file) (2026-10-11)
- [x] **T02.06** Windows spawn specifics behind `cfg(windows)`: CREATE_NO_WINDOW, Job Object KILL_ON_JOB_CLOSE, env SystemRoot/TEMP (2026-10-11)
- [x] **T02.07** `Supervisor` (states, ready loop, ownership check, auto-restart once, graceful shutdown) + fake-binary tests (2026-10-11)
- [x] **T02.08** Stale-process reaping via `server.json` + exe-path check + test (2026-10-11)
- [x] **T02.09** `Scheduler` (Interactive/Background, bounded 8, cancel) + ordering/cancel tests (2026-10-11)
- [x] **T02.10** `structured::chat_json` (validate, reask once, Fallback) + matrix tests (2026-10-11)
- [x] **T02.11** Examples `chat.rs` / `json.rs` + `tests/real_model.rs` (#[ignore]) — written + compile-checked; ignored tests must be run on the Mac (2026-10-11)
- [ ] ⭐ **T02.12** 🍎 Exit check: `cargo test -p nexus-llm -- --ignored` passes; no orphans; timings in SPIKES.md — ⏳ Mac-only; owner to run

## M03: App shell and design system · [spec](docs/milestones/M03-app-shell.md)
- [x] **T03.01** Scaffold `apps/desktop` (Vite + React 19 + TS strict) + `src-tauri` (Tauri 2.12), add to workspace (2026-10-11)
- [x] **T03.02** Platform configs: `tauri.conf.json`, `tauri.macos.conf.json`, `tauri.windows.conf.json`, Info.plist, entitlements, capabilities, CSP (2026-10-11)
- [x] **T03.03** Tailwind v4 + `tokens.css` + globals + fonts (Inter, JetBrains Mono) + theme switcher (light/dark/system) (2026-10-11)
- [x] **T03.04** shadcn/ui init + primitives themed to tokens (UI-DESIGN §6) (2026-10-11)
- [x] **T03.05** AppState + setup (paths, DB open, supervisor start in background) + exit handling (shutdown on ExitRequested/Exit) (2026-10-11)
- [x] **T03.06** `runtime_paths.rs` (macOS externalBin, Windows resource runtime, manifest/index.json model resolution, NEXUS_MODEL_PATH) (2026-10-11)
- [x] **T03.07** 🔬 **S6** tauri-specta bindings (`app_info`, `runtime_*`, `settings_*`) → `src/bindings.ts` (fallback ts-rs per ADR-006) (2026-10-11)
- [x] **T03.08** Frontend `lib/transport/` (Transport interface + tauriTransport) + `lib/api.ts` facade + ESLint rule banning `invoke` in features (2026-10-11)
- [x] **T03.09** Shell UI: TitleBar (drag region), Sidebar nav (8 sections), placeholder pages, Inspector toggle (2026-10-11)
- [x] **T03.10** RuntimePill + popover (status, model, ctx, logs, restart) wired to `runtime:status` (2026-10-11)
- [x] **T03.11** macOS vibrancy sidebar + transparent window; Windows Mica + native decorations — macOS vibrancy done (config + CSS, untested off-Mac); Windows Mica deferred, see QUESTIONS Q-2 (2026-10-11)
- [x] **T03.12** Notifications plumbing: plugin, `notify()` service (DB + emit + OS when unfocused), bell popover, test notification (2026-10-11)
- [x] **T03.13** App icon (hexagon nexus) 1024px → `pnpm tauri icon` (2026-10-11)
- [ ] **T03.14** 🍎 Walking-skeleton `.app` (overlay config with default model) → move to /Applications → works offline → notification → no orphan — ⏳ Mac/Windows-only; owner to run
- [x] **T03.15** Write `scripts/fetch-runtime-windows.mjs` (GitHub release digest verification, extract, prune, lock) + mock test (2026-10-11)
- [ ] **T03.16** 🍎🪟🔬 **S7** cross-compile skeleton for Windows x64 (cargo-xwin + NSIS) on the Mac; run in a Win11 ARM VM / PC → SPIKES.md — ⏳ Mac/Windows-only; owner to run
- [ ] ⭐ **T03.17** Exit check: Ready < 5 s in dev; kill server → auto-restart; theme persists; skeletons OK on macOS + Windows — ⏳ Mac/Windows-only; owner to run

## M04: Chat MVP ★ · [spec](docs/milestones/M04-chat.md)
- [x] **T04.01** Store migration 0001 (settings, assistants, conversations, messages + FTS5 + triggers, attachments) + repos + tests (2026-10-11)
- [x] **T04.02** Service layer skeleton (`service/*`, `AppCtx`, `Caller`, `Sink` trait) + ChannelSink (2026-10-11)
- [x] **T04.03** `nexus-agent` minimal: preamble + `nexus.toml` rendering (minijinja) + insta snapshot (2026-10-11)
- [x] **T04.04** ContextBudget (tokenize + LRU cache, history window, reserve) + tests (2026-10-11)
- [x] **T04.05** TurnEngine chat workflow (stream → ChatEvent, think toggle, reasoning not re-sent) + mock-LLM tests (2026-10-11)
- [x] **T04.06** Commands: `chat_send` / `chat_stop` / `chat_regenerate` / `chat_edit_resend` with streaming persistence (500 ms) (2026-10-11)
- [x] **T04.07** Delta coalescing (16 ms / 32 chars) in Rust; Channel ordering test (2026-10-11)
- [x] **T04.08** Title job (`title.md` + schema + fallback) and history summary job (`history_summary.md`) (2026-10-11)
- [x] **T04.09** Conversations commands (list w/ FTS search, get, update, delete, export_md) (2026-10-11)
- [x] **T04.10** UI ConversationList (grouped by date, search, pin, rename, delete, export) (2026-10-11)
- [x] **T04.11** UI ChatView + MessageList (stick-to-bottom, "new messages" pill, virtualization > 200) (2026-10-11)
- [x] **T04.12** UI Markdown + CodeBlock (Shiki lazy singleton, copy) + ThoughtsBlock (2026-10-11)
- [x] **T04.13** UI Composer (auto-grow, Enter/Shift+Enter, Think toggle, ctx meter, Send/Stop) + `useChatStream` (rAF flush) (2026-10-11)
- [x] **T04.14** Empty state with assistant starters; error cards with actions; perf stats line (2026-10-11)
- [x] **T04.15** Text-file attachments (drop/pick, ≤ 256 KB, token cost chip) (2026-10-11)
- [ ] **T04.16** Shortcuts ⌘N ⌘. ⌘↑ ⌘⇧C (Ctrl on Windows) + focus management; responsive check at 390 px
- [ ] ⭐ **T04.17** 🍎 Exit check: TTFT < 1 s, 60 fps stream, stop < 200 ms, 50-turn context OK, restart persists, FTS works

## M05: Permissions and change sets · [spec](docs/milestones/M05-permissions.md) · [contract](docs/architecture/PERMISSIONS.md)
- [ ] **T05.01** Grants model + canonicalization (existing + non-existing paths, dunce on Windows) + tests
- [ ] **T05.02** Deny list (per OS, user entries) + check algorithm + exhaustive tests (boundaries, symlinks, expiry, max level)
- [ ] **T05.03** Store migration 0002 (grants, deny_rules, audit_log, changesets) + repos; load grants on startup
- [ ] **T05.04** Batched async audit writer + tests
- [ ] **T05.05** ChangeSet model + preview (diffs via `similar`, conflicts, missing grants, validation)
- [ ] **T05.06** Journal apply (atomic writes, backups, rollback on failure) + Trash (`trash` crate, small-file copies)
- [ ] **T05.07** Journal undo (conflict detection, restore) + proptest (≥ 256 cases, byte-identical)
- [ ] **T05.08** OS probes: macOS TCC EPERM detection + deep links; Windows Controlled Folder Access detection
- [ ] **T05.09** Permission broker (oneshot map, 5-min timeout, `permission:request`, OS notify when unfocused) + `permission_respond`
- [ ] **T05.10** Commands `fs_*`, `os_permissions_*`, `audit_list`, `changesets_*`
- [ ] **T05.11** UI Permissions page (grants table, macOS privacy cards, protected locations, activity log)
- [ ] **T05.12** UI Permission modal (Deny / Allow once / Allow folder ▾)
- [ ] **T05.13** UI Changes page (list, file list + stats, diff viewer unified/split, apply/undo/discard, conflict dialog)
- [ ] **T05.14** Debug command `debug_make_test_changeset` (dev only)
- [ ] ⭐ **T05.15** Exit check: PERMISSIONS §8 tests + proptest pass; TCC manual test; grep shows no writes outside Journal

## M06: Assistants, prompts, tools · [spec](docs/milestones/M06-assistants.md) · [contract](docs/architecture/ASSISTANTS.md)
- [ ] **T06.01** `AssistantSpec` (deny_unknown_fields) + schema validation + Registry (include_dir! built-ins + user from store)
- [ ] **T06.02** `prompt_lint` test for all 18 built-ins (tokens ≤ 450, schema, ids, starters, icons) and fix any failures
- [ ] **T06.03** Internal prompt loader (`prompts/internal/*.md`) + variable checks + snapshot tests
- [ ] **T06.04** Tool trait + `read_file`, `list_dir`, `search_files`, `grep_text`, `file_info`, `calculate`, `now` (budgeted outputs) + temp-dir tests
- [ ] **T06.05** Router workflow (heuristic gate, dynamic action schema, arg resolution, loop guard, max hops) + scripted mock tests
- [ ] **T06.06** Permission integration in tools (broker, denied → model told) + tests
- [ ] **T06.07** Store assistants repo + commands `assistants_*`, `assistant_validate` (field errors + token count)
- [ ] **T06.08** UI Assistants gallery (categories, search, cards, permission chips, pipeline cards disabled until ready)
- [ ] **T06.09** UI assistant switcher (⌘/) + header chip + "switched to" divider
- [ ] **T06.10** UI Builder sheet (form, icon/color pickers, prompt editor w/ live token count, params, tools, starters) + Test chat (draft spec)
- [ ] **T06.11** Import/export TOML + Prompt Architect "Open in builder" detection
- [ ] **T06.12** UI tool chips + status lines in chat
- [ ] ⭐ **T06.13** 🍎 Exit check: router eval 20 prompts ≥ 80% correct tool choice (SPIKES.md §router); denied path graceful

## M07: Model manager and downloads · [spec](docs/milestones/M07-models.md) · [contract](docs/architecture/MODEL-MANAGER.md)
- [ ] **T07.01** GGUF header parser (guards) + fixture writer + tests (+ ignored real-model test)
- [ ] **T07.02** Catalog loader + schema validation test (exactly one default)
- [ ] **T07.03** HF client (revision, tree w/ lfs, search, gated) + wiremock tests
- [ ] **T07.04** Downloader (Range resume, streaming SHA-256, retries, pause/cancel, manual redirects, token stripping) + axum tests
- [ ] **T07.05** RAM-fit estimator (sysinfo, Metal 70% rule) + tests
- [ ] **T07.06** Store migration 0003 (models, downloads, jobs) + registry
- [ ] **T07.07** Bundled models from `manifest.json` (1..n) + extra model folders scan + `models_verify_bundled`
- [ ] **T07.08** Commands `models_*`, one-at-a-time queue, activation → supervisor restart, `model:changed`, notifications
- [ ] **T07.09** UI Models page: runtime panel, Installed (Bundled / Downloaded / Imported+Discovered), Discover, Search HF
- [ ] **T07.10** UI model cards (RAM badge, capabilities, progress ring, pause/resume/cancel/use/delete) + detail drawer (GGUF meta)
- [ ] ⭐ **T07.11** 🍎 Exit check: pause → quit → resume → verify → use → chat; corrupted part → checksum error; offline states

## M08: Analyzer and reports · [spec](docs/milestones/M08-analyzer.md) · [contract](docs/architecture/ANALYZER.md)
- [ ] **T08.01** Scanner (ignore WalkBuilder, progress 10 Hz, cancel, warnings) + 100k-file benchmark
- [ ] **T08.02** Categories table (≥ 250 ext) + magic-byte fallback + junk rules + file naming checks + tests
- [ ] **T08.03** Duplicates (size → 64 KiB head → full BLAKE3, rayon) + tests
- [ ] **T08.04** Aggregates, age buckets, largest dirs, treemap pruning + facts builder (≤ 1 200 tokens) + tests
- [ ] **T08.05** Renderers: Markdown, JSON, CSV + tests
- [ ] **T08.06** 🔬 **S5** PDF extractors on your PDFs → SPIKES.md
- [ ] **T08.07** Extractors: plain/markdown/code, PDF (catch_unwind), DOCX, HTML + fixtures
- [ ] **T08.08** Chunker (700 tok, overlap, pages) + BM25 + tests
- [ ] **T08.09** File reports: code, text, pdf, docx, csv/tsv, json, logs, image (EXIF + GPS flag), zip
- [ ] **T08.10** Secrets scanner (rules + entropy + redaction) + tests with fake secrets
- [ ] **T08.11** Log parser + template clustering + timeline + tests
- [ ] **T08.12** CSV/JSON profiler (streaming, reservoir sampling) + tests
- [ ] **T08.13** Pipelines `folder_report` + narrative number validator
- [ ] **T08.14** Pipelines `doc_summarize` (map-reduce) + `doc_qa` (BM25 + citations) + `doc_search` tool
- [ ] **T08.15** Pipelines `tidy_plan`, `secrets_report`, `log_triage`, `data_profile`, `commit_message` + `scan_folder` tool
- [ ] **T08.16** Store migration 0004 (reports, notifications, watches) + commands `analyze_*`, `reports_*`, `report_narrative`, `cleanup_plan`
- [ ] **T08.17** UI Analyzer home + scan flow (pick folder → grant → progress → report)
- [ ] **T08.18** UI report view: KPI cards, canvas treemap (zoom, breadcrumb), category bars, virtualized tabs, AI summary card
- [ ] **T08.19** UI file report views per type + export menu + "Plan cleanup" → Changes
- [ ] **T08.20** Enable pipeline assistants in the gallery; "Open full report" links from chat
- [ ] ⭐ **T08.21** 🍎 Exit check: 100k files < 10 s; narrative numbers validated; DocLens cites correct pages 5/5; no raw secrets anywhere

## M09: Rename Forge · [spec](docs/milestones/M09-rename-forge.md) · [contract](docs/architecture/ANALYZER.md#5-rename-forge)
- [ ] **T09.01** 🔬 **S4** tree-sitter-gdscript 6.1 + tree-sitter 0.27 on your game (Godot 3 + 4 syntax) → SPIKES.md
- [ ] **T09.02** Fixture projects `tests/fixtures/godot3_game` + `godot4_game` (hand-written, with scenes, autoloads, signals, string calls)
- [ ] **T09.03** GDScript tags queries (`queries/gdscript/*.scm`) + S-expression snapshot tests
- [ ] **T09.04** Symbol index (defs, refs, scopes, extends chain, ambiguity) + serialization + incremental re-index
- [ ] **T09.05** Godot adapter: `.tscn/.tres` parser (connections, exported props, method tracks, embedded scripts), `project.godot` (autoloads, input actions)
- [ ] **T09.06** String-call detection (call, has_method, Callable, connect, emit_signal, rpc, set/get, tween_property) + tests
- [ ] **T09.07** ENGINE_VIRTUAL list + `scripts/gen-godot-builtins.mjs` → `builtin_methods.txt` + `globals.txt`
- [ ] **T09.08** Naming rules engine (all rules §5.5) + conventions per kind/language + tests
- [ ] **T09.09** Proposal pipeline (batching ≤ 20, `<sym>` blocks, pattern schema, validator, reask, needs_manual) + mock tests
- [ ] **T09.10** Planner → ChangeSet (byte ranges, scene/string refs, overlaps, collisions) + git-dirty detection + tests
- [ ] **T09.11** Verify with Godot (headless check-only per script + `--import`), streamed output
- [ ] **T09.12** Strings mode (extract, propose, placeholder validation, translation CSV, optional `tr()` change set)
- [ ] **T09.13** Godot 3→4 modernization hints panel (flag only + converter recommendation)
- [ ] **T09.14** Store migration 0005 + commands `rename_*` + jobs + notifications
- [ ] **T09.15** UI stepper + issues grid (virtualized, filters, inline edit w/ live validation, bulk accept, re-propose)
- [ ] **T09.16** UI occurrence drawer (code previews) + Plan & preview (Changes) + git banner + Verify panel + Strings tab
- [ ] **T09.17** 🍎 Real run on a COPY of your game → fix issues → notes in SPIKES.md §rename-real-run
- [ ] **T09.18** (P1) Adapters: Rust, TS/JS, Python, C# (Unity messages), Lua (LÖVE callbacks)
- [ ] ⭐ **T09.19** Exit check: fixtures pass, virtuals never proposed, scene refs updated, undo byte-identical, game playable

## M10: Code editor · [spec](docs/milestones/M10-editor.md)
- [ ] **T10.01** Editor shell: file tree (granted roots, lazy, ignore toggle, icons) + tabs + CodeMirror (EditorView per tab)
- [ ] **T10.02** Theme from tokens + language packs + legacy modes + custom GDScript StreamLanguage (+ token snapshot test)
- [ ] **T10.03** Save via `editor_save` → change set (confirm popover, session "always allow"), read-only mode without write grant
- [ ] **T10.04** AI actions panel (Explain, Improve names, Add docs, Find bugs, Simplify, Ask) + `editor_rewrite.md`
- [ ] **T10.05** Merge view with per-chunk accept/reject + suspicious-rewrite guard
- [ ] **T10.06** External change detection + large-file limits + shortcuts (⌘O ⌘S ⌘W ⌘⇧T)
- [ ] ⭐ **T10.07** Exit check: demo flow; 10k-line file smooth; no save bypasses change sets

## M11: Notifications, jobs, watches · [spec](docs/milestones/M11-notifications-jobs.md)
- [ ] **T11.01** JobRegistry (persist, events 4 Hz, cancel, startup recovery) + tests
- [ ] **T11.02** Migrate scan/download/propose/narrative to jobs
- [ ] **T11.03** UI Jobs indicator + popover (progress, cancel, open result)
- [ ] **T11.04** Notification preferences per kind + routing (click → route) + tests
- [ ] **T11.05** Watches backend (scheduler, `notify` on-change debounce, `diff_reports`, thresholds) + tests
- [ ] **T11.06** UI "Watch this folder" + watches list in Analyzer
- [ ] **T11.07** (optional) Tray icon + launch at login (autostart plugin)
- [ ] ⭐ **T11.08** 🍎 Exit check: 3 parallel jobs + cancel; OS notifications in background; watch delta notification

## M12: Bundling, profiles, App Builder (macOS) · [spec](docs/milestones/M12-bundling-app-builder.md) · [guide](guide.html)
- [ ] **T12.01** Validate `bundles/*.json` against `schemas/bundle-profile.schema.json` in a test (node or Rust)
- [ ] **T12.02** 🍎 Harden `fetch-model.mjs` multi-model (`--profile`, `--repo`, `--local`, `--verify --all`) on the Mac
- [ ] **T12.03** `scripts/stage-bundle.mjs` (hardlinks, manifest.json, licenses, generated overlay) + test with fake 10 MB models
- [ ] **T12.04** `scripts/verify-payload.mjs` (re-hash models + runtime, lipo/otool, Info.plist keys; `--windows` PE arch check)
- [ ] **T12.05** `scripts/build-app.sh --profile <p>` (doctor → verify → stage → build:web → tauri build → verify → codesign → zip + SHA256SUMS)
- [ ] **T12.06** `scripts/app-builder.mjs` server (127.0.0.1 + token, profile CRUD, size calc, build runner, SSE logs, idle exit, CLI mode)
- [ ] **T12.07** `scripts/app-builder/index.html` UI (models checklist, HF/local add, default radio, targets, Windows format, size summary, results)
- [ ] **T12.08** `scripts/make-model-pack.mjs` + `schemas/model-pack.schema.json` usage + test
- [ ] **T12.09** In-app model packs: export + import (verify, copy/reference, progress) + `models-extra/` first-launch import
- [ ] **T12.10** Settings → About shows `bundle.json` (profile, commit, date)
- [ ] **T12.11** Update `guide.html` macOS sections from "planned" to "available"
- [ ] ⭐ **T12.12** 🍎 Exit check: App Builder 3-model build → works offline on a 2nd Mac; tamper test fails verify; model pack import offline

## M13: Windows build, cross-compiled on the Mac (MUST) · [spec](docs/milestones/M13-windows.md)
- [ ] **T13.01** `scripts/doctor.sh --windows` checks (targets, cargo-xwin, makensis, clang-cl, lld-link, XWIN_CACHE_DIR)
- [ ] **T13.02** 🍎 Harden `fetch-runtime-windows.mjs` (x64 vulkan/cpu, arm64) on the Mac; locks recorded
- [ ] **T13.03** Rust Windows specifics complete: paths (dunce), Recycle Bin, CFA detection, Windows deny list, Mica, Ctrl shortcuts
- [ ] **T13.04** `windows/hooks.nsh` (offline kit post-install copy) + NSIS config (currentUser, compression none, WebView2 mode)
- [ ] **T13.05** `scripts/build-windows.sh` installer format (x64 + arm64)
- [ ] **T13.06** Portable format + `portable.flag` → data in `.\data\` (AppPaths portable mode)
- [ ] **T13.07** Offline kit format (basic installer + `models-extra/` + WebView2 offline installer + INSTALL.txt), auto rule > 1.8 GB
- [ ] **T13.08** App Builder integration (Windows targets + formats) + SHA256SUMS
- [ ] **T13.09** Write `docs/WINDOWS-TESTING.md` checklist
- [ ] **T13.10** 🪟🔬 **S8** Vulkan GPU on the studio PC; CPU fallback without vulkan-1.dll → SPIKES.md
- [ ] **T13.11** 🪟 Run the checklist on the Win11 ARM VM (arm64 + x64 emulation) and the x64 PC (offline)
- [ ] **T13.12** Update `guide.html` Windows sections to "available"
- [ ] ⭐ **T13.13** Exit check: offline install works on x64; Task Manager kill → no orphan; no console flash; 3-model kit verified

## M14: LAN Share + mobile web UI · [spec](docs/milestones/M14-lan-share.md) · [contract](docs/architecture/LAN-SHARE.md)
- [ ] **T14.01** Audit: every command delegates to `service::*` with `Caller`; no `invoke` in feature code
- [ ] **T14.02** `nexus-share` auth: pairing secrets (single-use, 5 min), PIN + rate limit/lockout, device sessions (hash-only), CSRF + tests
- [ ] **T14.03** WebSocket RPC (protocol, allow-list, streaming, cancel, push events) + policy guard + tests
- [ ] **T14.04** Static web UI embedding (`dist-web` via include_dir!, SPA fallback, security headers, cache rules)
- [ ] **T14.05** Server lifecycle: ShareController start/stop/restart, port 9889 default, `port_in_use` + next-free suggestion, graceful stop + tests
- [ ] **T14.06** Store migration 0006 (`share_devices`, `conversations.origin`) + commands `share_*` + `share:status`
- [ ] **T14.07** `ShareApi` impl → `service::*` with `Caller::Remote`; remote grant requests → desktop modal "from <device>"
- [ ] **T14.08** mDNS (`nanonexus.local`) + Info.plist `NSLocalNetworkUsageDescription` / `NSBonjourServices`
- [ ] **T14.09** Optional HTTPS (rcgen self-signed, SAN = LAN IPs + .local, fingerprint)
- [ ] **T14.10** Frontend `wsTransport` (reconnect, resubscribe, visibilitychange refetch) + `pnpm build:web` (`dist-web`)
- [ ] **T14.11** Mobile layout (< 768 px): bottom tabs, drawer, safe areas, 44 px targets, 16 px inputs, long-press menus
- [ ] **T14.12** Pairing screen (QR fragment + PIN pad) + PWA manifest/icons + minimal service worker + desktop-only route guards
- [ ] **T14.13** Desktop 📡 Share pill + popover (switch, URLs, QR SVG, PIN, devices)
- [ ] **T14.14** Settings → Sharing page (all LAN-SHARE §2 settings + devices table) + pairing notifications + tray item
- [ ] **T14.15** Playwright e2e (390×844 + 1280×800): pair → chat → stream → stop → history → revoke
- [ ] **T14.16** Update `guide.html` Sharing section (firewall prompts, troubleshooting) + WINDOWS-TESTING firewall step
- [ ] ⭐ **T14.17** Exit check: real phone demo; security tests; `lsof` shows only :9889 exposed; works on the Windows build

## M15: Polish · [spec](docs/milestones/M15-polish.md)
- [ ] **T15.01** Command palette (⌘K / Ctrl+K) with all groups
- [ ] **T15.02** Onboarding (4 steps, animated hexagon), re-runnable
- [ ] **T15.03** (optional) Quick Ask window (⌥Space / Alt+Space)
- [ ] **T15.04** Shortcuts complete + Settings → Shortcuts viewer
- [ ] **T15.05** Accessibility pass (contrast script over tokens, focus order, labels, reduced motion)
- [ ] **T15.06** Performance budgets (cold start, idle RAM, 60 fps, scan speed) → SPIKES.md §perf
- [ ] **T15.07** `scripts/eval-assistants.sh` + golden transcripts for all assistants → pass table
- [ ] **T15.08** (optional) Precompiled Metal shaders comparison
- [ ] **T15.09** Empty/error/loading states everywhere
- [ ] **T15.10** Visual QA: screenshots of every screen, light/dark, macOS/Windows/mobile → fix list done
- [ ] ⭐ **T15.11** Exit check: budgets met; keyboard-only walkthrough; eval ≥ 80% per assistant

## M16: Optional · [spec](docs/milestones/M16-optional.md)
- [ ] **T16.01** Linux build (in a VM)
- [ ] **T16.02** Custom OpenAI-compatible endpoint
- [ ] **T16.03** Embeddings RAG (hybrid BM25 + vectors)
- [ ] **T16.04** Vision (MiniCPM-V + mmproj)
- [ ] **T16.05** Voice input (whisper.cpp sidecar)
- [ ] **T16.06** macOS notarization
- [ ] **T16.07** Windows code signing from the Mac (osslsigncode)
- [ ] **T16.08** 🔬 S9 cross-compile llama.cpp for Windows on the Mac
- [ ] **T16.09** Speculative decoding with a draft model

## M17: Knowledge libraries (offline reference books) · [spec](docs/milestones/M17-knowledge.md)
- [ ] **T17.01** `nexus-knowledge` crate + migration `0007_knowledge.sql` (libraries, documents, chunks, FTS5/BM25) + repos + tests
- [ ] **T17.02** Extractors: md, txt, json, html (readable text), csv, docx, source files (reuse M08 extractors where present) + fixtures
- [ ] **T17.03** PDF (page-aware) and EPUB (spine order, chapter titles) extractors + fixtures; skipped-content notes
- [ ] **T17.04** Heading-aware chunker (600-800 tokens, overlap, breadcrumbs, page numbers) + tests
- [ ] **T17.05** Index job (Background, incremental by hash, progress, cancel, size/time limits) + tests
- [ ] **T17.06** `search()` BM25 + heading weights + diversity + neighbor expansion + 500 MB perf fixture test
- [ ] **T17.07** `kb_search` tool + Librarian assistant + chat context injection (`<doc source=... page=...>`, budgeted) + verified `[n]` citations + "not in the library" rule
- [ ] **T17.08** Prompt-injection and no-regression tests (libraries off = identical prompts/snapshots)
- [ ] **T17.09** Commands `kb_*` (list, add, remove, enable, reindex, search, passage) + job events
- [ ] **T17.10** UI: Library page (add via drag-drop/picker, progress, enable, re-index, remove, passage search) + composer "Libraries" chip + clickable citations
- [ ] **T17.11** Bundling: profile `knowledge[]`, stage-bundle copies sources + prebuilt index, `verify-payload` hashes, App Builder size summary
- [ ] **T17.12** Docs: user guide section, sample library, walkthrough (e.g. a Reaper manual, a JavaScript book)
- [ ] ⭐ **T17.13** 🍎 Exit check: offline cited answers from a bundled library; user-added EPUB/PDF/MD work; libraries off = no behavior change
