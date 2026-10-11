# NanoNexus — Product Requirements Document (PRD)

| | |
|---|---|
| **Owner** | You (single user, personal tool) |
| **Version** | 1.0 (planning baseline) — 2026-10-11 |
| **Primary platform** | macOS 13+ on Apple Silicon (M1–M5) |
| **Must-have platform** | Windows 11 x64 (+ arm64), **built on the Mac** by cross-compiling |
| **Optional** | Linux x86_64/arm64 (built in a Linux VM) |
| **Status** | Approved for implementation, milestone M0 |

---

## 1. Vision

A **beautiful, private, offline AI workbench** that is chat-first and backed by real tools.
A small local model can't do everything, but a small model **plus strong deterministic Rust
tools plus well-made system prompts** can do a lot of useful work on your own files: summarize
PDFs, map out messy folders, and rename the bad identifiers in an old game, with every change
reviewed and undoable.

**Design principle: "Small model, strong tools, human approves."**
- Rust does the heavy lifting: scanning, parsing, hashing, diffing, counting, extracting.
- The LLM handles language: summarizing, classifying, naming, explaining, drafting.
- Nothing on disk changes without a preview and an explicit click. Everything can be undone.

## 2. Users and context

- **One user: you.** You're a developer (Godot/GDScript, Rust), on a Mac with Apple Silicon.
- No accounts, no cloud, no telemetry. Network is used only to download models when you ask.
- The app is built on your machine. The model and runtime are compiled or downloaded locally
  and bundled into your own `.app` and Windows installer, both built on the Mac.
- Some target machines are **offline** (e.g. a studio PC). They need several models bundled or
  sideloaded, with no internet at install time or run time.
- You also want to use the assistant from your **phone or another computer on the same Wi-Fi**.

## 3. Goals and non-goals

### Goals (v1)
1. **Chat is the main attraction.** Fast streaming chat with a local model, with good
   Markdown and code rendering, conversation history, and an optional thinking mode.
2. **Assistants.** 18 built-in specialist assistants with researched system prompts, plus an
   in-app builder to create, duplicate, import, and export your own.
3. **Bundled default model.** The app works on first launch with no downloads
   (MiniCPM5-1B Opus-Fable5 V2 GGUF inside the `.app`).
4. **Model download view.** Browse a curated catalog, search Hugging Face, and
   download/resume/verify (SHA-256). Activate, import, and delete models. Shows a RAM-fit
   badge per model.
5. **File and folder permissions.** In-app grants (Read / Read+Write / Full) per folder,
   enforced in Rust, plus OS-level handling (macOS TCC prompts, Full Disk Access guidance;
   Windows/Linux equivalents).
6. **Analyzer.** Folder reports (sizes, types, categories, duplicates, junk, naming issues,
   age, treemap) and file reports (code, text, PDF, CSV/JSON, logs), with an AI narrative.
   Export to Markdown/JSON/CSV.
7. **Rename Forge.** Index code with tree-sitter (GDScript first, plus Godot `.tscn/.tres/project.godot`
   references), detect bad names against style guides, get AI rename proposals, review them in a
   grid, apply them as a single change set with a diff preview, and undo with one click.
8. **Code editor.** Small syntax-highlighted editor (CodeMirror 6) with tabs, find/replace,
   "Ask AI about selection", and accept/reject for AI diffs.
9. **Notifications.** Native OS notifications when the app is unfocused, plus an in-app
   notification center for downloads, scans, model status, applied changes, and watch reports.
10. **Looks great.** A native-feeling macOS design (vibrancy sidebar, overlay title bar),
    dark and light themes, a command palette, and keyboard-first navigation. Windows gets
    Mica and native chrome.
11. **Windows build (must).** NSIS installer, portable ZIP, and offline kit, all cross-compiled
    on the Apple Silicon Mac.
12. **Bundle profiles + App Builder.** The basic build ships the default model. A custom build
    ships any set of models. A local App Builder UI lets you tick models and targets and click
    Build. Model packs add models to installed apps offline over USB.
