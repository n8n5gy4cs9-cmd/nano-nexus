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
your machine by `scripts/fetch-model.mjs` (HF API LFS oid) into `vendor/models/lock.json`.

---

## ADR-009: Bundle layout: `externalBin` for llama-server, `resources` for the model
**Status:** Accepted

**Decision.** `tauri.conf.json`:
`bundle.externalBin = ["../../../vendor/runtime/llama-server"]` (the file on disk carries the
`-aarch64-apple-darwin` suffix, and Tauri strips it into `Contents/MacOS/llama-server`).
`bundle.resources = {"../../../vendor/models/model.gguf": "models/model.gguf", "../../../vendor/models/lock.json": "models/lock.json", "../../../vendor/runtime/licenses/": "licenses/"}`.
The model `resources` live in an **overlay config** `src-tauri/tauri.bundle.conf.json`, used
only by `pnpm tauri build --config src-tauri/tauri.bundle.conf.json`. That way `tauri dev`
doesn't copy 1.1 GB on every build. In debug builds the model resolves from
`$NEXUS_MODEL_PATH` or `<repo>/vendor/models/model.gguf`.
At runtime, the server is resolved as `current_exe().parent()/llama-server` and the model as
`resource_dir()/models/model.gguf`. A `GGML_METAL_EMBED_LIBRARY=ON` build means **no separate
.metallib** and no Metal toolchain download. Precompiled metallibs (as in the Godot project) are
an optional optimization (M13).

---

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
