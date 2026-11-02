# USB Portable Studio conversion plan

Status: full parity authorized in phases; portable storage implementation started. Updated: 2026-11-02.

## Authority and execution

This owner-requested conversion is tracked in `update-tasks.md`; the original `tasks.md` remains intact. The owner's instructions override the normal verification loop: **no testing; ask before any build**. Do not run check scripts, smoke runs, benchmarks, compilers, bundlers, or containers without the required build approval. Completed source work must be labelled unbuilt/unverified. Update the task list immediately after each completed task, not at session end. Preserve existing uncommitted work; never sweep it into conversion commits.

## Requirements

Explicit requirements:
- Use `REFE/Portable-Local-Studio-main` as the feature source and preserve working NanoNexus features.
- Deliver one ready-made DIST folder that can be copied to USB and run without installing developer tools or runtime dependencies on the target host.
- Prioritize macOS and Windows prebuilt Rust executables; models, downloads and dependencies stay inside the portable folder.
- Keep a single model store shared by platform executables. Updates must preserve models and user data.
- Explain exFAT formatting for cross-platform USB use.
- If Colima is necessary, remove only build-owned temporary containers/disks after approved builds; do not delete unrelated Colima environments.

Confirmed scope and inferred implementation requirements:
- Full parity includes image generation, Whisper transcription, Kokoro speech synthesis, output galleries/metadata, model download/import/select/delete, engine lifecycle, hardware recommendations, backend selection, telemetry, settings/themes, and opt-in web search.
- Portability includes webview storage, worker caches, temporary files, logs, SQLite sidecars, credentials, crash output under application control, and saved outputs.
- Validate inputs, model kinds and licenses; integrity-check downloads, resume/cancel safely, deduplicate by digest, prevent traversal and untrusted executable downloads, protect credentials, and retain current permission/pairing boundaries.
- Handle empty/loading/error UI, huge/duplicate files, drive-letter/mount changes, concurrent access, interrupted downloads/updates, disk-full, read-only media, unsafe removal and unsupported hardware.

## Findings from local source inspection

The reference README describes chat (llama.cpp), images (stable-diffusion.cpp), transcription (whisper.cpp), and Kokoro ONNX TTS. Its server also exposes backend management, hardware/telemetry, output management, model import/link/move/download, and web search. Its setup scripts download dependencies on first launch and some paths require Python/Node tooling. These scripts cannot be adopted unchanged for the requested prebuilt experience. README claims are an inventory starting point, not evidence of working parity; audit individual implementation paths before marking parity complete.

NanoNexus already has model management, a portable.flag path mode, bundling, offline packs and platform build scripts. Current portable paths are executable-relative, which places macOS state inside the app bundle. Current bundling embeds per-platform models, and Windows packaging can install WebView2. These require redesign for one shared external store and no target-host installation.

## Proposed DIST contract

```
DIST/
  portable.json                 # versioned marker and layout contract
  Start-Mac.command
  Start-Windows.cmd
  apps/macos-arm64/NanoNexus.app
  apps/windows-x64/             # executable and private webview runtime
  runtimes/<platform>/<engine>/ # prebuilt engines and private dependencies
  models/                      # one registry and shared weights by digest
  data/                        # conversations, settings, knowledge, journals
  outputs/                     # images/audio/transcripts and metadata
  cache/<platform>/            # includes temporary files and shader caches
  logs/<platform>/
  licenses/
  guide.html
```

Resolve the root from the installed layout, never current working directory. Store internal paths relative to it. Reject a missing/unwritable portable root with a clear error; never silently fall back to the host profile. Preserve marker compatibility for existing portable installs through an explicit migration design. No executable writes into signed macOS bundle contents.

Models are shared across platforms only when the engine format is compatible. Platform-specific converted artifacts live once under an explicit variant key; do not pretend CoreML/OpenVINO conversions are identical to GGUF. exFAT lacks hard links and Unix permission semantics: deduplication must not depend on links, and launch behavior needs a documented solution.

