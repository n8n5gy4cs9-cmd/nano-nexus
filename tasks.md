# NanoNexus: Tasks

> **How to use:** work top to bottom. `[ ]` = to do, `[x]` = done. When you finish a task: tick it,
> add `(YYYY-MM-DD)` and a short note if anything deviated, then **commit and push to `main`**
> (`git commit -m "T04.07: …" && git push origin main`). Spec for each milestone: `docs/milestones/`.
> Run `scripts/progress.sh` for a summary. `scripts/progress.sh --write` refreshes the block below.

<!-- progress:start -->
**Progress:** 183/234 tasks done (78%) · **Next:** T00.08 🍎 Run `scripts/doctor.sh` and `scripts/check.sh` on the Mac; record the environment in `docs/SPIKES.md` — ⏳ Mac-only; owner to run (skipped in the Linux container)
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
- [x] **TP.07** 17 built-in assistants with researched system prompts (`assistants/`) (2026-10-11)
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
- [x] **T04.16** Shortcuts ⌘N ⌘. ⌘↑ ⌘⇧C (Ctrl on Windows) + focus management; responsive check at 390 px (2026-10-11)
- [ ] ⭐ **T04.17** 🍎 Exit check: TTFT < 1 s, 60 fps stream, stop < 200 ms, 50-turn context OK, restart persists, FTS works — ⏳ Mac-only; owner to run

## M05: Permissions and change sets · [spec](docs/milestones/M05-permissions.md) · [contract](docs/architecture/PERMISSIONS.md)
- [x] **T05.01** Grants model + canonicalization (existing + non-existing paths, dunce on Windows) + tests (2026-10-11)
- [x] **T05.02** Deny list (per OS, user entries) + check algorithm + exhaustive tests (boundaries, symlinks, expiry, max level) (2026-10-11)
- [x] **T05.03** Store migration 0002 (grants, deny_rules, audit_log, changesets) + repos; load grants on startup (2026-10-11)
- [x] **T05.04** Batched async audit writer + tests (2026-10-11)
- [x] **T05.05** ChangeSet model + preview (diffs via `similar`, conflicts, missing grants, validation) (2026-10-11)
- [x] **T05.06** Journal apply (atomic writes, backups, rollback on failure) + Trash (`trash` crate, small-file copies) (2026-10-11)
- [x] **T05.07** Journal undo (conflict detection, restore) + proptest (≥ 256 cases, byte-identical) (2026-10-11)
- [x] **T05.08** OS probes: macOS TCC EPERM detection + deep links; Windows Controlled Folder Access detection (2026-10-11)
- [x] **T05.09** Permission broker (oneshot map, 5-min timeout, `permission:request`, OS notify when unfocused) + `permission_respond` (2026-10-11)
- [x] **T05.10** Commands `fs_*`, `os_permissions_*`, `audit_list`, `changesets_*` (2026-10-11)
- [x] **T05.11** UI Permissions page (grants table, macOS privacy cards, protected locations, activity log) (2026-10-11)
- [x] **T05.12** UI Permission modal (Deny / Allow once / Allow folder ▾) (2026-10-11)
- [x] **T05.13** UI Changes page (list, file list + stats, diff viewer unified/split, apply/undo/discard, conflict dialog) (2026-10-11)
- [x] **T05.14** Debug command `debug_make_test_changeset` (dev only) (2026-10-11) — returns an error in release builds; fixtures in the OS temp folder
- [ ] ⭐ **T05.15** Exit check: PERMISSIONS §8 tests + proptest pass; TCC manual test; grep shows no writes outside Journal — ⏳ grep done (only the dev-only T05.14 fixtures write, in the OS temp dir); `cargo test` + TCC manual test need the Mac; owner to run. (`bindings.ts` regenerated 2026-10-11)

