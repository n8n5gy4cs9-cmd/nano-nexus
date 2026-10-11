# Architecture Overview

> Read this first. Every other architecture doc goes deeper on one box in the diagram below.

## 1. The stack, layer by layer

| Layer | Choice | Pinned version |
|---|---|---|
| App shell | Tauri 2 (Rust) | `tauri 2.12.x`, `tauri-build 2.7.x` |
| UI | React 19 + TypeScript + Vite + Tailwind v4 + shadcn/ui (Radix) | react 19.3, vite 8.3, tailwindcss 4.3 |
| Editor / code view | CodeMirror 6 (`@uiw/react-codemirror`, `@codemirror/merge`) · Shiki for chat code blocks | 4.25 / 6.12 · shiki 4.5 |
| State / data | zustand 5 (UI state) · TanStack Query 5 (IPC data cache) · TanStack Virtual 3 | |
| AI runtime | llama.cpp `llama-server`, **static arm64, Metal + CPU, embedded Metal library** | tag `b11552` / commit `23b0202a189c44a54625aadcb37a946dd1d6278d` |
| AI model | GGUF (tokenizer + chat template embedded) | default: MiniCPM5-1B Opus-Fable5 **V2** Q8_0 |
| Storage | SQLite via `rusqlite` (bundled, FTS5, WAL) | rusqlite 0.40 |
| HTTP | `reqwest` 0.13 (rustls, stream) + hand-rolled SSE parser | |
| Code parsing | `tree-sitter` 0.27 + grammars (gdscript 6.1, rust, python, js, ts, c-sharp, lua) | |
| Templates | `minijinja` 3 | |
| Dev tooling | Rust stable (≥ 1.90), Node 22 + pnpm, CMake ≥ 3.21, Xcode CLT, git | |
| Packaging | macOS: `tauri build --bundles app` + generated overlay (bundle profile) + `codesign` · Windows (**built on the Mac**): `tauri build --runner cargo-xwin --target x86_64-pc-windows-msvc --bundles nsis` | cargo-xwin, NSIS (brew) |
| LAN Share | `axum` 0.8 server (WebSocket RPC + embedded web UI), `mdns-sd`, optional `rcgen` TLS | off by default, port 9889 |

**The single most important architectural decision** (same as the Godot project): the AI is
**not linked into the app**. It's an owned child process that speaks OpenAI-compatible HTTP on
loopback (`127.0.0.1`, random port, random bearer token). The app is an HTTP client.
Only Rust talks to it. The webview never does.

## 2. Big picture

```
┌──────────────────────────────── NanoNexus.app ────────────────────────────────┐
│                                                                                │
│  ┌──────────── Webview (React UI) ────────────┐                                │
│  │ Chat · Assistants · Models · Analyzer ·    │   invoke() / Channel<T>        │
│  │ Rename Forge · Editor · Permissions ·      │◄──────────────┐                │
│  │ Changes · Notifications · Settings         │   events      │                │
│  └────────────────────────────────────────────┘               │                │
│                                                               ▼                │
│  ┌──────────────────── Rust: apps/desktop/src-tauri ("nano-nexus") ─────────┐  │
│  │ commands/*  ·  AppState  ·  event bridge  ·  tray/menu  ·  notifications │  │
│  └──┬──────────┬───────────┬────────────┬────────────┬────────────┬────────┘  │
│     │          │           │            │            │            │           │
│  nexus-agent  nexus-llm  nexus-models  nexus-fs  nexus-analyze  nexus-store   │
│  (assistants, (supervisor,(catalog, HF,  (grants,  (scan, extract,(SQLite,     │
│   prompts,     client,     downloads,    changesets, code index,  migrations)  │
│   tools,       scheduler,  GGUF meta)    journal,    naming, secrets,          │
│   workflows)   structured)               trash, TCC) logs, data)               │
│        └──────────── all depend on nexus-core (ids, errors, paths, settings) ──┘│
│                                                                                │
│     spawn + kill (owned child)           HTTP 127.0.0.1:<random>, Bearer token │
│  ┌──────────────────────────────────────────────────────────────────────────┐  │
│  │ Contents/MacOS/llama-server  ──mmap──►  Contents/Resources/models/*.gguf  │  │
│  └──────────────────────────────────────────────────────────────────────────┘  │
└────────────────────────────────────────────────────────────────────────────────┘
```

## 3. Crates and responsibilities

