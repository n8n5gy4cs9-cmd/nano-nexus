# Portable AI Stick — session handoff

Updated 2026-11-02. Repository: `/Volumes/macdisk/Projects/RUST/nano-nexus`, branch `main`.

## Start here

1. Read `AGENTS.md`, `update-plan.md`, `update-tasks.md`, ADR-018 and `docs/portable/STORAGE.md`.
2. Inspect git status before editing. Preserve the unrelated local changes listed below.
3. Continue source work using the task-owner labels. Update `update-tasks.md` immediately after each task, commit that task and push to `main`. Do not open branches/PRs.
4. **No tests, smoke runs, runtime trials, benchmarks, builds, dependency/bootstrap downloads or containers were run in this batch. The owner forbids testing and requires approval before builds.** Do not interpret “continue” as build approval. Read-only source inspection and source formatting are allowed.
5. Report source completion separately from working binaries or verified behavior. There is **no ready-made DIST release yet**, and nothing is running in the background.

The active 12-task batch is finished at source/documentation level. The owner then requested this handoff (`update-handoff.md`, not `handoff.md`) and a short next-session prompt. New implementation work remains in the checklist; do not count the full conversion as finished.

## Owner's target

One copyable USB DIST containing prebuilt Mac/Windows applications, private dependencies, one shared model store, all application-controlled large files/caches/temp on USB, and updates that preserve models/data. Small OS-managed traces are acceptable; expose observable/configured write destinations honestly. Recommend exFAT in the user guide, with backup/erase warning. Never format a drive automatically.

Windows must work without NVIDIA/CUDA or a GPU, including older PCs. CPU-only and conservative non-AVX2 artifacts/selection remain delivery work; no current runtime compatibility claim has been verified. Prioritize Apple Silicon macOS and Windows x64; other architectures need explicit documented support.

Full Portable Local Studio parity was confirmed in phases: chat/model management, images, Whisper, Kokoro, galleries, hardware/backend selection, telemetry/settings/themes and opt-in web search, plus existing NanoNexus capabilities. The owner added ClaudeCode-Portable features and useful code-stick patterns, emphasizing coding agents that actually perform tool work. Cloud support is opt-in; local/offline llama.cpp remains the default. Do not add a second Ollama model store merely to imitate a reference.

## Completed batch and commits

All listed commits are pushed to `origin/main`.

| Task | Deliverable | Commit |
|---|---|---|
| UP.010 | Reference feature/route/backend/license-obligation inventory | 826de4b |
| UP.011 | NanoNexus capability and API preservation inventory | aa5e677 |
| UP.012b | Reconcile architecture contracts with ADR-018 | 9c582d1 |
| UP.013 | Versioned storage, relative references, migration/update design | dd11cd2 |
| UP.020b | Contained paths, writable probes, exclusive lifetime lock | 29201c8 |
| UP.021 | Persistent USB paths, local application Trash, root protection, autostart guard | 9db48a3 |
| UP.022 | Pre-initialization Tauri/webview path configuration and private worker environment | 42386c4 |
| UP.023 | Free-space floor, diagnostics and per-launch host-access reconfirmation | ad6070d |
| UP.030 | Transactional shared model/asset-role registry and digest-based import storage | ea4ef54 |
| UP.031 | Verified HF asset transfers, progress/resume/pause/cancel and desktop commands | d39bc3d |
| UP.032 | Models → USB Studio UI, selection/unload/reclassification/logical removal | c906bc6 |
| UP.033 | Non-destructive legacy adoption, pack imports, legacy API routing and path rebinding | 6c8d944 |

Additional owner-directed work: coding reference audit `UP.079` in `6dba35f`; Terra category guidance `UP.090` in `dd06e2a`. Initial root resolution/planning was already committed as `898b487` before this batch.

These checkmarks mean the stated source/doc deliverables exist. They are **unbuilt and untested**. They do not mean production portability, security guarantees or feature parity have been demonstrated.

## Source map

- `crates/nexus-core/src/{paths,portable}.rs`: recognized apps layout/marker, containment, directory probes, root OS lock, USB paths and worker environments.
- `apps/desktop/src-tauri/src/lib.rs`: portable preflight before Tauri builder, app-directory override, webview policy, lock lifetime, store/permissions/Trash setup.
- `crates/nexus-llm/src/{config,process}.rs`: explicit private worker environment, preserving env_clear/token isolation.
- `crates/nexus-store/migrations/0008_studio_models.sql` and `0009_studio_selections.sql`; `src/studio.rs`: model kinds/variants/roles, shared assets and selections. Never rewrite an applied migration.
- `crates/nexus-store/src/lib.rs`: USB-local consistent VACUUM INTO snapshot before a portable schema upgrade.
- `crates/nexus-models/src/studio.rs`: digest-based import/adoption, format checks, strict HF file URLs and legacy-pack asset resolution. `download.rs` retains GGUF download behavior and adds verified generic-asset transfer.
- `apps/desktop/src-tauri/src/service/studio.rs`: imports/downloads, projection into existing chat models, selection/unload/reclassification/logical removal, explicit legacy adoption/pack import, startup rebind.
- `service/models.rs`: portable Catalog/Search downloads/imports/packs use shared storage; queued destinations are resolved from the current root; legacy deletion protects shared assets.
- `commands/studio.rs` and source `bindings.rs`: thin commands and registration. New commands are desktop-only and not added to LAN allow-lists.
- `apps/desktop/src/features/models/{StudioModels.tsx,studio-types.ts}` and `src/lib/api.ts`: UI and typed Transport facade.
- `docs/portable/{REFERENCE-INVENTORY,NANONEXUS-INVENTORY,CODING-REFERENCES,STORAGE}.md`: detailed source mappings/contracts.