13. **LAN Share.** A one-click Start/Stop server (default port 9889) with QR/PIN pairing and a
    responsive mobile/desktop web UI, so you can use your assistants from a phone on the same network.

### Non-goals (v1)
- Cloud models or API keys (an optional "custom OpenAI-compatible endpoint" may come later).
- Multi-user, sync, accounts, plugins marketplace.
- Autonomous agents that change files without approval. **Never.**
- Mac App Store distribution (no App Sandbox). Notarization is optional.
- Training or fine-tuning models.
- Vision, voice, and embeddings-RAG. Planned as optional (M16).
- Access from outside your local network (no port forwarding or relay). LAN only.

## 4. Feature requirements

Priorities: **P0** is required for v1, **P1** is strongly wanted, **P2** is nice to have.

### F1. Chat (P0)
| ID | Requirement | Acceptance criteria |
|---|---|---|
| F1.1 | Streaming replies token by token | First token appears in < 800 ms on M1 with the 1B Q8 model and a short prompt. Text streams smoothly at 60 fps without layout jank. |
| F1.2 | Markdown rendering: GFM tables, lists, code blocks with syntax highlighting, inline code, links (opened in the external browser) | Code blocks have a language label, a copy button, and "Open in editor". Rendering stays correct mid-stream. |
| F1.3 | Stop, regenerate, edit-and-resend the last user message, copy message | Stop cancels within 200 ms and keeps the partial text, marked "stopped". |
| F1.4 | Conversations: list, search (full text), rename, pin, delete, export as Markdown | Auto-title after the first exchange (internal prompt). Grouped by Today / Yesterday / 7 days / Older. |
| F1.5 | Assistant per conversation, switchable from the header | The switch applies to the next turn and shows a divider "Switched to Folder Scout". |
| F1.6 | Thinking toggle ("Think harder") | Sends `enable_thinking=true`. Reasoning appears in a collapsible "Thoughts" block and isn't fed back into history. |
| F1.7 | Attachments: drag-drop files or folders, `@` mention files in granted folders, paste text | Each attachment is a chip with token cost. Over-budget attachments are chunked or summarized, never silently truncated. |
| F1.8 | Context meter | Shows used / ctx tokens. Warns at 80%. Older turns are auto-summarized when needed. |
| F1.9 | Slash commands | `/scan <folder>`, `/summarize <file>`, `/rename <project>`, `/explain`, `/think`, `/new`, `/assistant <name>` |
| F1.10 | Performance stats | tokens/s and time to first token shown subtly under each answer (toggle in settings). |

### F2. Assistants (P0)
| ID | Requirement | Acceptance criteria |
|---|---|---|
| F2.1 | 18 built-in assistants (see §6) | Each has an icon, color, tagline, starters, sampling parameters, allowed tools, and a system prompt of ≤ 450 tokens. |
| F2.2 | Assistant gallery | Grid by category with search. A card shows name, tagline, and needed permissions. One click starts a chat. |
| F2.3 | Assistant builder | Form fields (name, icon, color, prompt, params, tools, starters, workflow) with a live token count and a test-chat side panel. |
| F2.4 | Import / export | `.toml` files validated against `schemas/assistant.schema.json`. Invalid files show field-level errors. |
| F2.5 | Duplicate a built-in to customize it | Built-ins are read-only. A duplicate becomes a user assistant. |
| F2.6 | Prompt Architect assistant generates new assistant TOML | Its output can be opened directly in the builder. |

### F3. Model runtime and bundled model (P0)
| ID | Requirement | Acceptance criteria |
|---|---|---|
| F3.1 | Bundled `llama-server` + bundled GGUF(s) in the `.app` / Windows install (default model, or every model in the chosen bundle profile) | First launch works with networking off. All bundled models are listed as "Bundled" and can be switched offline. |
| F3.2 | Owned child process on 127.0.0.1, random port, random 24-byte bearer token | `/v1/models` succeeds with the token and returns 401 without it. The token never reaches the webview. |
| F3.3 | Health and ownership checks, ready timeout, crash detection, auto-restart once | The UI shows the states Starting → Loading → Ready / Error, with the log tail. |
| F3.4 | Killed on app exit, window close (configurable), and crash recovery | No orphan `llama-server` after quitting. Stale PIDs are reaped on next launch. |
| F3.5 | Runtime settings: context size, GPU layers, flash-attn, threads | Changing them restarts the server with a toast. |
| F3.6 | Structured outputs via JSON schema, validation, one retry, deterministic fallback | Covered by unit and integration tests with a mock server. |

