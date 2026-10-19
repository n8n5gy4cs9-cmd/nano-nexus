# Architecture Decision Records (ADR log)

Format: **Context → Decision → Consequences → Alternatives considered.**
To change a decision, add a new ADR that supersedes the old one. Don't silently edit old ones.
Agents: if your task conflicts with an ADR, stop and write the conflict in `docs/QUESTIONS.md`.

---

## ADR-001: Tauri 2 (Rust core) + React/TypeScript UI
**Status:** Accepted · 2026-10-11

**Context.** The requirements are "Rust app", "make the UI look good", a syntax-colored code
editor, rich Markdown chat, native notifications, file dialogs, and a priority on macOS. Other
(lesser) AIs must be able to continue the work reliably.

**Decision.** Use **Tauri 2.12.x (stable)**. All logic (process supervision, HTTP to
llama-server, file system, permissions, analysis, persistence) lives in Rust crates. The UI is
**React 19 + TypeScript + Vite + Tailwind CSS v4 + shadcn/ui (Radix)**. The code editor is
**CodeMirror 6**, and code blocks in chat use **Shiki**.

**Consequences.**
- Node 22 + pnpm are needed at build time only. The shipped app has no Node runtime.
- The web UI gives the best visual quality per hour of work, and LLMs know React and Tailwind
  very well.
- Tauri 3 is in alpha (3.0.0-alpha.4). **Do not upgrade** until it is stable and an ADR approves it.

**Alternatives.** egui (fast, looks utilitarian, weak code editor and Markdown) · iced (better
looks, small ecosystem) · gpui (beautiful, but unstable API and thin docs) · Dioxus (webview +
Rust UI code, but LLMs know it less well) · Slint (good, but a declarative DSL is less known to LLMs).

---

## ADR-002: AI runtime = owned `llama-server` child process over loopback HTTP
**Status:** Accepted · cloned from the Godot project

**Context.** The Godot project proved the pattern: the AI isn't linked into the app. It's a
child process that speaks the OpenAI-compatible HTTP API on `127.0.0.1` with a random port and
token.

**Decision.** Ship a statically linked arm64 `llama-server` (Metal + CPU) built from a **pinned
llama.cpp commit** (`b11552` = `23b0202a189c44a54625aadcb37a946dd1d6278d`, 2026-10-10). Rust spawns
it, finds a free port (bind probe, 16 retries), passes the token via the `LLAMA_API_KEY` env var
(never argv), polls `/health`, confirms ownership with an authenticated `/v1/models`, and kills
it on exit.

**Consequences.** Metal/CPU switching, model loading, chat templates, and grammar sampling are
llama.cpp's job. Swapping models means restarting a process. A crash in inference can't take the
UI down.

**Alternatives.** `llama-cpp-2` in-process bindings (tighter coupling, harder Metal builds,
crash = app crash) · Ollama (external dependency, the user must install it) · MLX (Apple-only
and Python-centric).

---

## ADR-003: Structured outputs via JSON Schema, validate, one retry, deterministic fallback
**Status:** Accepted · cloned from Godot `strategy_provider.gd`

**Decision.** Anything machine-read (router actions, rename proposals, classifications, titles)
uses `response_format: {"type":"json_schema","json_schema":{"name":…,"strict":true,"schema":…}}`.
Thinking is **off** for these calls (`chat_template_kwargs.enable_thinking=false`). Rust
re-validates semantically (enums, bounds, regex, uniqueness, collisions). On failure, retry
**once** at lower temperature and append the validator's error message (the Instructor-style
"reask"). If that fails too, use a **deterministic, domain-specific fallback** (e.g. keep the
original name and mark it "needs manual review"; classify as `other`; router → `answer`).

**Why not native tool calling?** MiniCPM5 emits an XML tool-call format that llama.cpp may not
parse natively for this template. Our schema-constrained "action" protocol works with **any**
GGUF model and is grammar-enforced.

---