## M06: Assistants, prompts, tools · [spec](docs/milestones/M06-assistants.md) · [contract](docs/architecture/ASSISTANTS.md)
- [x] **T06.01** `AssistantSpec` (deny_unknown_fields) + schema validation + Registry (include_dir! built-ins + user from store) (2026-10-11)
- [x] **T06.02** `prompt_lint` test for all 17 built-ins (tokens ≤ 450, schema, ids, starters, icons) and fix any failures (2026-10-11) — Godot Sidekick dropped (owner: no Godot anywhere; Rename Forge is language-agnostic, see M09)
- [x] **T06.03** Internal prompt loader (`prompts/internal/*.md`) + variable checks + snapshot tests (2026-10-11)
- [x] **T06.04** Tool trait + `read_file`, `list_dir`, `search_files`, `grep_text`, `file_info`, `calculate`, `now` (budgeted outputs) + temp-dir tests (2026-10-11)
- [x] **T06.05** Router workflow (heuristic gate, dynamic action schema, arg resolution, loop guard, max hops) + scripted mock tests (2026-10-11)
- [x] **T06.06** Permission integration in tools (broker, denied → model told) + tests (2026-10-11) — `PermissionAsker` trait in nexus-agent; the app's broker-backed impl lands with the T06.07 service wiring
- [x] **T06.07** Store assistants repo + commands `assistants_*`, `assistant_validate` (field errors + token count) (2026-10-11) — also: registry in AppState, chat resolves assistants from it (+ `draft_spec`), router wired into chat turns with the broker-backed asker; bindings regenerated
- [x] **T06.08** UI Assistants gallery (categories, search, cards, permission chips, pipeline cards disabled until ready) (2026-10-11)
- [x] **T06.09** UI assistant switcher (⌘/) + header chip + "switched to" divider (2026-10-11)
- [x] **T06.10** UI Builder sheet (form, icon/color pickers, prompt editor w/ live token count, params, tools, starters) + Test chat (draft spec) (2026-10-11)
- [x] **T06.11** Import/export TOML + Prompt Architect "Open in builder" detection (2026-10-11) — export = clipboard + webview download (the app never writes to arbitrary paths); new command `assistant_draft`
- [x] **T06.12** UI tool chips + status lines in chat (2026-10-11)
- [ ] ⭐ **T06.13** 🍎 Exit check: router eval 20 prompts ≥ 80% correct tool choice (SPIKES.md §router); denied path graceful

## M07: Model manager and downloads · [spec](docs/milestones/M07-models.md) · [contract](docs/architecture/MODEL-MANAGER.md)
- [x] **T07.01** GGUF header parser (guards) + fixture writer + tests (+ ignored real-model test) (2026-10-11)
- [x] **T07.02** Catalog loader + schema validation test (exactly one default) (2026-10-11)
- [x] **T07.03** HF client (revision, tree w/ lfs, search, gated) + wiremock tests (2026-10-11)
- [x] **T07.04** Downloader (Range resume, streaming SHA-256, retries, pause/cancel, manual redirects, token stripping) + axum tests (2026-10-11)
- [x] **T07.05** RAM-fit estimator (sysinfo, Metal 70% rule) + tests (2026-10-11)
- [x] **T07.06** Store migration 0003 (models, downloads, jobs) + registry (2026-10-11)
- [x] **T07.07** Bundled models from `manifest.json` (1..n) + extra model folders scan + `models_verify_bundled` (2026-10-11)
- [x] **T07.08** Commands `models_*`, one-at-a-time queue, activation → supervisor restart, `model:changed`, notifications (2026-10-11) — HF token via `HF_TOKEN` env only (Q-4); split GGUFs refused (Q-5)
- [x] **T07.09** UI Models page: runtime panel, Installed (Bundled / Downloaded / Imported+Discovered), Discover, Search HF (2026-10-11)
- [x] **T07.10** UI model cards (RAM badge, capabilities, progress ring, pause/resume/cancel/use/delete) + detail drawer (GGUF meta) (2026-10-11)
- [ ] ⭐ **T07.11** 🍎 Exit check: pause → quit → resume → verify → use → chat; corrupted part → checksum error; offline states