## Delivery phases

1. Audit reference behavior and existing NanoNexus capabilities; map every capability to a task and document differences.
2. Approve portable storage/upgrade contract and ADR changes; implement root resolution and all application-controlled storage routing.
3. Implement shared model registry, download/import UI, integrity/resume/cancel and model-kind support.
4. Port reference capabilities through Rust services, thin commands and the existing API facade; bundle private workers where required. Retain existing assistants, tools, knowledge, LAN Share and Activity permission boundaries.
5. Add prebuilt platform packaging and safe incremental updates. Windows requires a private fixed WebView2 runtime; macOS uses system frameworks. No package managers, runtime compilation or automatic driver installation on target hosts.
6. Prepare exFAT/offline/update guide and approval-ready build commands, estimated download/disk needs and cleanup inventory. Ask before building.
7. Only after approval, produce prebuilt DIST artifacts. Testing stays prohibited unless separately authorized. Report all unverified acceptance criteria honestly.

## Update and migration rules

An update may replace only versioned application/runtime assets. Never recursively clear DIST, models, data or outputs. Stage replacements inside DIST, retain rollback metadata, handle partial updates, refuse updates while engines run, and validate manifest paths/digests. Database migrations need USB-local backup and rollback policy before code. Never duplicate weights per OS, release or staging directory. Packaging must support referencing the existing shared store rather than copying it into every archive.

## Limits and pending decisions

- Owner confirmed full parity in phases. Keep all application-controlled large files on USB; small OS-managed traces are acceptable. Provide visible write reporting with explicit coverage limits.
- Windows must work without NVIDIA or any GPU. Ship a CPU fallback and a separately compiled conservative CPU variant for older PCs; do not assume a CPU-labelled upstream binary supports non-AVX2 hardware. Model guidance must consider RAM, and unsupported instructions must be detected before starting an engine.
- A normal process cannot guarantee literally zero host writes: OS swap, security databases, crash reporting and driver caches are outside its control. Do not advertise that guarantee.
- A single folder can hold multiple binaries; one binary cannot run on both macOS and Windows. Initial proposed targets: Apple Silicon macOS and Windows x64; Windows ARM, Intel Mac and Linux require explicit support decisions.
- GPU/NPU drivers and OS frameworks must already exist. Unsupported acceleration needs a CPU fallback or a clear limitation; no host driver installation.
- Existing ADR-009/014/015 and bundling contracts must be amended explicitly before contradictory implementation. Web access remains opt-in.
- No tests or builds were run during preparation. No ready-made binaries have been produced.

## Write visibility contract

Record application-controlled write operations and configured worker/cache destinations in a USB-local bounded log. The future app view must distinguish actual recorded operations, configured destinations, and unobserved OS/driver behavior. Include destination, subsystem, operation, timestamp, outcome and bytes when known; exclude contents, tokens and secrets. Aggregate high-frequency cache writes, rotate logs and avoid recursively recording the audit log itself. Show outside-root destinations prominently; deny application-owned large-file storage outside DIST in portable mode. Explicit user-requested file edits remain governed by existing Journal permissions. This is not a system-wide filesystem monitor.

## Added coding references (owner direction, 2026-11-02)

Include REFE/ClaudeCode-Portable-main features and useful code-stick-main patterns; source audit and adaptation choices are in docs/portable/CODING-REFERENCES.md. Prioritize actual tool execution/proposal/apply/error/cancel behavior. Retain local llama.cpp/shared weights; optional cloud providers and external official runtimes need explicit engine identity, private USB dependencies, opt-in networking and license review. External direct writes/bypass modes conflict with Journal safety: isolate proposed edits and apply only through Journal. No extra Ollama weight store, plaintext secret persistence or runtime bootstrap downloads on target hosts. Existing 12-task batch continues; added coding tasks are tracked separately and not counted as completed conversion work.
