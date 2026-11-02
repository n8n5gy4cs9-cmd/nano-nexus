# USB Portable Studio conversion tasks

Updated: 2026-11-02. `[x]` means the stated deliverable is complete; it does not imply runtime verification. Update this file immediately whenever a task completes. No testing; ask before builds.

## Done — planning

- [x] **UP.001** Inspect local reference README, frontend/server capability surface, existing path resolution and bundling contract. (2026-11-02; source inspection only)
- [x] **UP.002** Write `update-plan.md` with explicit/inferred requirements, proposed DIST layout, phases, preservation rules and feasibility limits. (2026-11-02)
- [x] **UP.003** Create categorized task tracker and request scope/storage-boundary clarification before code. (2026-11-02)

## Pending decisions

- [x] **UP.004** Owner confirmed full reference parity in phases. (2026-11-02)
- [x] **UP.005** Owner clarified: no large host files, small OS traces acceptable, expose observed writes; Mac/Windows priority and Windows without NVIDIA on older PCs. Exact minimum OS/CPU remains a packaging decision. (2026-11-02)

## Undone — architecture and inventory

- [x] **UP.010** Source capability/dispatch inventory, feature-to-task map, license obligations and model/backend matrix in docs/portable/REFERENCE-INVENTORY.md. (2026-11-02; runtime behavior and artifact licenses remain unverified delivery gates)
- [x] **UP.011** Record existing feature/service preservation rules and known verification gaps in docs/portable/NANONEXUS-INVENTORY.md. (2026-11-02; later implementation remains subject to these contracts)
- [x] **UP.012a** Record ADR-018 and packaging conflict Q-12 under owner-authorized USB direction. (2026-11-02)
- [ ] **UP.012b** Reconcile bundling/model/IPC contracts with ADR-018.
- [ ] **UP.013** Design versioned portable marker, relative-path registry, platform-specific state and migration/backups from existing installations.

## Undone — USB storage

- [x] **UP.020a** Implement fixed-layout root/marker resolution and shared AppPaths for Mac/Windows; recognized layouts fail on missing/invalid markers. (2026-11-02; source only, unbuilt/untested)
- [ ] **UP.020b** Add writable-root enforcement, symlink containment policy and migration behavior before declaring portable storage complete.
- [ ] **UP.021** Route DB/WAL, settings, logs, knowledge, journals, reports, outputs and credentials to USB-owned storage.
- [ ] **UP.022** Route webview profiles, child-process home/temp/cache and engine caches to USB before initialization.
- [ ] **UP.023** Handle read-only/missing drive, low space, concurrent instances, changed mount paths, interruption and portable permission grants.

## Undone — unified models

- [ ] **UP.030** Shared digest-based model registry for chat/images/speech/TTS; distinguish platform conversion variants without per-platform weight copies.
- [ ] **UP.031** Catalog and Hugging Face URL downloads; validated revisions/digests, progress, resume, cancel and disk budgeting.
- [ ] **UP.032** Import/select/unload/delete/reclassify UI, duplicate detection, supported companion assets and safe reference accounting.
- [ ] **UP.033** Migrate existing models without destructive moves or avoidable copies; keep offline pack compatibility.

## Undone — reference features and retained features

- [ ] **UP.040** Map and retain chat/history/streaming/settings capabilities from both applications.
- [ ] **UP.041** Image engine lifecycle, generation parameters, progress/cancel and model compatibility validation.
- [ ] **UP.042** Image gallery, metadata, export and journal-governed deletion.
- [ ] **UP.043** Whisper file/microphone transcription, progress/cancel, transcript storage/export and permission UI.
- [ ] **UP.044** Kokoro voice/model selection, speech generation/playback, saved audio and progress/cancel.
- [ ] **UP.045** Hardware detection, recommendations, backend selection/download and resource-aware engine switching; CPU fallback and unsupported states.
- [ ] **UP.046** Telemetry/performance display and reference settings/themes through NanoNexus design tokens.
- [ ] **UP.047** Opt-in web search with current outbound-network and credential boundaries.
- [ ] **UP.048** Retain assistants/Duo, file proposals/journal, analyzer, knowledge, authenticated LAN Share, Activity and offline help; document any incompatible reference behavior explicitly.

## Undone — packaging and updates

- [ ] **UP.050** Implement non-destructive DIST assembly with one shared model store and platform launchers.
- [ ] **UP.051** Package prebuilt macOS Rust app/engines and private workers; handle exFAT launch/signing constraints.
- [ ] **UP.052** Package prebuilt Windows Rust app/engines, private fixed WebView2 and private workers; no target installers or package managers.
- [ ] **UP.053** Pin runtime artifacts/digests, collect licenses and disable target-side source builds/implicit downloads.
- [ ] **UP.054** Implement versioned staged updates, running-process guard, rollback and data preservation; never clear DIST.
- [ ] **UP.055** Bound build scratch space and optional Colima resources; cleanup only task-owned resources after approved builds.

## Undone — guide and delivery gates

- [ ] **UP.060** Update `guide.html`: back up before formatting, choose exFAT, copy the entire DIST folder, launch each OS, download to USB, work offline, update safely and eject safely. Formatting erases data; never format automatically.
- [ ] **UP.061** Document OS/CPU minimums, hardware drivers, unsupported acceleration, USB speed/capacity and OS-managed trace limits.
- [ ] **UP.062** Present concrete build commands, target matrix, download sizes, disk estimates and cleanup plan; ask for build approval.
- [ ] **UP.063** Build Mac artifacts only after approval; mark them untested under current instruction.
- [ ] **UP.064** Build Windows artifacts only after approval; mark them untested under current instruction.
- [ ] **UP.065** Deliver ready-made DIST folder with manifests/licenses/guide; report any unverified portability claims and missing platform artifacts.

## Deferred by owner instruction

- Tests, smoke runs, runtime trials, benchmarks and full verification are not authorized. No verification task may be silently marked complete.

## Undone — write visibility and older Windows PCs

- [ ] **UP.070** USB-local bounded write audit: recorded operations versus configured destinations, byte counts where known, errors, redaction, rotation and explicit coverage gaps.
- [ ] **UP.071** App storage/write-activity view with outside-root destinations and subsystem filters; never claim to observe all OS writes.
- [ ] **UP.072** Package Windows CPU-only baseline independently of CUDA/Vulkan; add conservative non-AVX2 variant and CPU-instruction selection before engine startup.
- [ ] **UP.073** RAM-aware model recommendations and conservative context/thread defaults; explain older-PC image/speech limitations and unsupported OS/CPU cases.
