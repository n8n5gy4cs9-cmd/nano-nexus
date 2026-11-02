# USB Portable Studio conversion tasks

Updated: 2026-11-02. `[x]` means the stated deliverable is complete; it does not imply runtime verification. Update this file immediately whenever a task completes. No testing; ask before builds.

## Model ownership guidance (2026-11-02)

These are project-specific task-fit recommendations, not proof that any model can finish a category without mistakes. `[TERRA]` means GPT-6.1 Terra can own the bounded category using the existing contracts. `[TERRA+REVIEW]` means Terra can implement a scoped slice, with lead review of architecture/security/platform implications. `[LEAD]` means keep a reasoning-focused lead on the cross-cutting work; it is not a claim that Terra is incapable. `[BUILD APPROVAL]` never grants permission to build. No label authorizes testing.

Official guidance describes Terra as suitable for scoped code fixes and concise writing. Our category assignments below are an inference from task scope, not an OpenAI guarantee. Sources checked 2026-11-02: [model-selection guidance](https://learn.chatgpt.com/docs/model-selection), [GPT-6.1 Terra](https://developers.openai.com/api/docs/models/gpt-6.1-terra).

| Whole category | Recommended ownership | Boundary |
|---|---|---|
| User documentation | **TERRA — whole category** | UP.060/061: use source-backed behavior; label pending platforms honestly |
| Focused UI | **TERRA — whole category** | UP.022b/071: implement against agreed services/schema; do not invent backend guarantees |
| Planning inventories | **TERRA — whole inventory work** | Source mapping and tracker maintenance; ADR/security decisions remain lead-owned |
| USB storage and migrations | **LEAD** | Containment, permissions, device changes, DB safety and failure recovery |
| Unified model integrity/lifecycle | **LEAD or TERRA+REVIEW for bounded fixes** | Digests, resumability, deduplication, shared references and destructive cleanup |
| Reference workspace engines | **LEAD; TERRA+REVIEW for UI slices** | Native engine lifecycle, memory, cancellation and untrusted assets cross layers |
| Packaging/update delivery | **LEAD + BUILD APPROVAL** | Toolchains, WebView2, CPU instructions, signing, exFAT and rollback |
| Write audit / older Windows PCs | **LEAD backend; TERRA UI below** | Cannot claim all OS writes are observable or all old CPUs are supported |
| Coding-agent runtimes/providers | **LEAD** | Actual tool execution, protocol adaptation, shell isolation, approvals and truthful receipts |

Use Terra for one task at a time with explicit files/contracts and completion criteria. Higher reasoning can help a scoped code change, but labels do not substitute for validation. Because testing is currently forbidden, all source-only completion reports must remain explicitly unverified.


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
- [x] **UP.012b** Reconcile BUNDLING, MODEL-MANAGER, DATA-AND-IPC and OVERVIEW with ADR-018; legacy distributions remain supported. (2026-11-02)
- [x] **UP.013** Define schema-1 marker, relative references, owned paths, host-bound grants, explicit backup/import and update lifecycle in docs/portable/STORAGE.md. (2026-11-02; contract, implementation tracked separately)

## Undone — USB storage

- [x] **UP.020a** Implement fixed-layout root/marker resolution and shared AppPaths for Mac/Windows; recognized layouts fail on missing/invalid markers. (2026-11-02; source only, unbuilt/untested)
- [x] **UP.020b** Add contained directory/marker resolution, escaping-link rejection, per-owned-directory writable probes and an OS lifetime lock. Startup never implicitly migrates legacy/host sources; explicit migration remains UP.033. (2026-11-02; source only, unbuilt/untested)
- [x] **UP.021** Route existing DB/settings/logs/knowledge/journal/report data through shared AppPaths, add USB outputs/trash, validate DB sidecars, protect entire distribution from agent writes and disable portable autostart; credentials remain memory/environment-only. (2026-11-02; source only, unbuilt/untested; new media writers tracked separately)
- [x] **UP.022** Set Tauri app paths/webview profile before initialization, use nonpersistent WKWebView on Mac, route worker home/temp/HF/Torch/ONNX/shader/package caches explicitly before spawn. (2026-11-02; source only, unbuilt/untested)
- [x] **UP.023** Add startup space floor and diagnostics; reuse OS lock/writable checks and existing DB/job/download interruption errors, reconfirm grants/external sources each launch and disable portable LAN autostart. Paths resolve from current executable, never saved mount location. (2026-11-02; source only, unbuilt/untested; asset-path migration remains UP.030/033)

## Undone — unified models

- [x] **UP.030** Add transactional model/kind/variant/asset-role registry, digest-based physical storage with cross-model reuse, bounded verified/cancellable imports and root-relative paths. Add consistent USB-local DB backup before schema upgrade. (2026-11-02; source only, unbuilt/untested; UI/downloader/legacy migration wiring remains UP.031–033)
- [x] **UP.031** Preserve catalog downloader and add shared-registry HF file-URL transfers with resolved revisions/digests, generic asset validation, streamed progress, partial-file resume/pause/cancel, disk budgeting and desktop-only services/commands. (2026-11-02; source only, unbuilt/untested; no network requests run)
- [x] **UP.032** Add Models → USB Studio with typed Transport API, category/variant/companion roles, streamed import/download progress, pause/resume/cancel, availability/error states, chat activation, workspace selection/unload, reclassification and confirmed logical removal. Shared bytes are retained safely and legacy deletion protects Studio assets. (2026-11-02; source only, unbuilt/untested; media engine integration and orphan reclaim remain pending)
- [x] **UP.033** Add explicit verified adoption of existing USB model rows without copying/moving weights, preserve legacy chat IDs, restore mount-independent projections, import schema-1 offline packs into deduplicated storage, and route legacy catalog/import/pack paths through the shared store in USB mode. Sources stay untouched; per-model failures and unsupported external references are visible. (2026-11-02; source only, unbuilt/untested)

## Undone — reference features and retained features

- [x] **UP.040** [TERRA+REVIEW] Map and retain chat/history/streaming/settings capabilities from both applications. (2026-11-02; docs/portable/CHAT-RETENTION.md; source inspection only, gaps listed there are not implemented)
- [x] **UP.041** [LEAD] Image engine lifecycle, generation parameters, progress/cancel and model compatibility validation. (2026-11-02; source only, unbuilt/untested) nexus-studio image.rs ports sd-server lifecycle/args/progress/cancel; service/media.rs, commands/media.rs and Studio page Images tab add validation, memory fit, build fallback, one job at a time. Needs prebuilt sd-server in runtimes/<platform>/image/<build>/ (UP.050+); bindings.ts not regenerated
- [x] **UP.042** [TERRA+REVIEW] Image gallery, metadata, export and journal-governed deletion. (2026-11-02; source only, unbuilt/untested) outputs gallery with metadata, settings reuse, save-a-copy; deletion moves files and sidecars to the USB app Trash (recoverable) instead of unlinking. Export into user folders is a plain download, not Journal-applied)
- [ ] **UP.043** [LEAD] Whisper file/microphone transcription, progress/cancel, transcript storage/export and permission UI. (PARTIAL 2026-11-02, unbuilt/untested) whisper-cli port (WAV only) with progress, cancel, transcript save/list/trash, permission prompt for the chosen file, Studio Transcribe tab. Not done: microphone dictation, non-WAV decoding, bundled whisper-cli
- [x] **UP.044** [LEAD] Kokoro voice/model selection, speech generation/playback, saved audio and progress/cancel. (2026-11-02; source only, unbuilt/untested) Kokoro Node worker port (reference script embedded, offline by default), voices/speed, progress, cancel, saved clips with playback, Studio Voice tab. Needs private Node + kokoro runtime packaging; no word highlighting yet
- [x] **UP.045** [LEAD] Hardware detection, recommendations, backend selection/download and resource-aware engine switching; CPU fallback and unsupported states. (2026-11-02; source only, unbuilt/untested) hardware.rs: CPU/RAM detection, recommendation with CPU/baseline fallback and unsupported states, GPU probing by parsing an installed chat engine's --list-devices (empty = unknown), telemetry sampling; service/media.rs status + hardware_telemetry command; Studio 'This computer' tab. Backend download deliberately not implemented (no implicit downloads; pinned artifacts ship in the DIST via UP.053). One heavy job at a time; speech/voice jobs stop the image engine first)
- [x] **UP.046** [TERRA+REVIEW] Telemetry/performance display and reference settings/themes through NanoNexus design tokens. (2026-11-02; source only, unbuilt/untested) live CPU/RAM telemetry in Studio (unavailable shown as unavailable); reference themes ported as six token-based accents (nord, sakura, dracula, cyberpunk, forest, ocean) in tokens.css for light and dark. Per-model settings and benchmark UI not ported)
- [x] **UP.047** [LEAD] Opt-in web search with current outbound-network and credential boundaries. (2026-11-02; kept ADR-016 WebClient, added time filter and query limits, docs/portable/WEB-SEARCH.md; source only, unbuilt/untested; chat-composer toggle and cache deliberately not ported)
- [x] **UP.048** [TERRA+REVIEW] Retain assistants/Duo, file proposals/journal, analyzer, knowledge, authenticated LAN Share, Activity and offline help; document any incompatible reference behavior explicitly. (2026-11-02; docs/portable/RETENTION.md; docs only, nothing exercised)

