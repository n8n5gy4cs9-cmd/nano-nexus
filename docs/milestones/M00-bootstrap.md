# M00: Bootstrap

**Goal:** an empty but correctly structured repo where `scripts/check.sh` passes, so every later
task has a place to land and a way to verify itself.

**Prereqs:** none. **Exit demo:** `scripts/doctor.sh && scripts/check.sh` both green on the Mac.

## Scope
- Cargo workspace with 7 library crates (empty but compiling) and shared dependency pins.
- `nexus-core` fully implemented (it's small and everything depends on it).
- pnpm workspace root (the frontend app itself comes in M03).
- `scripts/check.sh`, plus optional CI.

## Out of scope
The Tauri app (M03), any AI code (M02).

## Design notes

### Workspace `Cargo.toml`
```toml
[workspace]
resolver = "3"
members = ["crates/*"]            # apps/desktop/src-tauri is added in M03
[workspace.package]
edition = "2024"
rust-version = "1.90"
license = "MIT"                   # your choice; personal
version = "0.1.0"
[workspace.lints.rust]
unsafe_code = "forbid"
[workspace.lints.clippy]
unwrap_used = "deny"              # tests opt out with #[allow] or via clippy.toml allow-unwrap-in-tests
expect_used = "warn"
dbg_macro = "deny"
todo = "warn"
[workspace.dependencies]          # pin with `cargo add -p … --precise` style; versions as of 2026-10
tokio = { version = "1.53", features = ["rt-multi-thread", "macros", "process", "sync", "time", "fs", "io-util"] }
tokio-util = "0.7.20"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
thiserror = "2.0"
uuid = { version = "1.28", features = ["v7", "serde"] }
time = { version = "0.3", features = ["serde", "formatting", "macros"] }
tracing = "0.1"
specta = { version = "=2.0.0-rc.25", features = ["derive"] }   # S6 may switch to ts-rs (ADR-006)
# … each later milestone adds its crates here, never in member Cargo.tomls directly
```
`clippy.toml`: `allow-unwrap-in-tests = true`, `allow-expect-in-tests = true`.

### `nexus-core` contents
```
crates/nexus-core/src/
  lib.rs        pub mod ids; pub mod error; pub mod paths; pub mod settings; pub mod events;
  ids.rs        macro_rules! id_type { ($name:ident) => { #[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)] #[serde(transparent)] pub struct $name(pub String); impl $name { pub fn new() -> Self { Self(uuid::Uuid::now_v7().to_string()) } } } }
                id_type!(ConversationId); MessageId; AssistantId (not UUID: kebab id); GrantId; ChangeSetId; JobId; ReportId; ModelId; DownloadId; NotificationId; WatchId; RenameSessionId
  error.rs      ErrorCode enum (see DATA-AND-IPC §4), NexusError (thiserror) with fn code(&self) -> ErrorCode, IpcError {code, message, detail} + From<NexusError>
  paths.rs      AppPaths { data, models, journal, reports, runtime, cache, logs } ; AppPaths::from_roots(data_dir, log_dir) ; ensure_dirs()
  settings.rs   Settings struct (all keys from DATA-AND-IPC §1, typed, Default impl), SettingsPatch
  events.rs     RuntimeState (defined here so app and llm share it), JobSummary, NotificationPayload
```
Tests: id uniqueness and ordering (v7 sorts by time), error code serialization (`"permission_denied"`),
settings defaults round-trip through JSON, `ensure_dirs` creates the tree in a `tempfile::TempDir`.

### `scripts/check.sh`
```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
[ -d apps/desktop/node_modules ] && pnpm -C apps/desktop lint && pnpm -C apps/desktop typecheck && pnpm -C apps/desktop test --run
```

## Steps
1. Create the root `Cargo.toml`, `rust-toolchain.toml` (`channel = "stable"`, components `rustfmt`,
   `clippy`), `rustfmt.toml` (`max_width = 110`, `imports_granularity = "Crate"` only if nightly;
   keep stable options), and `clippy.toml`.
2. `cargo new --lib crates/nexus-{core,store,llm,models,fs,analyze,agent}`. Each `Cargo.toml` uses
   `version.workspace = true`, `edition.workspace = true`, `[lints] workspace = true`.
3. Implement `nexus-core` as above, with tests.
4. Root `package.json` (`"private": true`, `"packageManager": "pnpm@<current>"`, scripts
   `dev`/`build`/`check` that proxy to `apps/desktop`), `pnpm-workspace.yaml` (`packages: ["apps/*"]`),
   `.nvmrc` = `22`, `.editorconfig`.
5. `scripts/check.sh` (executable). Run it.
6. Optional: `.github/workflows/check.yml` on `macos-14` (arm64) running `scripts/check.sh`
   (no runtime or model needed; those tests are `#[ignore]`).

## Exit criteria
- [ ] `scripts/check.sh` passes locally.
- [ ] `cargo doc -p nexus-core --no-deps` builds without warnings.
- [ ] `tasks.md` M00 boxes ticked.

## Pitfalls
- Edition 2024 needs Rust ≥ 1.85. We pin rust-version 1.90 because Tauri 2.12 requires it.
- Don't add `tauri` to any library crate (OVERVIEW §3 rule).