## M08: Analyzer and reports · [spec](docs/milestones/M08-analyzer.md) · [contract](docs/architecture/ANALYZER.md)
- [x] **T08.01** Scanner (ignore WalkBuilder, progress 10 Hz, cancel, warnings) + 100k-file benchmark (2026-10-11) — benchmark is `#[ignore]`: 100k files scan well under 10 s on this Linux box (release)
- [x] **T08.02** Categories table (≥ 250 ext) + magic-byte fallback + junk rules + file naming checks + tests (2026-10-11)
- [x] **T08.03** Duplicates (size → 64 KiB head → full BLAKE3, rayon) + tests (2026-10-11)
- [x] **T08.04** Aggregates, age buckets, largest dirs, treemap pruning + facts builder (≤ 1 200 tokens) + tests (2026-10-11)
- [x] **T08.05** Renderers: Markdown, JSON, CSV + tests (2026-10-11)
- [ ] **T08.06** 🔬 **S5** PDF extractors on your PDFs → SPIKES.md — ⏳ owner to run on your own PDFs; `pdf-extract` + `pdftotext` fallback are wired and tested on generated PDFs
- [x] **T08.07** Extractors: plain/markdown/code, PDF (catch_unwind), DOCX, HTML + fixtures (2026-10-11)
- [x] **T08.08** Chunker (700 tok, overlap, pages) + BM25 + tests (2026-10-11)
- [x] **T08.09** File reports: code, text, pdf, docx, csv/tsv, json, logs, image (EXIF + GPS flag), zip (2026-10-11) — code facts are pattern-based for now; M09's tree-sitter index will sharpen them
- [x] **T08.10** Secrets scanner (rules + entropy + redaction) + tests with fake secrets (2026-10-11)
- [x] **T08.11** Log parser + template clustering + timeline + tests (2026-10-11)
- [x] **T08.12** CSV/JSON profiler (streaming, reservoir sampling) + tests (2026-10-11)
- [x] **T08.13** Pipelines `folder_report` + narrative number validator (2026-10-11)
- [x] **T08.14** Pipelines `doc_summarize` (map-reduce) + `doc_qa` (BM25 + citations) + `doc_search` tool (2026-10-11)
- [x] **T08.15** Pipelines `tidy_plan`, `secrets_report`, `log_triage`, `data_profile`, `commit_message` + `scan_folder` tool (2026-10-11) — pipelines return a `StreamPlan` + artifact; app wiring is T08.16/T08.20
- [x] **T08.16** Store migration 0004 (reports, watches) + commands `analyze_folder`, `reports_*`, `report_*`, `report_cleanup_plan`, `file_report`; chat runs pipelines (2026-10-11) — untested by owner request (no build run); narrative saved via `report_narrative_save`, not a separate streaming command; bindings.ts needs regeneration
- [x] **T08.17** UI Analyzer home + scan flow (pick folder → grant → progress → report) (2026-10-11) — written without running a build/tests (owner request); regenerate bindings.ts and run `scripts/check.sh` before relying on it
- [x] **T08.18** UI report view: KPI cards, canvas treemap (zoom, breadcrumb), category bars, virtualized tabs, AI summary card (2026-10-11) — written without running a build/tests (owner request); regenerate bindings.ts and run `scripts/check.sh` before relying on it
- [x] **T08.19** UI file report views per type + export menu + "Plan cleanup" → Changes (2026-10-11) — written without running a build/tests (owner request); regenerate bindings.ts and run `scripts/check.sh` before relying on it
- [x] **T08.20** Enable pipeline assistants in the gallery; "Open full report" links from chat (2026-10-11) — written without running a build/tests (owner request); regenerate bindings.ts and run `scripts/check.sh` before relying on it
- [ ] ⭐ **T08.21** 🍎 Exit check: 100k files < 10 s; narrative numbers validated; DocLens cites correct pages 5/5; no raw secrets anywhere

