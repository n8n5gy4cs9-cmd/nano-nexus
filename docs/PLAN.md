# NanoNexus: Master Plan and Roadmap

> **How to use this plan:** pick the lowest unchecked task in [`tasks.md`](../tasks.md) (repo root),
> read its milestone doc in [`docs/milestones/`](milestones), implement, verify, tick the box,
> then **commit and push to `main`** (one task = one commit). Never start a milestone before the
> previous milestone's exit criteria pass (spikes excepted).

## 1. Milestones at a glance

| # | Milestone | Outcome (demo) | Depends on | Size |
|---|---|---|---|---|
| **M00** | [Bootstrap](milestones/M00-bootstrap.md) | Workspace compiles, checks pass, `doctor.sh` green | — | S |
| **M01** | [Runtime and model pipeline](milestones/M01-runtime-model.md) | `llama-server` built from the pinned commit; model fetched and verified; spikes S1–S3 answered | M00 | M |
| **M02** | [Inference crate `nexus-llm`](milestones/M02-nexus-llm.md) | `cargo run --example chat` streams from the owned server; JSON-schema call validated | M01 | L |
| **M03** | [App shell and design system](milestones/M03-app-shell.md) | Pretty empty app: sidebar, title bar, themes, runtime pill, notifications plumbing, walking-skeleton `.app` **and Windows skeleton (S7)** | M02 | L |
| **M04** | [Chat MVP](milestones/M04-chat.md) ★ | **First daily-usable build:** streaming chat, history, stop/regenerate, thinking toggle | M03 | L |
| **M05** | [Permissions and change sets](milestones/M05-permissions.md) | Grant folders; permission prompts; change set preview/apply/undo; audit; macOS TCC status | M04 | L |
| **M06** | [Assistants, prompts, tools](milestones/M06-assistants.md) | 17 assistants in a gallery; builder; router tools reading granted files | M05 | L |
| **M07** | [Model manager and downloads](milestones/M07-models.md) | Download view: catalog, HF search, resumable verified downloads, activate, multi-bundled models | M04 | M |
| **M08** | [Analyzer and reports](milestones/M08-analyzer.md) | Folder Scout report with treemap and AI summary; DocLens PDF summary and Q&A; other pipelines | M06 | XL |
| **M09** | [Rename Forge](milestones/M09-rename-forge.md) | Rename symbols, files, and folders in any code project, safely, with preview, undo, and verify | M08 | XL |
| **M10** | [Code editor](milestones/M10-editor.md) | Tabs, highlighting, save via journal, AI actions with diff accept | M05, M06 | M |
| **M11** | [Notifications, jobs, watches](milestones/M11-notifications-jobs.md) | Jobs panel, per-kind notification settings, scheduled folder watches | M08 | M |
| **M12** | [Bundling, profiles, App Builder (macOS)](milestones/M12-bundling-app-builder.md) | App Builder UI → pick models → signed, verified `.app`; basic build; model packs | M07 (+ features) | L |
| **M13** | [Windows build (on the Mac)](milestones/M13-windows.md) **MUST** | Installer / portable / offline kit for x64 + arm64, built on the Apple Silicon Mac | M12 | L |
| **M14** | [LAN Share + mobile web UI](milestones/M14-lan-share.md) | 📡 Start/Stop sharing on :9889, QR pairing, phone chats with your assistants | M04, M06, M08, M11 | L |
| **M15** | [Polish](milestones/M15-polish.md) | ⌘K palette, onboarding, shortcuts, Quick Ask, tray, a11y, perf, assistant evals | all | L |
| **M16** | [Optional extensions](milestones/M16-optional.md) | Linux, custom endpoint, embeddings RAG, vision, voice, notarization/signing | M13 | — |

Sizes: S ≈ ½ day · M ≈ 1–2 days · L ≈ 3–5 days · XL ≈ 1–2 weeks (one focused developer + AI).

## 2. Dependency graph

```
M00 ─► M01 ─► M02 ─► M03 ─► M04★ ─► M05 ─► M06 ─► M08 ─► M09
                      │       │       │      │      └──► M11 ─┐
                      │       │       └──────┴─► M10          │
                      │       └─► M07 ─────────────► M12 ─► M13
                      │                                       │
                      └─ S7: Windows walking skeleton          └─► M14 (LAN Share) ─► M15 ─► M16
```
Two rules started early so later milestones stay cheap:
- **From M03:** platform configs (`tauri.macos.conf.json`, `tauri.windows.conf.json`) and a
  cross-compiled Windows skeleton (S7).