## ADR-004: Small model, strong tools: workflows over autonomous agents
**Status:** Accepted

**Context.** 1B models are poor at long-horizon planning. The Anthropic guide *Building Effective
Agents* (2024) recommends the simplest pattern that works: prompt chaining, routing,
orchestrator-workers. Use autonomous loops only when needed.

**Decision.** Three workflow kinds: `chat`, `router` (max 3 tool hops, one tool per hop), and
`pipeline` (fixed Rust steps with small LLM sub-steps such as map → reduce). Rust computes facts.
The LLM only turns facts into language or makes small, constrained decisions.

---

## ADR-005: The LLM never writes files directly: change sets + journal + Trash
**Status:** Accepted

**Decision.** There is no "write_file" tool exposed to the model. Any modification becomes a
`ChangeSet` (Edit/Write/Create/Move/Mkdir/Trash ops with base hashes). The UI shows a diff, the
user clicks Apply, Rust re-checks grants and hashes, backs up the originals into the journal,
writes atomically, and records the result. Undo restores from the journal. "Delete" always means
moving to the system Trash.

**Consequences.** Prompt injection via file content can't cause writes. Every change is
auditable and reversible.

---

## ADR-016: Agent tools: proposal-only file tools and opt-in web access
**Status:** Accepted · 2026-10-13 · amends ADR-005 and ADR-013 (the owner asked for `write_file` and `search_web`)

**Context.** Chat assistants could only paste code and say "I cannot write files". The owner wants
agent-style assistants that can act, including writing files and searching the web.

**Decision.**
- `write_file`, `edit_file`, `move_file` and `trash_file` exist as router tools, but they **propose**:
  each call stages a `ChangeSet` (origin = the assistant id) through a `ProposalSink`. Nothing touches
  disk until the user previews and applies it (`Journal::apply`, ADR-005 unchanged). The tool needs a
  `write`/`create`/`move`/`trash` grant and the deny list still wins. The model is told the change is
  *not yet written*.
- `search_web` and `fetch_url` run through `nexus_models::web::WebClient` only when
  Settings → Tools → "Allow web access" is on (default **off**). The fetcher accepts public `http(s)`
  addresses only, pins and re-checks DNS and every redirect, strips scripts, and caps time and size,
  so a page can never reach `llama-server` on 127.0.0.1 or the user's LAN. Search uses DuckDuckGo's
  HTML endpoint (no key). Web text is wrapped in `<web_results>`/`<web_page>` inside `<tool_result>`
  and is data, not instructions.
- `Settings.tools.disabled` (assistant id → tool names) lets the user switch off any tool an assistant
  declares; a tool can never be added to an assistant that does not declare it.
- New built-in assistant **Agent** (router, `always_route`, 5 hops) with all of these tools.
  Nexus gets the web tools (inert unless web access is on).

**Consequences.** Prompt injection from a web page or file can at worst *propose* a change that the
user sees as a diff. Web access sends search words and URLs off the computer, which is why it is opt-in.
A router decision carries file text in JSON, so `write_file` is limited to 12 000 characters per call.

---

## ADR-006: Rust→TS type sharing with tauri-specta (fallback: ts-rs)
**Status:** Accepted with fallback

**Decision.** Use `tauri-specta 2.0.0-rc.x` + `specta` (pinned exact versions) to generate
`apps/desktop/src/bindings.ts` for commands, events, and types. If it fails to compile with the
pinned Tauri, switch to `ts-rs` for types and hand-written typed `invoke` wrappers in `src/lib/ipc.ts`.
Either way, **never hand-edit generated files**.

---

## ADR-007: SQLite (rusqlite, bundled, WAL) as the single local store
**Status:** Accepted

**Decision.** One DB file `nexus.db` in the app data dir: conversations, messages (FTS5),
assistants (user), grants, audit log, change sets, jobs, notifications, models, reports, settings.
Migrations are numbered SQL files applied via `PRAGMA user_version`. Large blobs (reports,
journal backups) live on disk, and their paths are stored in the DB.