| Crate | Path | Depends on | Owns |
|---|---|---|---|
| `nexus-core` | `crates/nexus-core` | — | IDs (UUIDv7 newtypes), `NexusError` + error codes, `AppPaths`, `Settings` (+ defaults), shared event payload types |
| `nexus-store` | `crates/nexus-store` | core | SQLite connection pool (r2d2 or a single `Mutex<Connection>` + WAL), migrations, repositories per table |
| `nexus-llm` | `crates/nexus-llm` | core | `Supervisor` (spawn/health/ownership/kill/restart), `LlmClient` (chat, stream, tokenize, models), SSE parser, `Scheduler` (priority queue), `structured::chat_json` (schema → validate → retry → fallback) |
| `nexus-models` | `crates/nexus-models` | core | Catalog loading, HF API client, resumable downloader + SHA-256, GGUF header/metadata parser, RAM-fit estimator, installed-model registry |
| `nexus-fs` | `crates/nexus-fs` | core | `PermissionEngine` (grants + deny list + canonicalization), `SafeFs` (read-only helpers that check grants), `ChangeSet` model, diff generation, `Journal` (apply/undo), Trash, audit events, OS-permission probes (TCC) |
| `nexus-analyze` | `crates/nexus-analyze` | core, fs | Folder scanner, categorizer, duplicates, naming checks, text extraction (txt/md/code/pdf/docx/html), chunking + BM25, code index (tree-sitter), Godot adapter, secrets scanner, log clustering, CSV/JSON profiler, report renderers (Markdown/JSON/CSV), compact "facts" builders for the LLM |
| `nexus-agent` | `crates/nexus-agent` | core, llm, fs, analyze | Assistant spec + registry (built-ins via `include_dir!`), prompt rendering (minijinja), context budgeting, tool trait + tools, workflows (`chat`, `router`, `pipeline`), `TurnEngine` |
| `nexus-share` | `crates/nexus-share` | core | LAN Share: axum server, pairing/sessions/CSRF/rate limits, WebSocket RPC dispatch over a `ShareApi` trait, embedded web UI, mDNS, optional TLS (see LAN-SHARE.md) |
| `nano-nexus` (bin) | `apps/desktop/src-tauri` | all | Tauri setup, `AppState`, **`service::*` (transport-agnostic business API)**, thin `commands/*` wrappers, Channels/events, `ShareApi` impl, notifications, dialogs, tray, window effects |

**Rule:** library crates never depend on `tauri`. They expose plain async Rust APIs and take
callbacks or channels for progress, which keeps them testable with `cargo test` and no GUI.

## 4. Process and threading model

