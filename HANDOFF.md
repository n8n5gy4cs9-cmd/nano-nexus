# Handoff (session ending 2026-10-11, second session)

Read `CLAUDE.md` / `AGENTS.md` first. Work directly on `main`, one task = one commit, no PRs, no model names in commits.
Use `scripts/progress.sh` for the next task. **Run fmt, clippy and tests BEFORE committing.**
Pushing: `git push origin HEAD:main`. A stop hook also wants the session branch pushed
(`git push -u origin claude/bold-keller-vud8rs`); both point at the same commit.

## State
- `main` is pushed through `7b67c65` (T11.01). The working tree is clean. Everything below is committed.
- Verified in the sandbox (fmt, `clippy -D warnings`, tests): `nexus-analyze`, `nexus-agent`, `nexus-store`.
  Command: `cargo fmt -p nexus-analyze -p nexus-agent -p nexus-store && cargo clippy -p nexus-analyze -p nexus-agent -p nexus-store --all-targets -- -D warnings && cargo test -p nexus-analyze -p nexus-agent -p nexus-store`
- NOT verifiable here (no GTK, no node_modules): the app crate `apps/desktop/src-tauri` and the frontend.
  `service/rename.rs`, `commands/rename.rs` and their registration (`bindings.rs`, `state.rs`, `lib.rs`) were written
  but never compiled. **First thing on the Mac: `cargo check -p nano-nexus` and fix whatever breaks.** Never claim them verified.
- `cargo fmt` on `nexus-store` reformats `reports.rs` and `share.rs` (pre-existing drift). Revert those two files
  after formatting (`git checkout`) so commits stay focused, or format the crate in its own commit.

