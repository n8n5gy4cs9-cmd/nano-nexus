# Build plan for approval (UP.062)

2026-11-03. **Nothing in this document has been run.** It asks for approval; it is not approval.
Sizes are estimates from what is on disk now, not measurements of the future build.

## Rules the owner set

1. Nothing may be added to the internal SSD. Everything lands on the external HDD (`/Volumes/macdisk`).
2. Nothing existing is deleted or overwritten. Only new files are created; only task-owned scratch is removed.
3. Heavy work happens in a sandboxed scratch folder; the finished `DIST` folder is then **copied** to the real USB.
4. No tests or verification runs unless separately allowed. No build without an explicit yes.

## What would be built

| Output | How | Needs |
|---|---|---|
| macOS arm64 app, `portable-dist` feature | frontend build, then `cargo build --release -p nano-nexus --features portable-dist` | installed Rust, Node, pnpm |
| Windows x64 app, `portable-dist` feature | the same via `cargo xwin` cross-compile on the Mac | installed `cargo-xwin`; **first use downloads the Windows SDK/CRT (about 1-2 GB, estimate)** |
| Chat engines | already in `vendor/runtime/macos-arm64` and `windows-x64` | nothing downloaded |
| Windows `cpu-baseline` (no AVX2) llama.cpp | must be compiled from `vendor/llama.cpp-src` with AVX2 off | cmake and a Windows cross toolchain; **not confirmed present** |
| Image (`sd-server`), speech (`whisper-cli`), voice (Node + Kokoro) engines | **not in `vendor/`** | pinned downloads or source builds; sizes unknown. Decision needed (below) |
| Fixed WebView2 for Windows | Microsoft download | licence check, roughly 150-200 MB (estimate) |

## Prerequisite I must write first (source only, no build)

`UP.050`: a DIST assembler that creates the `runtimes/<platform>/<kind>/<variant>/` layout the app now
searches, the storage marker, launchers and a manifest with digests. It does not exist yet, so an
approved build would otherwise produce loose binaries, not a DIST. I would write it and show it to you
before asking for the build go-ahead.

## Where every write goes

The existing `scripts/portable-build-scratch.mjs` already routes Cargo artifacts and registry, temp,
npm/pnpm/Corepack/XDG caches and the cargo-xwin cache into `.portable-build/<id>/` in this checkout.
Its limits (stated in `BUILD-WORKSPACES.md`) still hold: it is a sampled stop threshold, not a quota.

Not yet covered, and to be fixed or checked **before** approval:

- `~/.cargo` and `~/.rustup` on the internal SSD are only *read* (toolchain). Cargo must not write its
  registry there; the wrapper sets a separate `CARGO_HOME`, so the toolchain binaries would be reached
  through `RUSTUP_HOME`/`PATH` only. I would confirm this with a dry look at the environment, not a build.
- `HOME`-relative caches of Tauri/Node tools are not redirected by the wrapper. I would set `HOME`-relative
  variables explicitly and list anything that still escapes.
- Colima/Docker (`~/.colima`) is on the internal SSD. **I will not use it.** Windows is cross-compiled
  with `cargo-xwin`, which needs no container.
- macOS itself may still write small traces (logs, Spotlight, crash reports) that no script can prevent.

## Disk estimate (external HDD, 119 GiB free now)

| Item | Estimate |
|---|---|
| Release build artifacts, Mac | 8-15 GB (the current debug `target/` is 13 GB) |
| Release build artifacts, Windows | 8-15 GB plus 1-2 GB SDK cache |
| Staged DIST (apps + engines, without models) | 1-3 GB |
| Scratch budget I would request | 40 GiB, reserve 10 GiB |

Models are not part of the build; they are copied or downloaded to the shared store later.

## Steps, each needing its own yes

1. Write UP.050 assembler (source only). Ask review.
2. `prepare` the scratch workspace (creates empty folders, no download).
3. Mac build, then stage into `.portable-build/<id>/dist/`.
4. Windows build (this is the step that downloads the SDK; I will ask again before it).
5. Copy the staged folder to the real USB (formatted exFAT by you) with a plain copy; no signing claims.
6. Remove only the scratch folder I created, after you confirm the DIST is kept.

## Decisions I need from you

1. Allow the Windows SDK download (1-2 GB) for `cargo-xwin`?
2. Image/speech/voice engines: download pinned prebuilt releases, build from source, or ship the first DIST without them?
3. Windows no-AVX2 baseline: build it from `vendor/llama.cpp-src` (needs cmake), or ship AVX2-only first and say so?
4. May the USB be written directly by the final copy step, and which volume name?

## What this will not give you

No runtime testing, so "works on older PCs" and "runs from exFAT" stay unverified. Mac signing and
notarization are not part of this plan; an unsigned app may need a manual open on first launch.