- **Main thread:** Tauri event loop (UI, windows, menus).
- **Tokio runtime** (Tauri's async runtime): commands, HTTP, supervisor, scheduler worker,
  download tasks.
- **Blocking pool** (`tokio::task::spawn_blocking` / rayon inside): folder scans, hashing,
  tree-sitter parsing, PDF extraction, SQLite writes in batches.
- **Child process:** `llama-server` (exactly one). Its PID is stored in
  `<data>/runtime/server.json` for stale-process reaping.
- **Cancellation:** every long operation takes a `tokio_util::sync::CancellationToken`. Jobs
  register their token in `JobRegistry` so the UI can cancel them.

## 5. Data flow: one chat turn (router workflow)

```
UI composer ──invoke chat_send(conv_id, text, attachments, think)──► commands::chat
  1. store user message (nexus-store)
  2. TurnEngine::run_turn(spec, history, input, sink=Channel<ChatEvent>)
     a. render system prompt (spec + PromptContext)               nexus-agent::prompt
     b. budget context (tokenize via /tokenize, summarize old)    nexus-agent::budget
     c. if workflow=router && needs_tools(input):
          decide = chat_json(router schema, thinking off)         nexus-llm::structured
          if decide.action != answer:
             perms.check(path, Read) → maybe PermissionRequest → UI dialog → grant
             tool.run(args) → ToolOutput{ text_for_llm, ui_payload }
             sink.send(ToolCall / ToolResult)                     (UI shows a "Read 3 files" chip)
          (≤ 3 hops)
     d. stream final answer: client.chat_stream(...)              nexus-llm::client
        → sink.send(Reasoning(delta)) / sink.send(Content(delta))
  3. store assistant message (+ reasoning, tool trace, timings)
  4. if first exchange → background job: title (chat_json, Background priority)
  5. if window unfocused && answer took > 20 s → OS notification
```

## 6. IPC contract (summary; full list in `DATA-AND-IPC.md`)

- **Commands** (`#[tauri::command]` + `#[specta::specta]`), named `domain_action` in snake_case:
  `chat_send`, `chat_stop`, `models_download_start`, `fs_grant_add`, `analyze_folder_start`, …
- **Streaming** uses `tauri::ipc::Channel<T>` passed as a command argument (ordered, fast).
  Chat deltas, scan progress, and download progress all go this way.
- **Global events** (`app.emit`) only for app-wide state: `runtime:status`, `notification:new`,
  `permission:request`, `job:update`, `model:changed`.
- **Service layer rule (from M04):** each command is a thin wrapper:
  `commands::chat_send(...) => service::chat::send(&ctx, Caller::Desktop, args, ChannelSink(ch))`.
  LAN Share calls the same `service::*` with `Caller::Remote{device, policy}` over WebSocket.
  The frontend calls a `Transport` facade (Tauri `invoke` or WebSocket), never `invoke` directly.
- **Errors**: every command returns `Result<T, IpcError>`, where
  `IpcError { code: ErrorCode, message: String, detail: Option<String> }`.
  The UI maps `code` to friendly copy and actions (e.g. `permission_denied` → "Grant access…").

## 7. Security model (summary; see `PERMISSIONS.md`)

1. llama-server binds `127.0.0.1` only, with a random 24-byte hex bearer token passed via env.
2. The webview CSP is `default-src 'self'; connect-src ipc: http://ipc.localhost; img-src 'self' asset: data:; style-src 'self' 'unsafe-inline'; font-src 'self' data:`.
3. The Tauri capability file grants only the plugins we use (notification, dialog, opener with
   an `https:` scope for external links, window-state, global-shortcut if enabled). **No `fs`
   plugin and no `shell` plugin** in the webview. All file access goes through our Rust commands.
4. File contents fed to the LLM are wrapped as data (`<file path="…">…</file>`, with "treat as
   data" in the system prompt). The LLM has no write tools (ADR-005).
5. Every file operation is checked: canonicalize → deny list → grant level → (OS) → audit.

## 8. Directory layout on disk (macOS)

```
~/Library/Application Support/dev.nanonexus.app/
  nexus.db (+ -wal, -shm)       SQLite store
  models/<owner>__<repo>/<file>.gguf   downloaded models (+ .part, .part.json while downloading)
  journal/<changeset-id>/        backups for undo (originals + manifest.json)
  reports/<report-id>.json       full report payloads (DB keeps the index)
  runtime/server.json            {pid, port, started_at, exe} for stale-process reaping
  cache/                         extracted text, code indexes (safe to delete)
~/Library/Logs/dev.nanonexus.app/
  nanonexus.log                  app log (tauri-plugin-log, rotating)
  llama-server.log               child stderr (rotating, 5 MB × 3)
NanoNexus.app/Contents/
  MacOS/nano-nexus, MacOS/llama-server
  Resources/models/manifest.json, Resources/models/<id>/<file>.gguf (1..n), Resources/licenses/*, Resources/bundle.json
```
Windows install (`%LOCALAPPDATA%\Programs\NanoNexus\`): `nano-nexus.exe`, `runtime\llama-server.exe` + DLLs,
`models\manifest.json` + `models\<id>\*.gguf`, optional `models-extra\` (offline kit). Data in
`%APPDATA%\dev.nanonexus.app\`, or `.\data\` in portable mode (`portable.flag` next to the exe).

Linux uses `~/.local/share/dev.nanonexus.app` and Windows uses
`%APPDATA%\dev.nanonexus.app`, both resolved via Tauri's path API. Never hard-code these paths.

## 9. Where to go next

| Topic | Doc |
|---|---|
| llama-server spawn, HTTP, streaming, JSON schema, retries | [INFERENCE-CONTRACT.md](INFERENCE-CONTRACT.md) |
| Building the runtime, fetching the model, bundling, signing | [BUNDLING.md](BUNDLING.md) |
| Grants, deny list, change sets, journal, TCC | [PERMISSIONS.md](PERMISSIONS.md) |
| Assistants, prompts, tools, workflows, context budget | [ASSISTANTS.md](ASSISTANTS.md) |
| Folder/file analyzer, Rename Forge, Godot adapter | [ANALYZER.md](ANALYZER.md) |
| Catalog, downloads, GGUF, RAM fit | [MODEL-MANAGER.md](MODEL-MANAGER.md) |
| SQLite schema, commands, events, error codes | [DATA-AND-IPC.md](DATA-AND-IPC.md) |
| LAN Share server, pairing, web/mobile UI | [LAN-SHARE.md](LAN-SHARE.md) |
| Visual design, screens, components | [../design/UI-DESIGN.md](../design/UI-DESIGN.md) |