**Alternatives.** sled/redb (no SQL, no FTS) · JSON files (no queries, corruption risk).

---

## ADR-008: Default bundled model
**Status:** Accepted, pending Spike S3

**Decision.** Default: `GnLOLot/MiniCPM5-1B-Claude-Opus-Fable5-V2-Thinking-GGUF`, file **Q8_0**
(~1.1 GB). Fallbacks: V1 Q4_K_M (657 MB, smallest) and the official
`openbmb/MiniCPM5-1B-GGUF` (Q8_0/Q4_K_M). Sampling: no-think `temp 0.7, top_p 0.95, min_p 0.0`.
Think: `temp 0.9, top_p 0.95, min_p 0.0` (OpenBMB guidance; llama.cpp's default `min_p 0.05`
causes repetition on MiniCPM5). Use `repeat_penalty 1.05` only if loops are observed.

**Notes.** This is a community fine-tune. "Opus/Fable5" is the uploader's label, not an
Anthropic release. The base model is Apache-2.0, and the training-data licensing is unclear.
That is acceptable because this is a **personal** app. The revision and SHA-256 are pinned on
your machine by `scripts/fetch-model.mjs` (HF API LFS oid) into `vendor/models/<id>/lock.json`.

---

## ADR-009: Bundle layout: platform configs, staged resources, manifest of bundled models
**Status:** Accepted (revised 2026-10-11 for multi-model bundles and Windows)

**Decision.**
- macOS: `llama-server` ships as a Tauri `externalBin` (`tauri.macos.conf.json` →
  `vendor/runtime/macos-arm64/llama-server`, which carries the `-aarch64-apple-darwin` suffix on disk)
  → `Contents/MacOS/llama-server`.
- Windows: the runtime folder (`llama-server.exe` + its DLLs) ships as a **resource** `runtime/`
  (`tauri.windows.conf.json` + overlay), so the DLLs sit next to the exe.
- Models ship as resources under `models/<id>/<file>.gguf`, plus `models/manifest.json` listing
  every bundled model and the default. All of this comes from a **generated overlay config**
  (`src-tauri/gen/tauri.bundle.<profile>.<target>.json`) written by `scripts/stage-bundle.mjs`.
  `tauri dev` never copies models. Debug builds resolve them from `$NEXUS_MODEL_PATH` or
  `vendor/models/index.json`.
- Runtime resolution: macOS `current_exe().parent()/llama-server`. Windows
  `resource_dir()/runtime/llama-server.exe`. Models come from `resource_dir()/models/manifest.json`.
- `GGML_METAL_EMBED_LIBRARY=ON` means **no separate .metallib** and no Metal toolchain download.
  Precompiled metallibs are an optional optimization (M15).

## ADR-010: Permissions model: in-app grants enforced in Rust; no App Sandbox
**Status:** Accepted

**Decision.** The app is **not sandboxed** (personal app, not on the App Store, and no need for
security-scoped bookmarks). The authority is the app's own permission engine. Grants are
canonical folder roots with level Read / ReadWrite / Full. A deny list always wins. OS-level
consent is handled by macOS TCC prompts (Info.plist usage strings), and the app detects EPERM
and guides the user to System Settings. Picking a folder via `NSOpenPanel` (dialog plugin)
counts as user intent and creates the grant.

**Important:** sign the app with a **stable self-signed certificate**. With ad-hoc signatures,
the identity changes on every build, so TCC prompts again after each rebuild.

---

## ADR-011: Prompts are data, not code
**Status:** Accepted

**Decision.** Assistant system prompts live in `assistants/*.toml`. Pipeline prompts live in
`prompts/internal/*.md` (MiniJinja templates). Both are embedded at compile time via
`include_dir!`/`include_str!`, and user assistants are stored in SQLite. A prompt-lint test
checks token budgets (≤ 450 tokens for system prompts, ≤ 350 for internal step prompts), that
required template variables exist, and that the TOML matches its schema.