### F4. Model Manager / Download view (P0)
| ID | Requirement | Acceptance criteria |
|---|---|---|
| F4.1 | Tabs: Installed · Discover (curated catalog) · Search Hugging Face | The catalog loads offline from the bundled JSON. Search needs network and shows a friendly message offline. |
| F4.2 | Download with progress, speed, ETA, pause/resume, cancel | Resume works after an app restart (`.part` + meta). |
| F4.3 | SHA-256 verification against HF LFS oid; pinned revision | A mismatch deletes the `.part` and shows an error. The verified badge appears only after a hash match. |
| F4.4 | Disk-space check before download; RAM-fit badge | Badges: "Runs great", "Tight", "Too big". |
| F4.5 | Activate a model and show its params | Server restarts on the new model. Sampling presets come from catalog metadata. |
| F4.6 | Import a local `.gguf` | GGUF magic and metadata are validated, then the file is copied or linked. |
| F4.7 | Delete a downloaded model; the built-in one can't be deleted | Confirmation dialog shows the space that will be freed. |

### F5. Permissions and safe file operations (P0)
| ID | Requirement | Acceptance criteria |
|---|---|---|
| F5.1 | Folder grants: Read / Read+Write / Full (create, move, trash), persistent or "this session" | Enforced in Rust on canonical paths. Symlink escapes are blocked. Tested. |
| F5.2 | Deny list that overrides grants | `~/.ssh`, `~/.gnupg`, Keychains, `/System`, `/usr`, `C:\Windows`, the app's own DB, … |
| F5.3 | Permission prompt when an assistant needs access | Choices: Allow once · Allow folder (Read) · Allow folder (Read+Write) · Deny. |
| F5.4 | Change sets: every write is a planned set of operations with a diff preview | Nothing is written before an "Apply" click. Conflicts are detected by hash. |
| F5.5 | Journal and undo | Every applied change set can be undone, and the undo is verified by hashes. |
| F5.6 | Deletion means moving to the system Trash, never unlinking | Uses the `trash` crate. |
| F5.7 | Audit log | Every read/write/move/trash is logged with the assistant, time, and path. Viewable and exportable. |
| F5.8 | macOS OS-permission status and guidance | Detects TCC denials (EPERM) for Desktop/Documents/Downloads. Deep-links to System Settings → Privacy → Files & Folders / Full Disk Access. |

### F6. Analyzer and reports (P0 folders, P1 files)
| ID | Requirement | Acceptance criteria |
|---|---|---|
| F6.1 | Folder scan: totals, by category, by extension, largest files and folders, age buckets, duplicates (size → partial hash → full BLAKE3), empty folders, junk, naming issues, depth | 100k files scanned in < 10 s on an M1 SSD (no hashing). Progress and cancel work. |
| F6.2 | Report UI: summary cards, treemap, category bars, sortable tables, duplicate groups with "plan cleanup" | Cleanup creates a change set (Trash ops) that needs approval. |
| F6.3 | AI narrative: executive summary and recommendations from compact facts | Uses only numbers from the facts and passes validation. |
| F6.4 | File report: code (language, LOC, symbols, TODOs, long functions), text, PDF (pages, metadata, summary), CSV/JSON (profile), logs (levels, clusters, timeline) | Each type has a renderer and a facts→LLM summary. |
| F6.5 | Export to Markdown, JSON, CSV | Files are saved via the save dialog. |
| F6.6 | AI content categories (optional pass) | Classifies text documents into categories (invoice, contract, notes, code, …) using a constrained enum schema. |
| F6.7 | Watches (P1) | Re-scan a folder on a schedule or on change, and notify with what changed ("Downloads +3.2 GB since last week"). |