- **From M04:** a transport-agnostic `service::*` layer + a frontend `Transport` facade (needed
  by M14). A responsive layout from day one (mobile breakpoints are free if built in early).

## 3. Spikes (answer early; results go into `docs/SPIKES.md`)

| ID | Question | When | Decision it drives |
|---|---|---|---|
| **S1** | Does `chat_template_kwargs.enable_thinking=false` disable thinking for the bundled GGUF? Does `--reasoning-format deepseek` split thoughts? | M01 | "Think harder" toggle support |
| **S2** | Does the OpenAI `json_schema` response_format constrain output on b11552 (pattern, enum), thinking on and off? | M01 | ADR-003 shape |
| **S3** | Q8_0 vs Q4_K_M on your Mac: tok/s, TTFT, RAM, quality | M01 | ADR-008 default quant |
| **S4** | `tree-sitter 0.27` ABI + grammars (python, js/ts, rust, c-sharp, go, java); parse your own projects? | before M09 | Grammar pins/patches |
| **S5** | `pdf-extract` quality on 3 of your PDFs | before M08 | PDF extractor |
| **S6** | `tauri-specta 2.0.0-rc.25` with `tauri 2.12`? | M03 | ADR-006 |
| **S7** | Cross-compile the M03 skeleton to `x86_64-pc-windows-msvc` + NSIS **on the Mac**; runs in the Win11 ARM VM (x64 emulation) and on a PC; is the platform config picked by `--target`? | M03 | ADR-014 viability, build flags |
| **S8** | Windows Vulkan zip: GPU used on the studio PC? Falls back to CPU without `vulkan-1.dll`? | M13 | Default Windows runtime |
| **S9** | (optional) Cross-compile llama.cpp itself for Windows on the Mac | M16 | Custom Windows runtime |

## 4. Definition of Done (every task)

1. Code compiles: `cargo build --workspace` and `pnpm -C apps/desktop build` (once they exist).
2. Checks pass: `scripts/check.sh` (fmt, clippy `-D warnings`, tests, eslint, typecheck, vitest).
3. New logic has tests (Rust unit tests at minimum; UI logic via Vitest where it's pure).
4. No TODOs without a task ID (`// TODO(T08.12): …`).
5. `tasks.md` box ticked (`[x]`) with the date and a short note if anything deviated.
6. Docs updated if behavior or contracts changed (architecture docs are contracts). `guide.html`
   updated when a user-facing build or sharing workflow changes.
7. **Commit and push to `main`** right after the task: `git commit -m "T04.07: stream chat deltas over Channel"`
   then `git push origin main`. One task = one commit.

## 5. Verification commands (copy-paste)

```bash
scripts/doctor.sh                                   # toolchain (add --windows for the cross toolchain)
scripts/progress.sh                                 # what's done / what's next
scripts/check.sh                                    # fmt + clippy + tests + frontend checks
cargo test -p nexus-llm -- --ignored                # real-model smoke (Mac, after M01)
pnpm -C apps/desktop tauri dev                      # run
node scripts/app-builder.mjs                        # App Builder UI (M12+)
scripts/build-app.sh --profile basic                # macOS basic build (M12)
scripts/build-windows.sh --profile basic --arch x64 # Windows installer, built on the Mac (M13)
```

## 6. Risk burn-down order

1. **Runtime and bundling, macOS + Windows** (M01, M03 skeletons + S7): platform risk first.
2. **Structured output with this model** (S1/S2): the core reliability assumption.
3. **Permission engine correctness** (M05): safety-critical, property-tested.
4. **Rename correctness** (M09): import/config/string reference handling, tested on a copy of a real project first.
5. **LAN exposure** (M14): auth and policy tests before any real use on shared Wi-Fi.

## 7. Working agreements for AI agents
See [`AGENTS.md`](../AGENTS.md) and [`docs/agent-playbook/`](agent-playbook). In short: one
task per commit, push to `main` after every task, smallest change that passes the DoD, never
weaken the permission engine or LAN auth, never commit `vendor/` or `*.gguf`, and ask in
`docs/QUESTIONS.md` when blocked.