## Session 3 summary (2026-10-11) — read this first
Owner asked for NO builds/tests in the second half of the session. Everything in the app crate (`apps/desktop/src-tauri`)
and the frontend was written blind. **First thing on the Mac:** `git pull`, `cargo check -p nano-nexus`,
`cargo test -p nano-nexus`, `pnpm -C apps/desktop lint && pnpm -C apps/desktop typecheck && pnpm -C apps/desktop test`,
then regenerate `bindings.ts` and run `scripts/check.sh`. Expect small compile/type fixes. Two build/start breakages already
happened and were fixed: duplicate TS name `VerifyResult` (Rename Forge's is now `RenameVerifyResult`), and a startup abort
from `tokio::spawn` in the Tauri setup hook (use `tauri::async_runtime::spawn`; see `service::watches::run_scheduler`).

### Done this session
- M11: T11.02 jobs wiring (`AppState.jobs`, `job:update`, `jobs_list`/`job_cancel`; scan, index, propose, download mirrored; narrative stays a chat turn),
  T11.03 Jobs indicator, T11.04 notification routing + quiet hours, T11.05 watches backend (`nexus-analyze::watch`, `nexus-store::watches`,
  `service/watches.rs`, `notify` 8.2), T11.06 watched-folders UI.
- M18: T18.03 Duo tier badge + "Use strong model". Duo swaps only when light/strong models are chosen AND installed.
- Settings page (`features/settings/SettingsPage.tsx`, all settings except LAN Share details which live at `/settings/sharing`).
- M09 UI: T09.15/T09.16 Rename Forge page (`features/rename/*`): start view, suggestions, review, usages, find, check, plan -> Changes.
  Fixed channel arg names (`on_event`) in rename commands; items become `applied` when the change set is applied (`rename::on_changeset_status`).
- Rename Forge chat assistant is now selectable (`pipeline_ready` returns Ok); real work is on the `/rename` page.
- Librarian: the first hop now ALWAYS runs `kb_search` with the user's own words (router no longer asks the 1B model).
- App Builder (`node scripts/app-builder.mjs`): fixed port 8787 + `bundles/local/app-builder-state.json`; saves only after restore, also on build.

### NOT finished — Rename Forge (owner has asked for this repeatedly; do it next)
1. **Strings tab + commands** (T09.13 backend exists: `nexus-analyze::code::strings` `extract_strings`, `validate`, `export_csv/json`,
   `plan_key_rewrite`; `nexus-agent::pipelines::strings::propose`). Missing: service + commands (`rename_strings`, `rename_strings_propose`,
   `rename_strings_export`, `rename_strings_apply` -> change set via `plan_key_rewrite`), a Strings tab in `RenamePage`, store table or in-memory state.
2. **`find_symbol` data source**: implement `nexus_agent::tools::CodeSource` over `RenameRuntime.indexes` (`find_symbols`/`render_find`), set `ToolCx.code`
   in `service/chat.rs`, add `find_symbol` to an assistant's `allowed` list (rename-forge.toml is a pipeline assistant; a router assistant may be needed).
3. Planner integration test through `nexus-fs::Journal::apply` + undo (byte-identical; M09 exit criterion). `PlanOptions` lacks `typo`/`dynamic_refs`.
4. UI: list is paged (200 rows), not virtualized; no "Overview" tab; no component tests; after applying a plan the user must reopen the project.
5. T09.17 (real run on a copy of a project, Mac), T09.18 (more languages), T09.19 (exit check).
6. Known index limits: name+scope resolution only; member refs on unknown receivers attach to every candidate; C# has no import mapping.

### Other open items
- T11.07 tray + autostart (optional, needs plugin deps), T11.08 and T18.02 (Mac), T18.04/T18.05 (Duo v0.2), "Watch this folder" button on a report page.
- Library retrieval: a small doc was answered "not in the library" by Nexus+Libraries. Suspect chunking merges short sections under the first heading
  (`nexus-knowledge/src/chunk.rs`, new heading only starts a chunk once the current one is a third full) and `max_per_doc = 2`. Investigate with a real
  Reaper-like fixture; consider heading-per-chunk for short sections and a `daw`-style low-value term filter. Not verified.
- Library questions could count as "hard" for Duo (route to the strong model). Not done.
- Windows: shared code paths were reviewed, nothing Windows-specific was built this session.

## Done this session
- T09.09 verified (was WIP) · T09.10 planner · T09.11 find · T09.12 verify · T09.13 strings mode · T09.14 store/service/commands · T11.01 JobRegistry.
- `crates/nexus-analyze/src/code/`: `plan` (`plan_renames`, `git_dirty`, `parse_porcelain`), `find` (`find_symbols`, `usages`,
  `overview`, `render_find`), `verify` (`detect`, async `run`), `strings` (`extract_strings`, `placeholders`, `validate`,
  `fallback_key`, `export_csv/json`, `plan_key_rewrite`). Tests in `crates/nexus-analyze/tests/code_{plan,find,verify,strings}.rs`.
  Fixture `tests/fixtures/strings_app`.
- `crates/nexus-agent`: `pipelines::strings::propose`; `tools::FindSymbol` + `CodeSource` trait + `ToolCx.code`
  (router resolves `find_symbol`).
- `crates/nexus-store`: migration **`0007_rename.sql`** (the docs say 0005; 0005/0006 are share/knowledge), `rename.rs`
  (sessions + items CRUD), `jobs.rs` (`JobRegistry`, job CRUD, `recover_interrupted_jobs`, `prune_jobs`).
- Dependencies added (already pinned in the workspace): `tokio` in nexus-analyze, `tokio-util` in nexus-store.

## Next (in order)
1. **T11.02** Migrate scan / download / propose / narrative to jobs. This is also where `JobRegistry` gets wired into
   `AppCtx`, the `job:update` event and `jobs_list` / `job_cancel` commands (not done yet). `RenameRuntime` in
   `service/rename.rs` has its own job-cancel map; move it onto the registry.
2. **T09.15 / T09.16** Rename Forge UI (stepper, issues grid, occurrence drawer, plan/preview, git banner, verify panel,
   strings tab, find tab). Commands available: `rename_session_create`, `rename_sessions`, `rename_session_delete`,
   `rename_items`, `rename_propose`, `rename_item_update` (validates, returns `error`), `rename_plan`, `rename_checkers`,
   `rename_verify`, `rename_usages`, `rename_find`, `rename_cancel`. Frontend is unverified: say so in task notes.
3. T09.17 🍎 real run on a copy of a project (Mac only) · T09.18 more languages (needs grammar crates) · T09.19 exit check.
4. Then T11.03–T11.08 (jobs UI, notification prefs, watches, tray), remaining Duo tasks T18.02–T18.05, and the open
   Windows-bundle items (rebuild after the `macOSPrivateApi` fix, M12/M13 leftovers, Q-6, Q-7 in `docs/QUESTIONS.md`).

## Known gaps in what was written
- Not exposed as commands yet: strings mode (extract / propose / export / `t("KEY")` rewrite) and an overview command.
- Items are not marked `applied` after the rename change set is applied. Hook `changeset:update` and call
  `Store::bulk_set_rename_status(session, "accepted", "applied")`.
- `find_symbol` has no data source in the app: implement `CodeSource` over the loaded session index (`RenameRuntime.indexes`)
  and put the tool into a router assistant's `allowed` list.
- Planner: tested with a stand-in applier, not against `nexus-fs::Journal::apply`. Add an integration test that applies
  a plan through the journal and undoes it (byte-identical, exit criterion of M09). `PlanOptions` has no `typo` or
  `dynamic_refs` handling beyond the `PUBLIC_API` / `RESERVED` flags.
- Strings: template literals, f-strings, C# `$"…"` and Rust raw strings are skipped. Overview omits the naming-issue summary.
- Index limitations from before still hold: name+scope resolution only; member refs on unknown receivers attach to every
  candidate (flagged ambiguous when >1); C# has no file/folder import mapping; no `typo` naming rule.
- Verify needs Write access to the project (builds create files there); it never runs automatically.