### F7. Rename Forge (P0 for GDScript, P1 for other languages)
| ID | Requirement | Acceptance criteria |
|---|---|---|
| F7.1 | Index a project: definitions and references via tree-sitter | GDScript, Rust, JS/TS, Python, C#, Lua. |
| F7.2 | Godot adapter: `.tscn/.tres` signal `method=` refs, exported-property keys, AnimationPlayer method tracks, `project.godot` autoloads and input actions, string-based calls (`call`, `has_method`, `Callable(obj,"x")`, `connect("x")`, `emit_signal`) | String/dynamic refs are flagged for review. Engine virtuals (`_ready`, `_process`, …) are never proposed for renaming. |
| F7.3 | Naming rules per language style guide | Issues: wrong case, too short, meaningless, Hungarian, abbreviations, inconsistent synonyms. |
| F7.4 | AI proposals in batches with a JSON schema whose regex pattern enforces the target case | Every proposal is validated: identifier syntax, not a keyword, no scope collision, convention match. |
| F7.5 | Review grid: accept/reject/edit, filters, bulk accept by confidence | Edits are revalidated live. |
| F7.6 | Apply as one change set: exact tree-sitter byte ranges, no regex replace | Unified diff preview. Warns if the git tree is dirty and offers to stop. |
| F7.7 | Strings mode: user-facing strings → improved text and/or i18n keys plus Godot translation CSV export | Opt-in per string. Placeholders (`%s`, `{name}`) are preserved and validated. |
| F7.8 | Godot 3→4 modernization hints (P1) | Flags `yield`, `onready`, `export`, `.instance()`, … and recommends `godot --headless --convert-3to4` first. |

### F8. Code editor (P1)
| ID | Requirement | Acceptance criteria |
|---|---|---|
| F8.1 | CodeMirror 6 with syntax highlighting for ~20 languages, including a GDScript mode | Matches the app theme. Handles 10k-line files smoothly. |
| F8.2 | Tabs, dirty markers, save (needs a write grant, journaled), find/replace, go-to-line | Saving is a single-file change set that is auto-applied after confirmation and can be undone. |
| F8.3 | File tree of granted folders | Lazy loading, ignore rules, icons per type. |
| F8.4 | AI actions on a selection: Explain, Improve names, Add docs, Find bugs, Ask… | Results go to the side panel or a diff view (accept/reject per hunk). |

### F9. Notifications (P0)
| ID | Requirement | Acceptance criteria |
|---|---|---|
| F9.1 | Native OS notifications when the window is unfocused (configurable per type) | Download done/failed, model ready/crashed, scan done, rename proposals ready, watch report, long answer finished. |
| F9.2 | In-app notification center (bell): list, unread count, mark read, clear, click-through action | Persisted in SQLite. |
| F9.3 | Toasts for in-focus feedback | Sonner toasts, with "Undo" actions where applicable. |

### F10. App shell and polish (P0 shell, P1 polish)
- Overlay title bar, vibrancy sidebar, dark/light/system themes, accent color choice.
- Command palette (⌘K), keyboard shortcuts, onboarding (first run), settings, logs viewer.
- Jobs panel (downloads, scans, batch LLM) with progress and cancel.
- Optional: menu-bar tray icon, quick-ask global hotkey window (⌥Space), launch at login.

### F11. Windows build, cross-compiled on the Mac (P0)
| ID | Requirement | Acceptance criteria |
|---|---|---|
| F11.1 | `scripts/build-windows.sh` builds Windows x64 and arm64 on the Apple Silicon Mac (cargo-xwin + NSIS) | Produces `…-setup.exe` without any Windows machine |
| F11.2 | Windows runtime = official llama.cpp binaries of the same pinned tag (Vulkan x64 default, CPU arm64), digest-verified | `runtime.lock.json` records the hashes |
| F11.3 | Three formats: Installer, Portable ZIP (data next to the exe), Offline kit (installer + `models-extra/` + WebView2 offline) | Payload > 1.8 GB automatically uses kit or portable (NSIS limit) |
| F11.4 | Windows parity: chat, assistants, permissions (Recycle Bin, Controlled Folder Access guidance), notifications, LAN Share | `docs/WINDOWS-TESTING.md` checklist passes on x64 hardware and an arm64 VM |
| F11.5 | No orphan server: a Job Object kills `llama-server.exe` with the app. No console window flashes | Killing the app in Task Manager kills the server |