## Undone — packaging and updates

- [ ] **UP.050** [LEAD] Implement non-destructive DIST assembly with one shared model store and platform launchers.
- [ ] **UP.051** [LEAD] Package prebuilt macOS Rust app/engines and private workers; handle exFAT launch/signing constraints.
- [ ] **UP.052** [LEAD] Package prebuilt Windows Rust app/engines, private fixed WebView2 and private workers; no target installers or package managers.
- [ ] **UP.053** [LEAD] Pin runtime artifacts/digests, collect licenses and disable target-side source builds/implicit downloads.
- [ ] **UP.054** [LEAD] Implement versioned staged updates, running-process guard, rollback and data preservation; never clear DIST.
- [ ] **UP.055** [TERRA+REVIEW] Bound build scratch space and optional Colima resources; cleanup only task-owned resources after approved builds.

## Undone — user documentation [TERRA: whole category]

- [x] **UP.060** [TERRA] Update `guide.html`: back up before formatting, choose exFAT, copy the entire DIST folder, launch each OS, download to USB, work offline, update safely and eject safely. Formatting erases data; never format automatically. (2026-11-02; guide USB workflow added; pending launchers/updater explicitly labelled; documentation only)
- [ ] **UP.061** [TERRA] Document OS/CPU minimums, hardware drivers, unsupported acceleration, USB speed/capacity and OS-managed trace limits.
## Undone — build approval and delivery [LEAD + BUILD APPROVAL]