---

## ADR-012: Single inference lane with a priority queue
**Status:** Accepted

**Decision.** `--parallel 1` (the full context goes to one slot). The Rust `Scheduler` keeps two
priorities: **Interactive** (chat, editor actions) before **Background** (pipelines, batch
proposals, titles). Background jobs submit **one small request at a time**, so chat waits at
most for one chunk. The bounded background queue holds 8 items (as in Godot). Cancellation
drops the HTTP stream, which makes llama-server stop the slot.

---

## ADR-013: No external network by default except model downloads
**Status:** Accepted

**Decision.** The CSP blocks all remote origins. Rust's `reqwest` is used only by `nexus-models`
for catalog search and downloads, and only when Settings → Privacy → "Allow model downloads" is
on (default on). Fonts and icons are bundled locally (@fontsource, lucide).

---

## ADR-014: Windows is a must-have, cross-compiled on the Apple Silicon Mac
**Status:** Accepted · 2026-10-11

**Context.** Windows builds are required, and they must be produced on your Mac (no Windows
build machine).

**Decision.**
- App: `tauri build --runner cargo-xwin --target x86_64-pc-windows-msvc` (and `aarch64-pc-windows-msvc`)
  with **NSIS** installers (`brew install nsis llvm`). MSI is impossible off Windows (WiX).
- Runtime: the **official llama.cpp release binaries for the same pinned tag** (`b11552`),
  `win-vulkan-x64` by default (GPU + CPU fallback), `win-cpu-x64` optional, and `win-cpu-arm64`.
  Each is verified against the GitHub release-asset SHA-256 `digest` and recorded in
  `vendor/runtime/windows-*/runtime.lock.json`. Cross-compiling llama.cpp from source is optional (spike S9).
- Process safety on Windows: `CREATE_NO_WINDOW` plus a **Job Object with KILL_ON_JOB_CLOSE**,
  so the server can't outlive the app even after a crash.
- TLS: reqwest with **native-tls** (SChannel / Security.framework). This avoids aws-lc/NASM
  cross-compile pain.
- Testing: a Windows 11 ARM VM on the Mac (runs arm64 natively and x64 via Prism emulation),
  plus real x64 hardware for GPU and performance.

**Consequences.** Tauri cross-compilation is "less tested", so spike S7 runs early (M03).
Unsigned builds trigger SmartScreen ("More info → Run anyway"). Signing is optional (M16).

---

## ADR-015: Bundle profiles, App Builder, and offline model packs
**Status:** Accepted · 2026-10-11

**Context.** Besides the basic build (one default model), you want to easily ship **several
models inside the app** for machines without internet (e.g. a studio PC), on macOS and Windows.

**Decision.**
1. **Bundle profiles** (`bundles/*.json`, schema-validated) list the models (catalog id, HF
   repo + quant, or a local GGUF), the default, the targets, and the Windows format. `basic.json`
   is the default build.
2. **App Builder** (`scripts/app-builder.mjs`): a zero-dependency local web UI on 127.0.0.1 with
   a random token. Tick models, pick targets, click Build. It has a headless CLI mode too. It
   only orchestrates the same scripts (`fetch-model`, `stage-bundle`, `build-app`, `build-windows`).
3. **Bundled-model manifest** in the app (`models/manifest.json`). The app registers every listed
   model as read-only "Bundled" and lets you switch between them offline.
4. **Windows size rule:** NSIS can't build installers much over 2 GB, so payloads > 1.8 GB
   automatically use the **offline kit** (installer + `models-extra/` + WebView2 offline
   installer) or the **portable ZIP**.
5. **Model packs:** export/import folders (`pack.json` + GGUFs, SHA-256-verified) to add models to
   an installed app over USB with no rebuild and no internet.

**User docs:** `guide.html` (repo root) explains every workflow step by step.

---

