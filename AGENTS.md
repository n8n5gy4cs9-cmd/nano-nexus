# AGENTS.md: Rules for AI Coding Agents Working on NanoNexus

You are continuing a carefully planned project. **The plan is the product.** Follow it exactly,
work in small verified steps, and leave the repo better documented than you found it.

## 1. Sources of truth (read in this order)
1. `tasks.md` (root): what to do next. Pick the **first unchecked task** whose milestone prerequisites are met.
2. `docs/milestones/Mxx-*.md`: the spec for that task's milestone (scope, design notes, steps, exit criteria, pitfalls).
3. `docs/architecture/*.md`: **contracts** (inference, bundling, permissions, assistants, analyzer, models, data/IPC, LAN share).
4. `docs/DECISIONS.md`: ADRs. Never contradict one silently.
5. `docs/design/UI-DESIGN.md`: tokens, layouts, components (for any UI task).
6. `docs/agent-playbook/`: conventions, gotchas, review checklist, prompts.

If sources disagree, the more specific one wins (milestone > architecture > PRD), **except**
ADRs and safety rules, which always win. Record the conflict in `docs/QUESTIONS.md`.

## 2. The work loop (one task at a time)
1. `scripts/progress.sh`: confirm the next task. Read its milestone doc section.
2. Write a 3–6 line plan (files to touch, tests to add) in your scratch notes (not in the repo).
3. Implement the **smallest change** that satisfies the task and its tests.
4. Verify: `scripts/check.sh` (fmt, clippy -D warnings, tests, frontend lint/typecheck/tests).
   Run the task's specific verify step if it has one.
5. Tick the task in `tasks.md`: `- [x] **T04.07** … (YYYY-MM-DD)`, plus a short note if something deviated.
   Run `scripts/progress.sh --write` to refresh the progress block.
6. **Commit and push to `main` immediately after every task:**
   ```bash
   git add -A && git commit -m "T04.07: stream chat deltas over Channel" && git push origin main
   ```
   One task = one commit. The subject starts with the task ID. If a push fails because of the network,
   retry up to 4 times with backoff (2 s, 4 s, 8 s, 16 s).
7. If you are blocked: add `⛔ blocked: see QUESTIONS Q-n` to the task line, write the question in
   `docs/QUESTIONS.md` (context, options, recommendation), commit + push, and move to the next
   unblocked task.

## 3. Hard rules (never break these)
- **Never** commit `vendor/`, `*.gguf`, model files, build outputs, secrets, or `bundles/local/*`.
- **Never** write files outside `nexus-fs::Journal::apply` (except the app data dir, caches, and
  download temp files). The LLM never gets a write tool (ADR-005).
- **Never** weaken the permission engine: canonicalize first, the deny list always wins, use
  component-wise `starts_with`, and run the tests in PERMISSIONS §8.
- **Never** expose `llama-server` beyond `127.0.0.1`, and never send its token to the webview or LAN clients.
- **Never** make LAN Share endpoints work without pairing auth (LAN-SHARE §3).
- **Never** call `invoke` directly in feature code. Use `src/lib/api.ts` (Transport facade).
  Tauri commands are thin wrappers over `service::*` (M04 rule).
- **Never** hand-edit generated files (`src/bindings.ts`, `src-tauri/gen/*`).
- **Never** add a dependency without pinning it in the workspace `[workspace.dependencies]` or
  `package.json` and noting why in the commit body. Never upgrade Tauri to 3.x (ADR-001).
- No `unwrap()`/`expect()` in non-test Rust code. No `any` in TypeScript (use `unknown` + narrowing).
- Don't delete or rewrite planning docs to make a task "fit". Propose changes in QUESTIONS.md.

## 4. Definition of Done
See `docs/PLAN.md` §4. In short: it builds, `scripts/check.sh` is green, new logic is tested,
docs and contracts are updated if behavior changed, the task is ticked, and the commit is pushed to `main`.

## 5. Environment notes
- Primary dev machine: **macOS on Apple Silicon** (Metal). Windows builds are **cross-compiled on
  the Mac** (cargo-xwin + NSIS). See M13 and BUNDLING §7.
- Tasks marked 🍎 need the Mac, and 🪟 needs a Windows VM or PC. If you're in a Linux container, do
  everything else (unit tests, docs, frontend) and leave 🍎/🪟 steps for the owner, noting what remains.
- Tests that need the real model are `#[ignore]` and run with `cargo test -- --ignored` on the Mac.

## 6. Style in one breath
Rust 2024, `thiserror` errors with `ErrorCode`, `tracing` for logs (never log file contents or
tokens at info level), small modules, `///` docs on public items. TypeScript strict, function
components, zustand for UI state, TanStack Query for IPC data, Tailwind with design tokens only
(no raw hex colors). Full details: `docs/agent-playbook/CONVENTIONS.md`.
