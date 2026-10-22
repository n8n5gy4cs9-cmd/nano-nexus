# Latest session boundary — 2026-10-22

Four implementation commits plus T06.24 evidence audit on main in this session: T07.12 safe model-reference removal,
T07.13 reference-first imports + Read consent, T06.19 isolated specialist assignment/bounded
refusal recovery + stronger semantic/manual Apply/Undo tests, T06.23 deterministic percentage
calculator. Full scripts/check.sh passed after final source changes: 81 app tests (2 ignored),
153 agent unit tests, 151 frontend tests; `/tmp/nano-nexus-check-percentage-final.log`.
T06.24 corrected the recorded count and repeated the full suite successfully:
`/tmp/nano-nexus-check-evidence.log`.

Important limit: overall small-model reliability is NOT passing. Focused Reviewer test passed
3/3 each model, and wider Gemma passed 3/3, but subsequent MiniCPM full-service still promised
a review without identifying the defect. Earlier MiniCPM also lost the dot in a temp folder
path; permissions correctly denied the wrong path. Missed percentage routing is now locked
down by a real-service exact-result test with no planning model calls. All failures are retained
in docs/SPIKES.md/new-tasks.md; never weaken their assertions or permission boundaries.

Continue T06.20 quality/path fidelity first, then T07.14 model defaults/Auto/tuning/custom GGUF
handling/help, T06.21 evaluated optional skills, T06.22 actual project-folder coding/verification,
T18.02/.04/.05 Duo, T15.12 hardware performance. These are unfinished, not declared complete or
artificially blocked. Session limits stopped further implementation; original requirements
remain in new-tasks.md and mapped tasks.md entries. No model copies/downloads, production DB
changes, branches or PRs. Existing untracked .agents/ is preserved. The owner's previously
uncommitted project-coding addition was preserved in T07.12's new-tasks.md update.

Local REFE/pi-main and REFE/opencode-dev MIT licenses and actual skill-discovery/subagent/
write/edit implementations were inspected. No REFE code was bundled/copied in these changes.
The remaining project/skills work must adapt real behavior under Journal.apply and scoped
permissions; it is not satisfied by the consultation prompt repair.

## Specialist reliability repair — 2026-10-22

T06.19 adds a dedicated text-consultation preamble and final assignment, keeping the original
task verbatim. Known refusal/self-delegation gets one isolated correction then explicit failure.
No child tools/history/authority added. Semantic tests require actual empty-input review finding;
manual Apply/Undo asserts exact bytes. Full check passes (80 app tests, 2 ignored; 151 frontend).
Focused Reviewer 3/3 each model. Wider Gemma scenario 3/3; intermediate MiniCPM only 1/4:
wrong temp-folder dot denied correctly, self-delegation, and missed calculator call. Keep these
failures; broader reliability is not complete. See docs/SPIKES.md and new-tasks.md for logs.
Next: T06.20, beginning with deterministic percentage routing and path fidelity. Remaining
model defaults/controls, REFE skills, project coding, Duo, older-PC/performance work is mapped
in tasks.md without deleting original requirements. No model download/copy or real DB changes.

## Copy-free import completion — 2026-10-22

T07.13: local-file/pack imports default to reference, with explicit copy and existing Read
broker consent before parsing. Per-member pack checks preserve nested denies. Full check
passes: 78 app tests + 151 frontend tests; `/tmp/nano-nexus-check-copy-free-final.log`.
No model duplicated/downloaded, no production database changes. Next: reliable small-model
behavior/defaults, optional evaluated skills, project workflow, Duo and performance work in
new-tasks.md. The owner's project-coding addition remains intact.

## Continuation — 2026-10-22

T07.12 repairs external-model removal (including legacy rows): explicit copy/reference
metadata plus canonical app-model-root containment. Unknown/missing/external/symlink-escape
paths unlink; bundled protection and active guard remain. Temporary GGUF regressions and
full scripts/check.sh pass (76 app tests, 148 frontend tests; log
`/tmp/nano-nexus-check-model-ownership.log`). Doctor: only signing identity warning.
Preserved the owner's uncommitted project-coding requirements in new-tasks.md and untracked
.agents/. Next: T07.13 copy-free import dialog/Read consent, then remaining new-tasks scope.
Pi and OpenCode local MIT licenses and actual skill discovery/write/edit implementations
were inspected; no reference implementation code copied in T07.12.