## ADR-016: LAN Share: embedded axum server, pairing auth, one frontend codebase
**Status:** Accepted · 2026-10-11

**Context.** You want to use the assistant from your phone or other computers on the same
network, with a simple start/stop button and a settings page. The default port is 9889.

**Decision.**
- An optional axum server (`crates/nexus-share`) inside the app, **off by default**, bound to the
  LAN on port **9889** (configurable). It serves the same React UI built for the web
  (`dist-web/`, responsive mobile/desktop, PWA) and a WebSocket RPC that mirrors the Tauri
  commands.
- **Auth is always on:** QR/PIN pairing (single-use, 5-minute secrets, rate-limited PIN), per-device
  HttpOnly session cookies (hash-only storage), CSRF header, and revocation. HTTPS
  (self-signed via rcgen) is optional.
- **Remote policy** defaults to "chat + read granted folders". Applying changes and granting new
  folders requires the desktop (or the `full` policy plus desktop confirmation).
- `llama-server` **stays loopback-only.** Only NanoNexus's authenticated API is exposed.
- Architecture rule from M04: Tauri commands are thin wrappers over a transport-agnostic
  `service::*` layer, and the frontend calls a `Transport` facade (Tauri or WebSocket), never
  `invoke` directly.

**Alternatives.** Expose llama-server directly (no auth model, no assistants, unsafe) · a
separate web app (duplicate UI work) · Tauri remote/IPC over the network (not supported).

---

## ADR-017: Activity Observatory: passive, read-only, metadata-only agent monitoring
**Status:** Accepted · 2026-10-19

**Context.** The owner wants a live dashboard (like the "AOS-1" Claude Code activity graph) that shows
what is running: NanoNexus' own work, and optionally Claude Code and Codex sessions on the same
computer: which model, tokens, current tool, run time, stalled or errored agents, and two agents
touching the same file.

**Decision.**
- New library crate `crates/nexus-observe`, UI page **Activity** (`/activity`).
- **Passive:** external agents are observed by *tailing the transcript files they already write*
  (`~/.claude/projects/**.jsonl` incl. `subagents/`, `~/.codex/sessions/**/rollout-*.jsonl`).
  No hooks, no changes to their config, no network, no code injected into them.
- **Read-only and metadata-only:** parsers extract event *kinds, counts, model names, token usage,
  tool names, timestamps and file paths of edit tools* and **discard everything else at parse time**.
  Prompts, assistant text, shell commands, tool inputs/outputs and file contents are never stored,
  logged, sent over IPC, or placed in the replay buffer. A canary test enforces this (T19.09).
- **Opt-in:** NanoNexus' own activity is on by default (it is our data). Claude Code and Codex are
  **off until the owner enables each in Settings → Activity** (it reads outside the granted folders,
  so ADR-010's "in-app grants" model needs an explicit consent for these two fixed, read-only locations).
  The reads are confined to those two roots (canonicalised, no symlink escape) and use the OS file API
  directly, not the LLM file tools. The LLM can never see these sources.
- **In-memory by default:** the replay buffer lives in RAM (a few hours, downsampled). Nothing new is
  written to SQLite, so there is no migration. File paths are kept in memory only.
- **Desktop-only:** not exposed through LAN Share (LAN-SHARE §3 unchanged); the route is guarded like
  the other desktop-only pages.
- **Optional extras:** a terminal view (`examples/top.rs`) and a **read-only MCP server** over stdio so
  an agent can ask "what else is running / is anyone editing this file?". Both reuse the same crate.

**Alternatives.** Claude Code hooks (needs editing the user's settings and doesn't cover Codex) ·
OpenTelemetry export (needs setup per tool) · storing events in SQLite (privacy and migration cost for no
need) · a separate Electron/Node app (second codebase).

**Consequences.** Log formats are not a stable API: parsers are tolerant (unknown lines are skipped,
never fatal), have fixtures, and a "format drift" counter shows in the UI when many lines fail to parse.