### F12. Bundle profiles, App Builder, model packs (P0)
| ID | Requirement | Acceptance criteria |
|---|---|---|
| F12.1 | Bundle profiles (`bundles/*.json`): models (catalog / HF repo + quant / local GGUF), default model, targets, Windows format | Schema-validated. `basic.json` = default model only |
| F12.2 | **App Builder**: `node scripts/app-builder.mjs` opens a local web UI. Tick models, see total size and warnings, pick targets, click **Build**, watch logs, get artifacts with SHA-256 | Works headless too: `--profile X --target all --yes` |
| F12.3 | Multi-model bundles in the app (`models/manifest.json`) | All listed as Bundled, switchable offline, verifiable |
| F12.4 | Model packs: export (CLI or app) and import (app) a folder of verified models, no rebuild, no internet | Import verifies every SHA-256 and rejects tampered files |
| F12.5 | Extra model folders: the app also discovers GGUFs in `<exe>/models-extra/` and user-added folders | Shown under Installed → "Discovered" |
| F12.6 | `guide.html`: step-by-step user guide for every build and offline workflow (macOS, Windows-from-Mac, studio PC) | Kept up to date at each milestone exit |

### F13. LAN Share and mobile web UI (P0)
| ID | Requirement | Acceptance criteria |
|---|---|---|
| F13.1 | One-click **Start/Stop sharing** (title-bar 📡 pill + Settings → Sharing), default port **9889**, editable | Start < 1 s. Port conflicts show a fix ("Use 9890"). Stop closes the port |
| F13.2 | Pairing by QR code or 6-digit PIN; per-device sessions; device list with rename/revoke | Unpaired requests get 401. A revoked device is logged out immediately |
| F13.3 | Responsive web UI served by the app: mobile layout (< 768 px: bottom tabs, drawer, safe areas) and desktop layout (≥ 768 px); PWA installable | Playwright passes at 390×844 and 1280×800. Works on iOS Safari and Android Chrome |
| F13.4 | Remote features: chat (streaming, stop, regenerate), assistants, history, reports viewer, notifications. Desktop-only screens show a friendly hint | Remote conversations sync to the desktop, tagged by device |
| F13.5 | Remote policy: chat only / chat + read granted folders (default) / full (with desktop confirmation) | Remote can't apply changes or grant folders under the default policy |
| F13.6 | Optional Bonjour name `nanonexus.local`, optional self-signed HTTPS with fingerprint | Toggle in settings |
| F13.7 | `llama-server` is never exposed to the LAN | Verified with `lsof`/`netstat` in tests |

## 5. Non-functional requirements

| Area | Requirement |
|---|---|
| **Privacy** | No telemetry. Network only for model catalog search and downloads. A "Network: off" switch blocks even those. |
| **Security** | llama-server binds only to 127.0.0.1 with a random bearer token. The webview never talks to it (Rust proxies everything). Strict CSP. File content is treated as untrusted data (prompt-injection aware). The LLM has no direct write tool. |
| **Performance** | Cold start to interactive UI < 1.5 s. Model ready < 5 s (1B Q8, M1). Idle RAM (app without model) < 250 MB. Chat UI at 60 fps while streaming. |
| **Reliability** | Server crash → auto-restart once → clear error. No orphan processes. DB in WAL mode. Atomic file writes. |
| **Portability** | Paths resolved relative to the executable/resources, so the app works after being moved. macOS 13+ and Windows 10/11 (x64, arm64), both built on the Mac. |
| **LAN exposure** | Off by default. Auth always on. Only the NanoNexus API on the chosen port. Rate-limited pairing. Every remote action is audited. |
| **Accessibility** | Full keyboard navigation, visible focus rings, `prefers-reduced-motion`, contrast ≥ 4.5:1. |
| **Maintainability** | Rust crates with unit tests (≥ 70% on core logic). TS types generated from Rust (tauri-specta). Lint and format enforced. |
| **Size** | Basic build ≈ 1.1 GB (model) + ~40 MB (app + runtime) on macOS, ≈ 1.2 GB on Windows (Vulkan runtime DLLs). Custom profiles add their model sizes. |

