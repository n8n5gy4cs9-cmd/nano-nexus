# NanoNexus

> A private, offline AI assistant for your Mac **and Windows**, both built on your Apple Silicon
> Mac. It runs a small local model, comes with specialist assistants that can read, analyze, and
> (with your OK) change files, and can share itself to your phone over Wi-Fi.

**Status:** 📐 Planning complete, implementation not started. Progress is tracked in [`tasks.md`](tasks.md).

NanoNexus is a Rust desktop app (Tauri 2, with a React UI) that runs a bundled `llama-server`
(llama.cpp) as a private child process on `127.0.0.1`. You chat with **assistants** such as
*DocLens* (PDFs), *Folder Scout* (folder reports), and *Rename Forge* (cleans up badly named
code like an old Godot game). The model never leaves your machine. File changes are always
shown as a diff you approve, and every change can be undone.

The AI layer copies the architecture of the Godot project (*buddy bits*): an owned
`llama-server` child process, a random port, a random bearer token, JSON-schema-constrained
replies, validation, one retry, then a deterministic fallback.

---

## Documentation map

| If you want to… | Read |
|---|---|
| See what's done and what's next | [`tasks.md`](tasks.md) (run `scripts/progress.sh`) |
| Understand the product | [`docs/PRD.md`](docs/PRD.md) |
| See the roadmap and milestones | [`docs/PLAN.md`](docs/PLAN.md) → [`docs/milestones/`](docs/milestones) |
| **Build it, bundle more models, use it offline or from your phone** | [`guide.html`](guide.html) (open it in a browser) |
| Understand the architecture | [`docs/architecture/OVERVIEW.md`](docs/architecture/OVERVIEW.md) |
| See why things are built this way | [`docs/DECISIONS.md`](docs/DECISIONS.md) |
| See the UI and visual design | [`docs/design/UI-DESIGN.md`](docs/design/UI-DESIGN.md) |
| Read or edit assistants and system prompts | [`assistants/`](assistants) · [`docs/architecture/ASSISTANTS.md`](docs/architecture/ASSISTANTS.md) |
| Hand the work to another AI | [`AGENTS.md`](AGENTS.md) · [`docs/agent-playbook/`](docs/agent-playbook) |
| Share to phone / LAN | [`docs/architecture/LAN-SHARE.md`](docs/architecture/LAN-SHARE.md) |

## Repository layout (target)

```
nano-nexus/
├─ tasks.md                  ← the checklist (source of truth for progress)
├─ AGENTS.md / CLAUDE.md     ← rules for AI coding agents
├─ docs/                     ← PRD, plan, milestones, architecture, design, decisions, playbook
├─ assistants/               ← built-in assistant definitions (TOML + system prompts)
├─ prompts/internal/         ← pipeline prompts (router, map-reduce, rename proposals, …)
├─ schemas/                  ← JSON Schemas (assistant spec, LLM outputs, catalog, lock files)
├─ catalog/models.json       ← curated model catalog for the Download view
├─ scripts/                  ← doctor, progress, runtime build, model fetch
├─ crates/                   ← Rust library crates (nexus-*)          [M0+]
├─ apps/desktop/             ← Tauri app (src-tauri) + React UI (src)  [M3+]
└─ vendor/                   ← (gitignored) llama.cpp build + model files, produced locally
```

## Quick start (once implemented)

```bash
scripts/doctor.sh                       # check toolchain (Xcode CLT, Rust, Node 22, pnpm, CMake)
scripts/setup-runtime.sh                # build pinned llama.cpp llama-server (Metal, arm64, static)
node scripts/fetch-model.mjs            # download + verify the default GGUF into vendor/models
pnpm install && pnpm -C apps/desktop tauri dev            # run the app
scripts/build-app.sh --profile basic                      # NanoNexus.app with the default model
scripts/build-windows.sh --profile basic --arch x64       # Windows installer, built on the Mac
node scripts/app-builder.mjs                              # UI: tick models + targets → Build
```

The models are **not** in this repo. You build or download them on your machine, and the
models listed in a **bundle profile** go into the app at build time (`bundles/basic.json` = default
model only). Offline machines can also get more models later through **model packs** (USB).
See [`guide.html`](guide.html).

## Default model

`MiniCPM5-1B-Claude-Opus-Fable5-V2-Thinking` (Q8_0, ~1.1 GB, Apache-2.0 base). It is a community
fine-tune of `openbmb/MiniCPM5-1B` trained on Fable 5 traces, and V2 has better tool calling.
The name is the uploader's label and does not mean Anthropic made it. Other models can be
downloaded in the app. See [`catalog/models.json`](catalog/models.json) and
[`docs/architecture/MODEL-MANAGER.md`](docs/architecture/MODEL-MANAGER.md).