# NanoNexus repair handoff — 2026-10-22

## Phase outcome

Repairs were implemented directly on main using local `REFE/pi-main` agent-loop and
subagent examples as behavioral references. Native Rust ports preserve Journal.apply,
permission checks, scheduler ownership and loopback-only runtime access. No dependencies added.
The user requested closing this phase with remaining work documented due usage limits.

Completed commits before this final T06.18 commit:

- `34847b1` T06.14: real tool observations feed subsequent agent decisions; exact edit whitespace.
- `4e0c81c` T06.15: recent tool observations survive context pressure and large attachments.
- `2b81d51` T18.06: Planner/Reviewer built-ins and bounded isolated ask_assistant consultations.
- `bbde86f` T06.16: per-action required-argument schemas and real service regression test.
- `24214e5` T06.17: calculator/clock routing and bounded incomplete file-task recovery.
- T06.18: factual operation receipts and verbatim explicit specialist task/answer handling.

There are 22 built-in assistants. Specialist children have no tools or shared history;
consultations are cancellable, sequential and use the active model. True concurrent
two-model Duo is unfinished. Assistant reasoning quality is not uniformly passing.

## Models and disk constraints

Only the default MiniCPM was downloaded (~1.1 GB, SHA verified):
`vendor/models/minicpm5-1b-fable5-v2-q8/MiniCPM5-1B-Claude-Opus-Fable5-V2-Thinking-Q8_0.gguf`.
The dev default was set through `scripts/fetch-model.mjs`.

Use Gemma directly at:
`/Users/harriahola/Downloads/gemma-4-E4B-it-ultra-uncensored-heretic-Q5_K_M.gguf`.
No Gemma copy or third Qwen download was made. User removed duplicate model bundles due SSD
space. Do not recreate them. The production app database still contains three stale model
paths under deleted dist-bundles files; it was inspected read-only and not changed.
Fix external reference removal before configuring those references in the production app.

## Verification

`scripts/check.sh` passed after the final source changes: Rust fmt/clippy/tests, prompt lint,
frontend lint/typecheck and 148 frontend tests. Agent tests: 149; app tests: 74 plus one ignored.
Full output: `/tmp/nano-nexus-check-receipts.log`.

Run the real service scenario from the repository root (requires a loopback server):

```sh
NEXUS_MODEL=/absolute/path/to/model.gguf cargo test -p nano-nexus --lib real_model_agent_and_duo_apply_files_and_consult_a_specialist -- --ignored --test-threads=1 --nocapture
```

Check the app crate package name in Cargo.toml if the command's package selector changes.
The test uses an isolated database, temporary granted folder and test-only automatic Apply.
It verifies create/read-edit exact bytes, Reviewer consultation, calculator 441, actual UTC
clock and Duo creation. Both models passed: MiniCPM 9.93 s, Gemma 59.05 s. Logs:
`/tmp/nano-nexus-minicpm-receipts.log`, `/tmp/nano-nexus-gemma-receipts.log`.
No production grants/settings were modified and no app bundle was installed.

Golden evaluation: `scripts/eval-assistants.sh --json /tmp/nano-nexus-assistant-eval.json`.
Captured repeat: `/tmp/nano-nexus-assistant-eval-captured.json` and
`/tmp/nano-nexus-golden-responses.jsonl`. Temporary logs may disappear; the durable findings
are in `docs/SPIKES.md` and `new-tasks.md`. Do not weaken golden assertions.

## Next work and boundaries

Start with the external-model reference deletion bug in `new-tasks.md` using temporary
fixtures only, then copy-free import UI and real two-model switching. Read the applicable
milestone/contracts and skills, add planned tasks without rewriting completed history,
verify, commit and push each task to main. Never commit models/vendor/build outputs.

Pre-existing local `.gitignore` changes and untracked `.agents/` were preserved and excluded
from repair commits. Generated bindings were not hand-edited. Q10/Q11 in QUESTIONS and
ASSISTANTS/M06/M18 document model locations, schema rationale and consultation behavior.

---

## Historical handoff (superseded; retained for context)

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