## 6. Built-in assistants (v1)

| # | Name | ID | Category | What it does | Workflow |
|---|---|---|---|---|---|
| 1 | **Nexus** | `nexus` | General | Everyday copilot that can read files and folders you granted | router (tools) |
| 2 | **DocLens** | `doc-lens` | Documents | PDF and document analyst: summaries, Q&A with page citations, key facts | pipeline `doc_summarize` / `doc_qa` |
| 3 | **Folder Scout** | `folder-scout` | Files | Folder report: sizes, types, duplicates, clutter, recommendations | pipeline `folder_report` |
| 4 | **Rename Forge** | `rename-forge` | Code | Renames bad identifiers and strings in legacy projects (Godot-aware) | pipeline `rename_propose` |
| 5 | **Code Mentor** | `code-mentor` | Code | Explains, reviews, and improves code with small focused diffs | chat |
| 6 | **Bug Hunter** | `bug-hunter` | Code | Hypothesis-driven debugging from errors, traces, and logs | chat |
| 7 | **Godot Sidekick** | `godot-sidekick` | Code | Godot 4 / GDScript expert, including 3→4 migration | chat |
| 8 | **Doc Writer** | `doc-writer` | Code | Docstrings, READMEs, comments (Diátaxis-aware) | chat |
| 9 | **Commit Crafter** | `commit-crafter` | Code | Diff → Conventional Commit message, changelog, PR text | pipeline `commit_message` |
| 10 | **Tidy** | `tidy` | Files | Plans folder reorganization as a reviewable move plan, never deletes | pipeline `tidy_plan` |
| 11 | **Secret Sentinel** | `secret-sentinel` | Files | Scans for leaked keys, tokens, passwords, and PII; reports with severity | pipeline `secrets_report` |
| 12 | **Log Detective** | `log-detective` | Files | Log triage: error clusters, timeline, root-cause hypotheses | pipeline `log_triage` |
| 13 | **Data Wrangler** | `data-wrangler` | Data | CSV/JSON profiling, cleaning recipes, SQL/formulas | pipeline `data_profile` |
| 14 | **TL;DR** | `tldr` | Writing | Dense summaries of any text or file (Chain-of-Density style) | chat |
| 15 | **Scribe** | `scribe` | Writing | Write, rewrite, proofread, change tone (plain-language rules) | chat |
| 16 | **Polyglot** | `polyglot` | Writing | Translation and localization that keeps placeholders, tone, and length limits | chat |
| 17 | **Shell Sage** | `shell-sage` | System | Explains and builds terminal commands with safety ratings, never runs them | chat |
| 18 | **Prompt Architect** | `prompt-architect` | Meta | Interviews you and writes a new assistant (TOML) for NanoNexus | chat |

Users can add unlimited custom assistants (chat or router workflow, or reuse any pipeline
with their own prompt). The tutorial example is **Regex Smith**.

## 7. Success metrics (personal)