## Important incomplete work / limits

- Generated `apps/desktop/src/bindings.ts` was **not regenerated** (and already contains unrelated local edits). New command source registration and frontend-local types exist; regeneration awaits an approved generator/build workflow. No compile/typecheck/lint success is claimed.
- No packaged launchers, private fixed WebView2, prebuilt image/speech/TTS workers, CPU baseline artifacts, safe update assembler, final licenses or DIST artifacts yet (UP.050–065/072).
- Media “Select for workspace” saves configuration only. Image/Whisper/Kokoro engine execution is still UP.041–044. Current format checks do not guarantee backend compatibility or RAM fit. Native ckpt support is deferred visibly to the image-engine work; unsafe Python pickle loading is rejected.
- Existing runtime binary lookup remains the legacy app/resource layout; new runtime-distribution assembly/selection must be integrated before claiming the proposed runtimes/ layout works.
- Mac WKWebView uses nonpersistent browser storage because arbitrary persistent profile paths are unsupported by this integration. Core app data is USB SQLite, but browser-only preferences such as zoom still need UP.022b.
- Root protection intentionally denies agents access to the application distribution itself. Coding workspaces should be explicitly granted outside DIST (for example a sibling Projects folder on the same USB), not granted through the protected application root.
- Root/OS lock, probes and space floor are source implementations, not proven exFAT/device-removal behavior. Remaining hardening is UP.034/036: transfer coordination across old/new APIs, cancellation/shutdown races, cross-volume Trash routing, portable-required marker for moved/translocated app bundles, and visible Windows early-startup errors.
- Model paths have relative registry backing and startup projection rebinding. Other persisted internal paths, especially copied knowledge libraries, still need mount-independent rebinding (UP.023b). Host folder grants and external Activity sources require consent each launch; portable LAN Share never autostarts.
- Logical model removal retains physical assets, explicitly stated in UI. Reference-counted reclaim is UP.035. Pre-existing duplicate source files are not automatically deleted. External model-folder references are refused in USB mode rather than silently becoming nonportable.
- Write logging/view is **not implemented**: current diagnostics show configured paths, not observed writes. UP.070/071 must distinguish those and disclose OS/driver gaps.
- Optional official Claude Code runtime/provider/terminal integration is not implemented. Wrapper MIT licenses do not establish redistribution rights for official runtimes. Do not copy SDK direct-write or bypassPermissions behavior into NanoNexus; preserve Journal proposals/approval or isolate proposed edits in a scratch workspace.
- Agentic coding has **not been proven working** by this session. UP.085 must trace request → project/grants → model action → tool result/error → proposal → approval/Journal apply → truthful receipt. No runtime diagnosis/eval can be claimed without separate testing authorization.

## Recommended continuation

For a lead session: start with source review of UP.034/036 and registry/legacy integration before expanding engines. Then prioritize UP.085 with the coding-reference audit: inspect the real router/tool/turn/service/UI chain and improve visible failures/receipts without weakening permissions or inventing runtime evidence. Apply the relevant skills when editing prompts or diagnosing a concrete failure; owner restrictions still override testing workflows.

For a Terra session: whole **User documentation** (UP.060/061) and **Focused UI** (UP.022b/071) categories are marked `[TERRA]`. The write-view task depends on UP.070's agreed service schema; do not invent backend coverage. Start with guide/limitations or USB-backed UI preference persistence if no audit schema exists. `[TERRA+REVIEW]` is a bounded implementation recommendation, not sole ownership of safety decisions. See the checklist's ownership table and official-source links; no categorical model-success guarantee is asserted.

Do not begin a build merely to discover whether this source compiles. Prepare concrete commands, prerequisite/download/storage estimates and cleanup first, then ask. No Colima resources were created to clean up.

## Preserve unrelated local work

At handoff preparation, these were already modified before the conversion batch and have deliberately not been staged by it:

- `apps/desktop/src-tauri/src/service/app.rs`
- `apps/desktop/src/bindings.ts`
- `apps/desktop/src/features/settings/AboutSection.test.tsx`
- `apps/desktop/src/features/settings/AboutSection.tsx`
- `docs/design/UI-DESIGN.md`
- untracked `.agents/`

Inspect diffs and keep them. Do not use git add -A, reset, checkout or regeneration that overwrites them. Conversion commits name their own paths explicitly. No model weights/vendor/build outputs/secrets were committed.

## Short next-session prompt

Continue NanoNexus's Portable AI Stick conversion from update-handoff.md and update-tasks.md. Preserve unrelated local changes. Keep all app-controlled large files/dependencies in one USB DIST with shared models; prioritize CPU-only older Windows PCs and Mac. Include all three portable references and real agentic coding through the existing permission/Journal flow. Follow the task-owner labels; Terra can take the whole documentation or focused-UI category. Update the checklist after each task and commit/push each task to main. NO TESTING; ask before any build. Source changes so far are unbuilt/unverified; no DIST release exists yet.
