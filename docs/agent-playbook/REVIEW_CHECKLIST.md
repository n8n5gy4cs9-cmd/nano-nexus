# Review Checklist (run through it before every commit)

Copy the relevant sections into your head and check each line against your diff.

## Always
- [ ] The diff does **only** the current task. No drive-by refactors.
- [ ] `scripts/check.sh` is green (fmt, clippy `-D warnings`, tests, lint, typecheck, vitest).
- [ ] New logic has tests, and every bug fix has a test that fails without it.
- [ ] No `unwrap()`/`expect()` outside tests. No `any` in TS. No `console.log` left behind.
- [ ] No TODO without a task ID (`TODO(T08.12)`).
- [ ] No secrets, tokens, absolute personal paths, or `vendor/`/`*.gguf` files staged.
- [ ] Contracts and docs are updated if behavior or names changed (architecture docs, guide.html).
- [ ] `tasks.md` is ticked with the date. The progress block is refreshed (`scripts/progress.sh --write`).
- [ ] The commit subject is `<TaskID>: <imperative summary>`. The commit is pushed to `main`.

## Rust
- [ ] Errors map to an `ErrorCode`. User-facing messages are friendly, and the debug detail goes in `detail`.
- [ ] Long operations take a `CancellationToken` and report progress. Blocking work runs in `spawn_blocking`.
- [ ] No lock is held across `.await`.
- [ ] Logs don't contain file contents, prompts, tokens, or secrets at info level or above.
- [ ] Public items have `///` docs. IPC types derive `Serialize`/`Deserialize`/`specta::Type`.
- [ ] Windows: `cfg(windows)` branches compile (no Unix-only APIs leak into shared code).

## Files and safety (any task touching the filesystem)
- [ ] Every read goes through `PermissionEngine::check` / `SafeFs`.
- [ ] Every write goes through `Journal::apply` (grep your diff for `fs::write`, `File::create`, `rename`, `remove_`).
- [ ] Paths are canonicalized before comparison. No string-prefix path checks.
- [ ] Deletes go to Trash (`trash` crate). Never `remove_file` on user data.
- [ ] Tools never get write access, and file content reaches the model inside tags.

## Inference and prompts
- [ ] Requests to llama-server go through the `Scheduler` with the right priority.
- [ ] Machine-read outputs use a JSON schema, `Validate`, one reask, and a deterministic fallback.
- [ ] Thinking is off for JSON calls. Reasoning is never sent back in history.
- [ ] Prompts stay within budget (system ≤ 450 / internal ≤ 350 tokens), and data sits in tags.

## Frontend
- [ ] No direct `invoke` in feature code. Everything goes through `src/lib/api.ts`.
- [ ] Only design tokens (no raw colors). Both light and dark themes are checked.
- [ ] Works at 390 px wide (the mobile web UI reuses components).
- [ ] Keyboard reachable, visible focus, `aria-label` on icon buttons, reduced motion respected.
- [ ] Loading, empty, and error states exist for every new view.
- [ ] Generated `bindings.ts` is regenerated, not hand-edited.

## LAN Share (M14+)
- [ ] New remote-callable commands are added to the allow-list deliberately and policy-checked.
- [ ] No endpoint returns user data without a valid device session.
- [ ] llama-server is still bound to 127.0.0.1 only.

## Build and bundling (M12+)
- [ ] Locks (runtime and models) are verified before staging, and `verify-payload` runs after the build.
- [ ] Nothing large is committed. Generated overlays live in `src-tauri/gen/`.
- [ ] `guide.html` reflects any user-visible change in build steps.