- [ ] **UP.062** [LEAD] Present concrete build commands, target matrix, download sizes, disk estimates and cleanup plan; ask for build approval.
- [ ] **UP.063** [LEAD] Build Mac artifacts only after approval; mark them untested under current instruction.
- [ ] **UP.064** [LEAD] Build Windows artifacts only after approval; mark them untested under current instruction.
- [ ] **UP.065** [LEAD] Deliver ready-made DIST folder with manifests/licenses/guide; report any unverified portability claims and missing platform artifacts.

## Deferred by owner instruction

- Tests, smoke runs, runtime trials, benchmarks and full verification are not authorized. No verification task may be silently marked complete.

## Undone — write visibility and older Windows PCs

- [ ] **UP.070** [LEAD] USB-local bounded write audit: recorded operations versus configured destinations, byte counts where known, errors, redaction, rotation and explicit coverage gaps.
- [ ] **UP.072** [LEAD] Package Windows CPU-only baseline independently of CUDA/Vulkan; add conservative non-AVX2 variant and CPU-instruction selection before engine startup.
- [ ] **UP.073** [TERRA+REVIEW] RAM-aware model recommendations and conservative context/thread defaults; explain older-PC image/speech limitations and unsupported OS/CPU cases.

## Coding references — added during active batch

- [x] **UP.079** Source-audit ClaudeCode-Portable and code-stick; record feature map, useful patterns, license obligations and Journal conflicts in docs/portable/CODING-REFERENCES.md. (2026-11-02; no runtime claims)
- [ ] **UP.080** [LEAD] Provider/model profiles with local default, explicit network consent, tool capability flags and memory-only secrets.
- [ ] **UP.081** [LEAD] Optional private official coding runtime integration with license gate and proposal-only isolation; never enable direct-write bypass.
- [ ] **UP.082** [LEAD] Streaming protocol adapters with tool-argument validation, bounded results, cancellation and explicit compatibility errors.
- [ ] **UP.083** [LEAD] Per-engine session resume/terminal integration without conflating IDs or replaying writes; show engine/provider identity.
- [ ] **UP.084** [TERRA+REVIEW] Project/projectless coding sessions, USB-local attachments and reconfirmed project grants across hosts.
- [ ] **UP.085** [LEAD] Complete coding turn lifecycle: real read/search/list results, validated proposals, approval/Journal apply, visible tool failures, timeouts/cancel and honest completion receipts.

## Undone — focused UI [TERRA: whole category]

- [ ] **UP.022b** [TERRA] Persist browser-only UI preferences such as zoom in USB-backed settings so nonpersistent Mac browser storage does not reset them.
- [ ] **UP.071** [TERRA] App storage/write-activity view with outside-root destinations and subsystem filters; never claim to observe all OS writes.

## Undone — remaining portable integrity work [LEAD]

- [ ] **UP.023b** [LEAD] Store/rebind copied knowledge-library and other internal persistent paths relative to DIST; keep external paths host-bound and consent-based.
- [ ] **UP.034** [LEAD] Harden transfer coordination/recovery across legacy and Studio APIs, cross-volume Trash routing, shared-asset repair and engine shutdown; review actual error propagation before release. Source-only implementation has not proved these paths.
- [ ] **UP.035** [LEAD] Add explicit reference-counted orphan-asset reclaim with preview and recoverable removal. Current Remove entry retains physical bytes and says so.
- [ ] **UP.036** [LEAD] Ensure moved/translocated portable app bundles cannot fall back to installed-mode storage; add a packaged portable-required marker and actionable early startup errors on Windows.

## Done — model guidance and session handoff

- [x] **UP.090** [TERRA] Mark whole user-documentation and focused-UI categories as Terra-suitable; mark review/lead/platform boundaries and cite current official model guidance. (2026-11-02)
- [x] **UP.091** [TERRA] Write update-handoff.md with the exact completed batch, source map, remaining limitations, protected dirty files, task-owner guidance and short next-session prompt. (2026-11-02)
