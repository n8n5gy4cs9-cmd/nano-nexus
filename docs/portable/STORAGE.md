# Portable storage and migration contract — schema 1

2026-11-02, ADR-018. This is the target contract; checklist items describe implementation status.

## Root and marker

`DIST/portable.json` is UTF-8 JSON, at most 4096 bytes, exactly `{"schema":1}`.
Unknown fields, unsupported schema, missing marker in a recognized apps layout, links escaping
root, non-directories and unwritable storage fail startup. Do not infer USB root from cwd or
search arbitrary ancestors. Recognize apps/windows-x64, apps/windows-arm64 and
apps/macos-arm64/NanoNexus.app/Contents/MacOS. Canonicalize executable and root before use.
`portable.flag` retains its legacy executable-relative layout until explicit migration.

## Owned directories

| Relative destination | Purpose | Sharing |
|---|---|---|
| models/ | Registry and verified weights | Cross-platform compatible assets |
| runtimes/<target>/<engine>/ | Immutable prebuilt private engines/workers | Platform/architecture-specific |
| data/nexus.db and SQLite sidecars | History, settings, jobs, indexes | Single writer across all platform apps |
| data/journal, data/reports, data/knowledge, data/attachments | Persistent app data | Shared, paths stored root-relative where portable |
| data/runtime/<os>/ | Lock, PID/server metadata | Platform-local |
| data/trash/ | Journal deletion target | USB-local |
| outputs/ | Generated images/audio/transcripts + metadata | Shared |
| cache/<os>/temp, home, config, webview, workers | Temporary/profile/backend caches | Platform-local, no host fallback |
| logs/<os>/ | Bounded app/runtime/write records | Platform-local |
| data/backups/ | Migration backups with manifest | User-controlled retention |

All app-owned destinations must canonicalize inside root. Reject escaping symlinks/reparse
points before creating children; revalidate existing descendants before use. Directory names
are fixed, not supplied by the marker. Do not rely on Unix permissions/hard links on exFAT.
Filesystem mutation is single-process after a root-level OS file lock; OS lock release on
crash avoids permanently stale PID locks. A malicious process with write access to the same
USB root is outside this application's trust boundary; do not claim race-proof sandboxing.

## Relative references and host identity

Portable asset references are UTF-8 slash-separated relative components, not URLs, absolute
paths, drive letters, backslashes, dot/dot-dot, empty components or reserved Windows names.
Resolve each existing path canonically and require component-wise starts_with(root). Asset
registry records include kind, digest, byte count, relative file and explicit role/variant.
Never store an executable-supplied arbitrary path in the marker.

Host-specific permissions, external watched folders and external knowledge sources must not
be transferred as live grants to another host or silently rebound by drive letter. Store host
identity locally, expire host-bound session approvals and require reconfirmation before jobs
resume. External reads remain available with permission; explicit document modifications
remain Journal-governed. This boundary does not ban editing user-selected host documents.

## Secrets and write reporting

Portable tokens default to process memory/environment, never plaintext automatic persistence
or an implicit host keychain entry. A future encrypted secret store requires an explicit design.
Private child environment must override home/temp/XDG/HF/ONNX/Torch/package caches before
spawn. Webview profile must be set before window creation. Do not change process-global HOME
inside multithreaded Rust; pass explicit command environments. Never log tokens or contents.
Record app-owned writes and configured worker paths separately; do not call permission audits
or configured destinations observed writes. Bounded logs stay on USB and never self-audit.

## Startup and failure lifecycle

1. Resolve root before webview/plugin initialization, validate marker/containment.
2. Acquire exclusive OS lock at root, validate/create owned directories and probe writable
   storage with a uniquely named small file. Keep lock through engine shutdown.
3. Confirm free-space floor, then initialize USB-local profiles/DB/services.
4. On write failure (full/read-only/removed drive), return a useful error; do not switch storage
   roots, retry indefinitely or report a completed operation before fsync/commit succeeds.
5. Existing download/job recovery handles interrupted work; retain verified partials only where
   resume identity is known. Reconfirm host permissions before background work.
6. Shutdown engines, flush DB/audit/logs, release lock. Guide requires eject after exit.

## Explicit non-destructive migration

No automatic reads/copies from host app-data or legacy app bundle directories. Empty new DIST
starts empty. To migrate, owner selects source; app requires both versions stopped, previews
source/destination/estimated bytes, and refuses destination conflicts. Use SQLite backup API
for a consistent database snapshot, never copy a live WAL database alone. Write backup under
DIST/data/backups with version/hash manifest before changing schema. Existing migrations must
not mutate an imported source DB. Rebind model/library paths only after resolving and verifying
their content, and discard host-specific grants/watches/pairings until reconfirmed. Leave source
untouched. Same-volume compatible assets may be registered in place only if already under
DIST/models; external sources require explicit verified import. No automatic host migration is
implemented by root detection. Asset migration/pack import is tracked by UP.033.

## Updates

Updater cannot remove models/data/outputs/cache/logs. Prepare immutable app/runtime replacement
on USB, validate its manifest and available space, hold distribution lock, stop engines, preserve
old version for rollback and publish update only after staging succeeds. No links into model
weights from disposable staging. Database upgrades need backup and compatibility policy;
rollback of executables alone is unsafe after incompatible DB migration. Build scratch/cache
are development artifacts, not runtime dependencies. No build or verification is implied here.

## Webview implementation constraint

The local Tauri/Wry sources expose a Windows data_directory but WKWebView uses the OS-managed website store. The USB distribution selects a nonpersistent WKWebView store on macOS instead of claiming a USB-persistent browser profile. Core settings/history remain SQLite-backed; browser-only UI preferences (currently zoom and some view state) may reset between launches until moved to the USB settings store. This is a visible remaining persistence item, not a zero-host-write guarantee. Tauri app-directory overrides are configured before builder initialization; private workers receive explicit environment paths without changing process-global HOME.