- You use NanoNexus daily for at least one of: PDF summary, folder report, code question.
- Your old Godot game is fully renamed via Rename Forge with zero broken references (the game
  runs, and Godot's headless check and import report no script errors) and the change is undoable.
- No orphan processes or data loss incidents across 30 days.
- Chat feels instant: TTFT < 1 s, ≥ 40 tok/s on your Mac with the default model.
- The studio PC (offline, Windows) runs a 3-model offline kit built on your Mac with zero internet.
- You chat with your Mac's assistants from your phone over Wi-Fi in under 30 s from "Start sharing".

## 8. Risks and mitigations

| Risk | Impact | Mitigation |
|---|---|---|
| 1B model quality is limited (reasoning, tool choice) | Wrong answers, bad tool calls | Deterministic tools do the work. Constrained JSON. Short, explicit prompts. Retry plus fallback. One-click switch to bigger models (MiniCPM5-2B, Qwen3.5-4B, Gemma 4 E4B). |
| Thinking-template quirks (`enable_thinking` ignored) | Thinking leaks into JSON or chat | `--reasoning-format deepseek` extracts thoughts. Strip `<think>` client-side. Always disable thinking for JSON calls. Spike S1 verifies. |
| JSON-schema + reasoning interplay in llama.cpp | Invalid structured output | Spike S2. Thinking off for schema calls. Validation retry with error feedback. |
| macOS TCC grants reset on every rebuild (ad-hoc signature) | Repeated permission prompts | Sign with a stable self-signed certificate (see `docs/architecture/BUNDLING.md`). |
| Rename breaks the game via dynamic or string refs | Broken project | Godot adapter scans scenes/resources/strings. Dynamic refs are flagged. Git-dirty warning. Journaled undo. Post-apply check with `godot --headless`. |
| Large-file bundling (1.1 GB) slows builds | Slow iteration | Dev mode reads the model from `vendor/` directly. Bundle only with `--bundles app`. |
| Community model licensing ambiguity (trained on Claude outputs) | Fine for personal use | Personal use only. Documented in the catalog notes. Official `openbmb/MiniCPM5-1B-GGUF` is the fallback. |
| tauri-specta still RC | Build breaks on upgrade | Pin exact versions. Fallback: `ts-rs` plus hand-written invoke wrappers (ADR-006). |
| Tauri cross-compile to Windows is "less tested" | Windows build breaks | Spike S7 in M03 (walking skeleton on Windows). Pin tauri-cli. Avoid C deps needing MSVC/NASM (native-tls). |
| NSIS ~2 GB installer limit | Multi-model Windows installers fail | Auto-switch to offline kit / portable ZIP above 1.8 GB (ADR-015). |
| LAN Share exposes the assistant on Wi-Fi | Unauthorized use on shared networks | Off by default. Pairing + revocation. Default remote policy can't write. Firewall prompt guidance. A notification on every new pairing. |
| iOS Safari drops background WebSockets | Missed streamed tokens | Auto-reconnect and refetch the conversation from the DB on resume. |

## 9. Open questions (defaults chosen, change any time)

| # | Question | Default chosen |
|---|---|---|
| Q1 | UI stack: Tauri 2 + React (web UI, Rust core) vs. pure-Rust UI (egui/iced/gpui)? | **Tauri 2 + React.** Best-looking result, best code-editor and Markdown ecosystem, easiest for other AIs to continue (ADR-001). |
| Q2 | Default quant: V2 Q8_0 (1.1 GB) vs. V1 Q4_K_M (657 MB) | **V2 Q8_0**: better tool calling, and Q8 is near-lossless at 1B. Spike S3 measures it on your Mac. |
| Q3 | Run in the background (menu bar) for Watches? | Optional, **off** by default. |
| Q4 | Rename Forge language order | **GDScript → Rust → TS/JS → Python → C# → Lua** |
| Q5 | Windows default runtime: Vulkan (GPU) or CPU-only? | **Vulkan x64** (falls back to CPU). Use `--cpu-only` for the smallest build |
| Q6 | LAN Share remote default policy | **Chat + read granted folders.** Writes only from the desktop |
| Q7 | LAN Share transport | **HTTP + pairing** by default (works on every phone without cert warnings). HTTPS optional |

## 10. Glossary

- **Assistant**: a named persona with a system prompt, params, tools, and a workflow.
- **Workflow**: `chat` (plain), `router` (model picks one tool per hop via JSON, max 3 hops), `pipeline` (fixed Rust steps with LLM sub-steps).
- **Grant**: a user permission for a folder at a level.
- **Change set**: a planned list of file operations, previewed before it is applied and journaled so it can be undone.
- **Facts**: compact deterministic summary text produced by Rust and fed to the LLM, instead of raw data.
