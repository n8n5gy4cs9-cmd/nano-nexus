# Coding Conventions

## Rust
- **Edition 2024**, `rust-version = 1.90`, workspace lints (`unsafe_code = forbid`, `unwrap_used = deny`).
  Only the Windows Job Object FFI may use `unsafe`, isolated in `nexus-llm/src/win_job.rs` with
  `#[allow(unsafe_code)]` and a `// SAFETY:` comment, or prefer a safe crate wrapper.
- **Errors:** library crates define `thiserror` enums and map them to `nexus_core::ErrorCode` via
  `fn code(&self)`. The app returns `IpcError`. `anyhow` is allowed only in `xtask`/tests/examples.
- **Async:** tokio. Blocking work (fs walks, hashing, parsing, PDF) goes in
  `spawn_blocking`. Every long operation takes a `CancellationToken` and reports progress via a
  callback or channel. Never hold a lock across `.await` (use `tokio::sync` locks or restructure).
- **Logging:** `tracing` with structured fields (`info!(model = %id, "runtime ready")`).
  **Never log file contents, prompts, tokens, or secrets** above `debug`, and the debug level is off by default.
- **Modules:** one concept per file, files under ~400 lines, `pub(crate)` by default, and the
  public API re-exported in `lib.rs` with `///` docs.
- **Naming:** `snake_case` functions, `CamelCase` types, verbs for functions (`scan_folder`),
  nouns for types (`FolderReport`). IDs are newtypes (`ConversationId`).
- **Serde:** `#[serde(rename_all = "snake_case")]` for enums crossing IPC. Tagged enums use
  `#[serde(tag = "type")]`. Every IPC type also derives `specta::Type`.
- **Tests:** unit tests next to the code (`#[cfg(test)] mod tests`), integration tests in `tests/`.
  Use `tempfile` for filesystem tests, `insta` for snapshots (prompts, renders), `proptest` for
  invariants, and wiremock or axum for HTTP mocks. Real-model tests are `#[ignore]`.
- **Paths:** always `PathBuf`/`&Path`, never string concatenation. Windows: `dunce::canonicalize`.
- **Processes:** only via `nexus-llm` (llama-server) or explicitly allowed tools (`git` read-only
  in commit_message, the project's build checker in Rename Forge verify) with fixed argument lists, never through a shell.

## TypeScript / React
- `strict: true`, `noUncheckedIndexedAccess: true`. No `any`. Use `unknown` + type guards.
- Function components + hooks. Feature folders: `src/features/<area>/{components,hooks,api.ts}`.
- **IPC only through `src/lib/api.ts`** (Transport facade). ESLint `no-restricted-imports` forbids
  `@tauri-apps/api/core` in `src/features/**`.
- State: zustand for UI-only state, TanStack Query for anything that comes from Rust (query keys
  in `src/lib/queryKeys.ts`). Invalidate queries on the matching events.
- Styling: Tailwind utilities mapped to **tokens** (`bg-surface`, `text-muted`, `border-border`).
  No raw hex or oklch in components. Class merging via `cn()` (clsx + tailwind-merge).
- Accessibility: every interactive element is reachable by keyboard, icon buttons have
  `aria-label`, and focus rings are always visible.
- Responsive: components must work from 390 px wide (the mobile web UI reuses them).
- Tests: Vitest + Testing Library for pure logic and hooks, Playwright for e2e (M14+).

## Prompts (for the local model)
- Assistant prompts: `assistants/*.toml` `[prompt] system = '''…'''` (≤ 450 tokens).
- Internal prompts: `prompts/internal/*.md` with `=== section ===` markers (≤ 350 tokens of system text).
- Follow ASSISTANTS.md §2 rules R1–R16. Data goes in tags. Machine-read output uses a JSON schema.

## Git
- Branch: `main` (the owner's instruction). **One task = one commit = one push.**
- Subject: `<TaskID>: <imperative summary>` (≤ 72 chars). The body explains why if it isn't obvious,
  and lists any new dependency and its justification.
- Never commit generated files, `vendor/`, `*.gguf`, `bundles/local/*`, or `.env`.

## Docs
- Architecture docs are **contracts**. If you change behavior, update the doc in the same commit.
- Spike results go to `docs/SPIKES.md`, open questions to `docs/QUESTIONS.md`, and user-facing
  workflows to `guide.html`.