## M09: Rename Forge · [spec](docs/milestones/M09-rename-forge.md) · [contract](docs/architecture/ANALYZER.md#5-rename-forge)
- [x] **T09.01** 🔬 **S4** tree-sitter 0.27 + grammar pins (python, javascript, typescript/tsx, rust, c-sharp, go, java) parse your own projects → SPIKES.md (2026-10-11) — pins load and parse samples in the container; parsing your own projects happens in T09.17
- [x] **T09.02** Fixture projects `tests/fixtures/{py_app,ts_app,rust_app,cs_app,go_app,java_app}` (hand-written: cross-file refs, imports, string lookups, config files, nested folders) (2026-10-11) — six fixtures under `tests/fixtures/` (go, java, rust, python verified to compile/compile-all in the container; ts/cs not checked: no tsc project deps / dotnet)
- [x] **T09.03** Tags queries per language (`queries/<lang>/tags.scm`) + S-expression snapshot tests (2026-10-11) — queries in `crates/nexus-analyze/queries/<lang>/tags.scm` (typescript shared with tsx); runner `code::tags`; 13 insta snapshots in `crates/nexus-analyze/tests/snapshots`. Kind refinement (method vs function, constant vs variable, TS parameter properties) is done by the index (T09.04)
- [x] **T09.04** Symbol index (defs, refs, scopes, imports, file/folder symbols, ambiguity) + serialization + incremental re-index (2026-10-11) — `code::index` (`build_index`, postcard `save`/`load`, hash-incremental), `code::file_facts`; refs are resolved by name + scope; import-graph resolution comes in T09.05 (cross-file refs currently use imported names / package / namespace)
- [x] **T09.05** Import/module resolution per language (py, ts/js incl. tsconfig paths, rust mod/use, c# using, go packages, java packages) + tests (2026-10-11) — `code::imports`: py (relative/absolute, packages), ts/js (relative, `tsconfig` paths/baseUrl, index files, written extensions), rust (`mod`/`use` crate/self/super), go (go.mod module path → folders), java (package/import → folders/files); C# has no file mapping (namespaces). File/folder symbols get Import refs per specifier segment plus Code refs for module aliases
- [x] **T09.06** Non-code refs: config/manifest scanner (package.json, tsconfig, Cargo.toml, pyproject, *.csproj, go.mod, YAML/JSON/TOML), string-lookup detection, opt-in docs/comments + tests (2026-10-11) — `code::noncode`: config/manifest path+qualified-name scan (segment-wise, quoted single words), string lookups (recognised calls auto, other equal strings = possible/ambiguous), docs and comments stored as `Doc`/`Comment` refs (the planner includes them only when opted in, T09.10). `Location.file >= OTHER_BASE` = config/doc file, use `CodeIndex::path_of`
- [x] **T09.07** Reserved-name lists per language (keywords, builtins, entry points, dunders, tests, overrides) + `PUBLIC_API`/`OVERRIDE` flags + per-project ignore list (2026-10-11) — `code::reserved`: keyword/builtin lists in `queries/<lang>/{keywords,globals}.txt`, entry points, dunders, tests (attrs/names/paths), overrides (syntax, special methods, project inheritance), ignore list (`IndexOptions.ignore`: names with `*`, `path:<prefix>`), `DYNAMIC_REFS`; flagged `RESERVED` symbols are never proposed
- [x] **T09.08** Naming rules engine (all rules §5.5, symbols + files + folders) + conventions per kind/language + tests (2026-10-11) — `code::rules`: conventions per language/kind, all §5.5 rules except `typo` (optional, not built), `Case::{matches,convert,pattern}` (heck), `detect()` -> candidates with a deterministic `suggested` name, project-style detection for JS/TS file names. `heck` pinned in the workspace for case conversion
- [x] **T09.09** Proposal pipeline (batching ≤ 20, `<sym>` blocks incl. files/folders, pattern schema, validator, reask, needs_manual) + mock tests (2026-10-11) — `nexus-analyze::code::propose` (`<sym>` blocks, `Validator`) + `nexus-agent::pipelines::rename::propose` (case-only issues fixed by rule; batches ≤ 20 per language/kind/convention with the convention regex in the schema; one reask with the validator's words; fallback = deterministic suggestion or `needs_manual`; "Try again" = temp +0.2 and rejected names). Scripted-model tests
- [x] **T09.10** Planner → ChangeSet (byte ranges + file/folder moves, import/config/string refs, overlaps, collisions, case-only renames via temp name) + git-dirty detection + tests (2026-10-11) — `code::plan::plan_renames`: validator-gated items → one ChangeSet (file moves, then folder moves deepest-first from old paths, then byte-range edits at final paths; case-only via `.nexus-rename-tmp-N`); public API / ambiguous / doc / comment refs opt-in; stale-hash error; existing-target check; `git_dirty` via fixed-arg `git status --porcelain`. Not yet checked against Journal::apply in an integration test
- [x] **T09.11** Find: symbol + usages search, project overview, router tool `find_symbol` + tests (2026-10-11) — `code::find` (`find_symbols` exact/prefix/substring/fuzzy with usage counts by kind, `usages`, `overview`, `render_find`) + `nexus-agent` `FindSymbol` tool over a `CodeSource` trait on `ToolCx`. Not wired in the app crate yet (comes with T09.14: implement `CodeSource` over the stored index, add `find_symbol` to a router assistant); the naming-issue summary of the overview is left to the UI task
- [x] **T09.12** Verify with the project's own checker (cargo check, tsc, compileall, dotnet build, go build, mvn/gradle), fixed args, streamed output (2026-10-11) — `code::verify`: `detect` (cargo, tsc, python compileall, dotnet, go, mvn, gradle; root manifests only) and async `run` (fixed argv, no shell, merged stdout/stderr streamed per line, cancel token, 15 min timeout, first 10 error lines, `missing` when the binary is not installed). Uses the workspace `tokio` (already pinned) as a direct dependency of nexus-analyze
- [x] **T09.13** Strings mode (extract, propose, placeholder validation, CSV/JSON export, optional `t("KEY")` change set) (2026-10-11) — `code::strings` (UI-position extraction by argument/call name, log calls and f-strings excluded, placeholder extraction + exact-multiset validation, max_len, CONSTANT_CASE unique keys, fallback key, CSV/JSON export, `t("KEY")` rewrite ChangeSet incl. JSX `attr={t(..)}`) + `nexus-agent::pipelines::strings::propose` (batches ≤ 20, one reask, fallback). Fixture `tests/fixtures/strings_app`. Template-literal and C#/Rust-raw strings are skipped
- [x] **T09.14** Store migration 0005 + commands `rename_*` + jobs + notifications (2026-10-11) — migration is `0007_rename.sql` (0005/0006 were taken by share/knowledge); `nexus-store::rename` (sessions + items, tested). App crate: `service/rename.rs` + `commands/rename.rs` (session_create, sessions, session_delete, items, propose, item_update w/ live validation, plan → Changes, checkers, verify, usages, find, cancel; "N rename proposals ready" notification), registered in `bindings.rs`. **Written; not compiled or tested (no GTK in the sandbox)**. Jobs are tracked in `RenameRuntime` (cancel by job id), not yet in the `jobs` table (M11 JobRegistry). Not done: strings commands, marking items `applied` after the change set is applied, `CodeSource` for `find_symbol`
- [x] **T09.15** UI stepper + issues grid (Symbols / Files & folders tabs, virtualized, filters, inline edit w/ live validation, bulk accept, re-propose) (2026-10-11) — `features/rename/RenamePage.tsx` + `ItemsTable`: start view (pick folder with progress/stop, recent projects), per-project view with Suggest names (job, progress, stop), filter, inline edit with server-side validation errors, accept/reject/try again, per-item usages, Names and Files-and-folders tabs, 200-row paging instead of virtualization. **Frontend not linted/typechecked/tested**
- [x] **T09.16** UI occurrence drawer (code previews) + Plan & preview (Changes) + git banner + Verify panel + Strings tab + Find tab (2026-10-11) — `UsagesList` (occurrence drawer as an inline expander), plan options + Preview → Changes page with git-dirty banner and skipped list, `VerifyPanel` (checker buttons, streamed output, stop), `FindPanel`. **Not done: Strings tab (no strings commands exist yet).** Also fixed channel arg names (`on_event`) in the rename commands and mark items `applied`/`accepted` on change set apply/undo. **Not compiled or tested**
- [ ] **T09.17** 🍎 Real run on a COPY of your own project → fix issues → notes in SPIKES.md §rename-real-run
- [x] **T09.18** (P1) More languages: Kotlin, Swift, PHP, Ruby, C/C++, Lua (one tags query + reserved list + fixture each) (2026-10-12) — grammars pinned in the workspace (`tree-sitter-kotlin-ng/swift/php/ruby/c/cpp/lua`); `Lang::{Kotlin,Swift,Php,Ruby,C,Cpp,Lua}` with tags, keywords/globals, conventions, include/require/`require()` resolution (file renames rewrite them), fixtures `kt_app` … `lua_app`, `tests/code_langs.rs`. The Kotlin grammar (kotlin-ng) mis-parses `object`/`companion object`; those declarations are skipped, not mis-renamed
- [ ] ⭐ **T09.19** Exit check: fixtures pass, reserved names never proposed, folder renames update imports, undo byte-identical, project still builds — ⏳ automated part done 2026-10-12 (`tests/code_exit.rs`: every suggested rename applied to a copy of each fixture, then the project's own checker: python, go, javac, cargo, tsc, ruby, php, gcc/g++; found and fixed a Go package-clause bug and a Java type/file pairing bug). Remaining: real project copy (T09.17, owner on the Mac); C#/Kotlin/Swift/Lua checkers not installed here

## M10: Code editor · [spec](docs/milestones/M10-editor.md)
- [x] **T10.01** Editor shell: file tree (granted roots, lazy, ignore toggle, icons) + tabs + CodeMirror (EditorView per tab) (2026-10-11) — written; not compiled or tested (session was code-only)
- [x] **T10.02** Theme from tokens + language packs + legacy modes (+ token snapshot test) (2026-10-11) — written; not compiled or tested (session was code-only) — no token snapshot test; mapping tests only
- [x] **T10.03** Save via `editor_save` → change set (confirm popover, session "always allow"), read-only mode without write grant (2026-10-11) — written; not compiled or tested (session was code-only) — files over 2 MB are refused (not opened read-only), per `fs_read_text`
- [x] **T10.04** AI actions panel (Explain, Improve names, Add docs, Find bugs, Simplify, Ask) + `editor_rewrite.md` (2026-10-11) — written; not compiled or tested (session was code-only)
- [x] **T10.05** Merge view with per-chunk accept/reject + suspicious-rewrite guard (2026-10-11) — written; not compiled or tested (session was code-only)
- [x] **T10.06** External change detection + large-file limits + shortcuts (⌘O ⌘S ⌘W ⌘⇧T) (2026-10-11) — written; not compiled or tested (session was code-only)
- [ ] ⭐ **T10.07** Exit check: demo flow; 10k-line file smooth; no save bypasses change sets

## M11: Notifications, jobs, watches · [spec](docs/milestones/M11-notifications-jobs.md)
- [x] **T11.01** JobRegistry (persist, events 4 Hz, cancel, startup recovery) + tests (2026-10-11) — `nexus-store::jobs::JobRegistry` (+ `Store` job CRUD, `recover_interrupted_jobs`, `prune_jobs`): start/progress/finish/fail/cancelled/cancel, per-job 250 ms throttle (terminal updates always go out), cancel token per job, emit callback instead of Tauri so it is testable. Wiring into `AppCtx`, the `job:update` event and `jobs_list`/`job_cancel` commands comes with T11.02
- [x] **T11.02** Migrate scan/download/propose/narrative to jobs (2026-10-11) — `AppState.jobs` (`JobRegistry`, recovered at startup), `AppEvent::JobUpdate` → `job:update` event, commands `jobs_list` / `job_cancel`, `service::jobs::{list,cancel,end}`. Scan (`scan`), rename index (`index`) and proposals (`propose`) take their cancel token from the job (old cancel commands still work); downloads get a mirror job whose cancel cancels the download and whose progress follows the bytes. **Narrative is a normal chat turn (`ChatJobs`), not a job — left as is.** `RenameRuntime`'s cancel map stays (it now holds the job token). **App crate written but not compiled (no GTK); `bindings.ts` regenerates on the Mac**
- [x] **T11.03** UI Jobs indicator + popover (progress, cancel, open result) (2026-10-11) — `JobsIndicator` in the title bar (hidden until a job exists), `useJobs` (query + `job:update` upsert), `api.jobs`, local `JobSummary` type in `features/jobs/types.ts` (switch to the `bindings.ts` type after it regenerates). **Frontend not linted/typechecked/tested (owner asked for no runs); no component test yet**
- [x] **T11.04** Notification preferences per kind + routing (click → route) + tests (2026-10-11) — routing is the pure `Settings::wants_os_notification` (master switch, per-kind, unfocused, quiet hours via new `notifications.quiet_start/quiet_end` "HH:MM"), tested in nexus-core; `notify.rs` calls it with the local minute of day (`time` gained `local-offset`; UTC fallback). Click → route already travels as `NotificationAction.route`. Settings UI: `features/settings/SettingsPage.tsx` (all settings groups incl. per-kind switches and quiet hours) was added afterwards at `/settings`. **App crate unverified (no GTK); settings page not linted/typechecked/tested**
- [x] **T11.05** Watches backend (scheduler, `notify` on-change debounce, `diff_reports`, thresholds) + tests (2026-10-11) — `nexus-analyze::watch` (`diff_reports`, `is_significant`, `notification_text`, `is_due`, `Debounce`, `watch_folder` on `notify` 8.2; refuses home/root), `nexus-store::watches` CRUD, app `service::watches` (minute scheduler, on-change watchers, background runs never prompt, manual run always notifies) + commands `watches_list/watch_create/watch_set_enabled/watch_set_schedule/watch_delete/watch_run_now`. Pure parts have tests; **app crate written but not compiled or run (owner asked for no build/test runs this session)**
- [x] **T11.06** UI "Watch this folder" + watches list in Analyzer (2026-10-11) — `WatchesSection` on the Analyzer home (desktop only): pick folder + schedule, enable switch, Run now, delete; `api.watches`; local types in `features/analyzer/watches.ts`. Not done: a "Watch this folder" button on a report page. **Frontend not linted/typechecked/tested (owner asked for no runs)**
- [ ] **T11.07** (optional) Tray icon + launch at login (autostart plugin)
- [ ] ⭐ **T11.08** 🍎 Exit check: 3 parallel jobs + cancel; OS notifications in background; watch delta notification

## M12: Bundling, profiles, App Builder (macOS) · [spec](docs/milestones/M12-bundling-app-builder.md) · [guide](guide.html)
- [x] **T12.01** Validate `bundles/*.json` against `schemas/bundle-profile.schema.json` in a test (node or Rust) (2026-10-12) — `scripts/schemas.test.mjs` with a small dependency-free checker `scripts/lib/jsonschema.mjs`; also covers the manifest and model-pack schemas
- [ ] **T12.02** 🍎 Harden `fetch-model.mjs` multi-model (`--profile`, `--repo`, `--local`, `--verify --all`) on the Mac
- [x] **T12.03** `scripts/stage-bundle.mjs` (hardlinks, manifest.json, licenses, generated overlay) + test with fake 10 MB models (2026-10-12) — `scripts/stage-bundle.mjs` (CLI + `stage()`; `bundle.mjs` now uses it), test `scripts/stage-bundle.test.mjs` with fake 10 MB models; the manifest is validated against its schema; adds sampling/capabilities from the catalog, `licenses/models.txt`, and git commit/app version in `bundle.json`
- [x] **T12.04** `scripts/verify-payload.mjs` (re-hash models + runtime, lipo/otool, Info.plist keys; `--windows` PE arch check) (2026-10-12) — `scripts/verify-payload.mjs` + `verify-payload.test.mjs`: models (size + SHA-256, unlisted files), expected manifest, knowledge, bundle.json, runtime lock, macOS Info.plist/lipo/otool (skipped where the tools are missing), Windows PE machine type. Tamper test included
- [x] **T12.05** `scripts/build-app.sh --profile <p>` (doctor → verify → stage → build:web → tauri build → verify → codesign → zip + SHA256SUMS) (2026-10-12) — `scripts/build-app.sh`: doctor → fetch/verify models → runtime lock → stage → web UI → tauri build → verify-payload → codesign (`NEXUS_SIGN_IDENTITY`) → zip + SHA256SUMS.txt; `--dry-run` is tested (`build-app.test.mjs`). **A real build needs the Mac**
- [x] **T12.06** `scripts/app-builder.mjs` server (127.0.0.1 + token, profile CRUD, size calc, build runner, SSE logs, idle exit, CLI mode) (2026-10-11) — simplified: `scripts/app-builder.mjs` (127.0.0.1 + token, idle exit, SSE log) runs `scripts/bundle.mjs`; no profile CRUD or size calc yet
- [x] **T12.07** `scripts/app-builder/index.html` UI (models checklist, HF/local add, default radio, targets, Windows format, size summary, results) (2026-10-11) — `scripts/app-builder/index.html`: Downloads .gguf picker, folder browser, catalog, books, targets
- [x] **T12.08** `scripts/make-model-pack.mjs` + `schemas/model-pack.schema.json` usage + test (2026-10-12) — `scripts/make-model-pack.mjs` (`--profile`/`--id`, `--out`, `--verify <pack>`) + test; the pack is validated against `schemas/model-pack.schema.json`
- [x] **T12.09** In-app model packs: export + import (verify, copy/reference, progress) + `models-extra/` first-launch import (2026-10-12) — `nexus-models::pack` (export, import with copy/reference, SHA-256 on every file, cancel cleans up, `models-extra/pack.json` registered in the background as bundled), commands `models_pack_*` + `models_pick_folder`, Models page dialogs. The Windows kit end-to-end needs the Windows build (M13)
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
- [x] **T14.02** `nexus-share` auth: pairing secrets (single-use, 5 min), PIN + rate limit/lockout, device sessions (hash-only), CSRF + tests (2026-10-11) — written; not compiled or tested (session was code-only)
- [x] **T14.03** WebSocket RPC (protocol, allow-list, streaming, cancel, push events) + policy guard + tests (2026-10-11) — written; not compiled or tested (session was code-only)
- [x] **T14.04** Static web UI embedding (`dist-web` via include_dir!, SPA fallback, security headers, cache rules) (2026-10-11) — written; not compiled or tested (session was code-only)
- [x] **T14.05** Server lifecycle: ShareController start/stop/restart, port 9889 default, `port_in_use` + next-free suggestion, graceful stop + tests (2026-10-11) — written; not compiled or tested (session was code-only) — unit tests for auth/policy/assets/urls written; integration tests in `crates/nexus-share/tests/server.rs`
- [x] **T14.06** Store migration 0006 (`share_devices`, `conversations.origin`) + commands `share_*` + `share:status` (2026-10-11) — written; not compiled or tested (session was code-only) — migration is `0005_share.sql` (list was gapless); `conversations.origin` written on remote chat
- [x] **T14.07** `ShareApi` impl → `service::*` with `Caller::Remote`; remote grant requests → desktop modal "from <device>" (2026-10-11) — prompts now carry the device name
- [x] **T14.08** mDNS (`nanonexus.local`) + Info.plist `NSLocalNetworkUsageDescription` / `NSBonjourServices` (2026-10-11) — written; not compiled or tested — Bonjour via `mdns-sd`; the Info.plist keys were added earlier
- [x] **T14.09** Optional HTTPS (rcgen self-signed, SAN = LAN IPs + .local, fingerprint) (2026-10-11) — written; not compiled or tested — rcgen certificate kept in the data folder, ring TLS provider, fingerprint in Settings → Sharing
- [x] **T14.10** Frontend `wsTransport` (reconnect, resubscribe, visibilitychange refetch) + `pnpm build:web` (`dist-web`) (2026-10-11) — written; not compiled or tested (session was code-only)
- [x] **T14.11** Mobile layout (< 768 px): bottom tabs, drawer, safe areas, 44 px targets, 16 px inputs, long-press menus (2026-10-11) — written; not compiled or tested (session was code-only)
- [x] **T14.12** Pairing screen (QR fragment + PIN pad) + PWA manifest/icons + minimal service worker + desktop-only route guards (2026-10-11) — written; not compiled or tested (session was code-only) — pairing screen is a PIN/code field (QR scan opens `/pair#secret`); no icon PNGs, SVG icon only
- [x] **T14.13** Desktop 📡 Share pill + popover (switch, URLs, QR SVG, PIN, devices) (2026-10-11) — written; not compiled or tested (session was code-only)
- [ ] **T14.14** Settings → Sharing page (all LAN-SHARE §2 settings + devices table) + pairing notifications + tray item
- [ ] **T14.15** Playwright e2e (390×844 + 1280×800): pair → chat → stream → stop → history → revoke
- [x] **T14.16** Update `guide.html` Sharing section (firewall prompts, troubleshooting) + WINDOWS-TESTING firewall step (2026-10-11) — written; not compiled or tested — `docs/WINDOWS-TESTING.md` created for the firewall step
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
- [x] **T17.01** `nexus-knowledge` crate + migration `0007_knowledge.sql` (libraries, documents, chunks, FTS5/BM25) + repos + tests (2026-10-11) — migration is `0006_knowledge.sql` (gapless numbering, Q-7); repos in `nexus-knowledge::repo` (library crates compiled, clippy-clean, tests pass; app crate + UI not yet built here)
- [x] **T17.02** Extractors: md, txt, json, html (readable text), csv, docx, source files (reuse M08 extractors where present) + fixtures (2026-10-11) — md/txt/json/html/csv/tsv/docx/source via M08 + new json/csv extractors (library crates compiled, clippy-clean, tests pass; app crate + UI not yet built here)
- [x] **T17.03** PDF (page-aware) and EPUB (spine order, chapter titles) extractors + fixtures; skipped-content notes (2026-10-11) — PDF via M08 (page numbers, scanned-page notes) + EPUB spine/nav handling (library crates compiled, clippy-clean, tests pass; app crate + UI not yet built here)
- [x] **T17.04** Heading-aware chunker (600-800 tokens, overlap, breadcrumbs, page numbers) + tests (2026-10-11) — breadcrumbs, fenced-code aware, overlap 10% (library crates compiled, clippy-clean, tests pass; app crate + UI not yet built here)
- [x] **T17.05** Index job (Background, incremental by hash, progress, cancel, size/time limits) + tests (2026-10-11) — `index_library` (sync, cancellable, hash-incremental); app wraps it on the blocking pool (library crates compiled, clippy-clean, tests pass; app crate + UI not yet built here)
- [x] **T17.06** `search()` BM25 + heading weights + diversity + neighbor expansion + 500 MB perf fixture test (2026-10-11) — 500 MB perf fixture is `#[ignore]`d (`cargo test -p nexus-knowledge -- --ignored`) (library crates compiled, clippy-clean, tests pass; app crate + UI not yet built here)
- [x] **T17.07** `kb_search` tool + Librarian assistant + chat context injection (`<doc source=... page=...>`, budgeted) + verified `[n]` citations + "not in the library" rule (2026-10-11) — `kb_search` tool, Librarian, chat injection behind `knowledge.chat_context`; Nexus unchanged (Q-7) (library crates compiled, clippy-clean, tests pass; app crate + UI not yet built here)
- [x] **T17.08** Prompt-injection and no-regression tests (libraries off = identical prompts/snapshots) (2026-10-11) — defanging + e2e tests in `nexus-knowledge`; libraries off = no code path touched (library crates compiled, clippy-clean, tests pass; app crate + UI not yet built here)
- [x] **T17.09** Commands `kb_*` (list, add, remove, enable, reindex, search, passage) + job events (2026-10-11) — `kb_*` commands; progress via Channel (no separate job events yet) (library crates compiled, clippy-clean, tests pass; app crate + UI not yet built here)
- [x] **T17.10** UI: Library page (add via drag-drop/picker, progress, enable, re-index, remove, passage search) + composer "Libraries" chip + clickable citations (2026-10-11) — Library page, composer chip, clickable citations (library crates compiled, clippy-clean, tests pass; app crate + UI not yet built here)
- [x] **T17.11** Bundling: profile `knowledge[]`, stage-bundle copies sources + prebuilt index, `verify-payload` hashes, App Builder size summary (2026-10-11) — profile `knowledge[]`, `scripts/knowledge-manifest.mjs` (stage/verify/size) ready for stage-bundle/verify-payload once M12 lands; prebuilt index deferred (Q-6) (library crates compiled, clippy-clean, tests pass; app crate + UI not yet built here)
- [x] **T17.12** Docs: user guide section, sample library, walkthrough (e.g. a Reaper manual, a JavaScript book) (2026-10-11) — guide.html section, `docs/samples/knowledge/mixing-basics` + walkthrough (library crates compiled, clippy-clean, tests pass; app crate + UI not yet built here)
- [ ] ⭐ **T17.13** 🍎 Exit check: offline cited answers from a bundled library; user-added EPUB/PDF/MD work; libraries off = no behavior change

## M18: Duo assistant (light + strong model) · [spec](docs/milestones/M18-duo.md)
- [x] **T18.01** v0.1: `duo` assistant, heuristic tier classifier (`nexus-agent::duo`), settings `duo.light_model_id` / `duo.strong_model_id`, per-message model swap in chat, Duo panel on the Models page (2026-10-11) — one model runs at a time (swap on demand); classifier + settings + prompt_lint tested; app crate + UI not built here
- [ ] **T18.02** 🍎 Try Duo with two real models; tune `HARD_WORDS` / thresholds; note swap latency in SPIKES.md
- [x] **T18.03** Show the chosen tier on the message ("light" / "strong") and a "Use strong model" button on light answers (2026-10-11) — `AssistantMessage` gets `tier` + `onUseStrong`; `ChatView` derives the tier by comparing the stored `model_id` with `duo.light_model_id` / `duo.strong_model_id` (no badge when unset or equal); the button regenerates with Think harder, which Duo always routes to the strong model. **Frontend written but not linted/typechecked/tested (owner asked for no runs); no component test added yet**
- [ ] **T18.04** (v0.2) Run both models at the same time (second `Supervisor` + port, memory check via `MemoryInfo`) to remove swap latency
- [ ] **T18.05** (v0.2) Optional model-based classifier (light model scores difficulty 1–5) and automatic escalation when the light answer looks unsure
