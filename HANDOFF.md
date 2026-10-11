# Handoff (session ending 2026-10-11)

Read `CLAUDE.md` / `AGENTS.md` first. Work directly on `main`, one task = one commit, no PRs, no model names in commits.
Use `scripts/progress.sh` for the next task. **Run clippy and tests BEFORE committing** (I twice committed before clippy passed).

## State
- `main` is pushed up to `1bf6798` (T09.08). **T09.09 is committed as WIP (see last commit) and NOT yet verified**:
  first run `cargo fmt -p nexus-analyze -p nexus-agent && cargo clippy -p nexus-analyze -p nexus-agent --all-targets -- -D warnings && cargo test -p nexus-analyze -p nexus-agent`, fix anything (last known: two `type_complexity` lints were just patched with `type GroupKey` / `type Answer`), then push.
- Only library crates build in the sandbox (no GTK): the app crate (`apps/desktop/src-tauri`) and the frontend (no node_modules) are NOT compiled or tested here. Anything written there is unverified until the owner builds on the Mac.

## Done this session
1. **Duo assistant v0.1 + M18 tasks** (T18.01 done; T18.02–05 open). `assistants/duo.toml`, `nexus-agent::duo` (heuristic tier classifier), settings `duo.light_model_id` / `duo.strong_model_id`, per-message model swap in `service/chat.rs` + `service/runtime.rs` (`duo_model_for`, `ensure_model`), `DuoPanel.tsx` on the Models page. One model runs at a time (swap on demand). Spec: `docs/milestones/M18-duo.md`.
2. **Library "Ask in chat"** button (Library page → selects Librarian, sends the question via `useAssistantChoice.pendingQuestion` consumed in `ChatView`). Answer to the user: Library → add folder → wait for indexing → type question → "Ask in chat" (or use the "Libraries" chip in any chat).
3. **Windows Downloads bug**: router prompt now uses forward-slash paths (backslashes broke the JSON example so the model emitted garbage like `C:")</param>…`), and `resolve_path` cuts markup off model-written paths. User must also grant the Downloads folder in Permissions.
4. **M09 Rename Forge**: T09.01–T09.08 done and pushed; T09.09 written (see above).
   - `crates/nexus-analyze/src/code/`: `lang`, `tags` (+ `queries/<lang>/{tags.scm,keywords.txt,globals.txt}`), `file_facts`, `index` (build, postcard cache, incremental), `imports` (py/ts/rust/go/java resolution), `noncode` (config/string/doc/comment refs; `Location.file >= OTHER_BASE` = non-code file, use `CodeIndex::path_of`), `reserved`, `rules` (naming), `propose` (`<sym>` blocks + `Validator`).
   - `crates/nexus-agent/src/pipelines/rename.rs`: `propose()` (rule fixes, batches ≤ 20, reask, fallback).
   - Fixtures: `tests/fixtures/{py,ts,rust,cs,go,java}_app`. Snapshots: `crates/nexus-analyze/tests/snapshots` (insta; `INSTA_UPDATE=always` then review).
   - Deps pinned in workspace: tree-sitter + 7 grammars, postcard, heck.

## Next (in order)
- Verify/push T09.09, then **T09.10** planner → ChangeSet (byte-range edits per ref kind, file/folder `Move`s applied before edits, case-only renames via temp name, overlap + collision checks, git-dirty detection `git status --porcelain`; Doc/Comment refs only when opted in; ambiguous/possible refs unchecked by default; PUBLIC_API default unchecked). Look at `nexus_core::changes::{ChangeSet, FileOp}` and `crates/nexus-fs::Journal`. Exit criteria in `docs/milestones/M09-rename-forge.md` (folder rename updates imports, undo byte-identical, project still builds).
- T09.11 find + `find_symbol` router tool, T09.12 verify, T09.13 strings mode, T09.14 migration 0005 + `rename_*` commands + jobs (note `registry.rs:140` still says rename_propose "Available in M09"), T09.15/16 UI, T09.17 🍎, T09.18 (P1 languages), T09.19 exit check.
- Then **M11** (T11.01–T11.08: JobRegistry, jobs UI, notification prefs (settings `notifications.kinds` already exist), watches, tray) — not started.
- Remaining Duo tasks T18.02–T18.05; open Windows-bundle items from the older handoff (rebuild Windows after the `macOSPrivateApi` fix; M12/M13 leftovers; Q-6, Q-7 in `docs/QUESTIONS.md`).

## Known gaps / notes
- Index limitations: name+scope resolution only; member refs on unknown receivers attach to every candidate (flagged ambiguous when >1); C# has no file/folder import mapping; `typo` naming rule not built; Rust inline `crate::a::f()` paths resolve only through `mod`/`use` of the same file.
- `Helper_Funcs`-style case-only file fixes go through the deterministic rule (Go folder `util_funcs` → `utilfuncs`).
- Never claim frontend/app-crate work is verified; say so in task notes.
