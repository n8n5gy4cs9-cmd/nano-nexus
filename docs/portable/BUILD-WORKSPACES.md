# Task-owned portable build workspaces (UP.055)

Source implementation only, 2026-11-02. The owner still forbids tests and requires approval
before any build. This document and script are preparation, not approval. Neither the script,
a build, a smoke run, a dependency download nor a container was run for this task.

## Scope and limits

`scripts/portable-build-scratch.mjs` owns only `.portable-build/<id>/` inside this checkout.
`prepare` refuses an existing directory and requires free bytes for the requested budget plus
reserve before creating an ownership marker and empty subdirectories. `run` passes arguments
directly without a shell and creates a separate process group on the Mac. It routes Cargo
artifacts/registry, temporary files, XDG/Corepack/npm/pnpm caches and the cargo-xwin cache into
the task workspace. `NEXUS_BUILD_OUTPUT_DIR` offers an output destination to cooperating future
DIST build scripts. It does not make legacy scripts honor that variable.

The watcher samples apparent file bytes and remaining volume space once a second. It stops
the owned process group on budget exhaustion, low space or unavailable/unsupported accounting.
A build may overshoot the threshold between samples or while a large directory is being scanned.
This is an operational stop threshold, **not a hard filesystem quota or a sandbox**. Use a
smaller threshold than the physical capacity, keep a reserve and use an OS volume quota when
a strict cap is necessary. Symlinks/special files inside scratch cause monitoring to fail closed.
Source scanning itself can be slow on large dependency trees; do not infer measured performance.

The wrapper does not redirect HOME, CODEX_HOME, shared Rust toolchains, the macOS SDK, vendor
models/runtimes, repository node_modules, generated Tauri overlays, frontend dist, legacy dist/
or arbitrary tool-specific output paths. Existing legacy build scripts also assume `target/`
for locating artifacts and run prohibited verification steps. Do not wrap those scripts
unchanged and call the result a USB DIST build. UP.050–054/062 must choose concrete commands,
route staging/output paths, avoid unauthorized checks and account for all of those locations
separately before a build approval request. Select already installed toolchains deliberately;
no automatic toolchain installation is authorized by this helper.

## Lifecycle, after approval of the concrete build

Examples below are commands for the owner/next approved build session, not executed steps.
Node 22+ must already be present. IDs are 1–64 ASCII letters, digits, underscores or hyphens.
Budgets are whole GiB: scratch 1–512, reserve 1–128; defaults are 32 and 4. Preparation fails if
the full requested allocation plus reserve is not currently available (no physical reservation
is made).

```sh
node scripts/portable-build-scratch.mjs prepare --id up063-mac --budget-gib 32 --reserve-gib 4
node scripts/portable-build-scratch.mjs status --id up063-mac
```

For an approved low-level Cargo step, a future build session may use this form, after separately
preparing the required frontend/resources and approving all downloads. This example compiles
only; it does not assemble, sign, package or verify a release:

```sh
node scripts/portable-build-scratch.mjs run --id up063-mac -- cargo build -p nano-nexus --release --features portable-dist
```

The command inherits the working checkout and environment, including any explicitly provided
credentials; neither environment values nor arguments are written into workspace metadata.
Output is inherited by the caller. Do not put secrets in command arguments or publish raw logs.
The wrapper keeps its active marker until the direct child closes and its remaining process
group is stopped. Interrupts stop that same group; unrelated processes are not targeted.
A child that deliberately escapes its group is outside this wrapper's coverage. Windows-native
execution of `run` is refused because equivalent process-tree ownership is not implemented;
Windows cross-builds use the Mac host.

Keep failed/cancelled workspaces for inspection or retry. Copy needed artifacts out before
cleanup; `clean` deletes the entire task-owned tree, including its outputs. Cleanup requires an
exact valid ownership marker, canonical repository-contained directories, no active marker and
no linked/special-file descendants (at most two million entries for bounded accounting). Cleanup atomically reserves active ownership so it cannot race a new run. It never deletes `vendor/`, `target/`, DIST, models, any
other task's workspace or user containers.

```sh
node scripts/portable-build-scratch.mjs clean --id up063-mac
```

After a wrapper crash, `recover --id up063-mac` removes only the active marker, and only when its
recorded parent PID and child process group no longer exist. PID reuse or inability to inspect
processes causes refusal. An unknown/incompletely recorded child spawn also refuses automatic
recovery: inspect processes manually rather than guessing. Malformed metadata fails closed.
Recovery preserves files and does not kill any process; cleanup remains a separate action.
The wrapper is not designed to resist malicious edits by another process with checkout access.

## Colima

No Colima VM, disk, container or profile was created for this conversion. Mac-native Rust and
cargo-xwin are the current proposed build route. The helper does not invoke Colima, Docker or
package managers itself and does not run `prune` or delete the default Colima profile.
If a future approved build truly requires Colima, UP.062 must first name a new task-specific
profile, its disk/CPU/RAM ceilings, image/volume/container identifiers, estimated downloads and
cleanup commands restricted to those exact resources. Shared/default environments must remain
untouched. Colima integration is deliberately excluded until that need and resource plan exist.
