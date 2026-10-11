# M05: Permissions, Change Sets, Journal

**Goal:** the safety foundation. Folder grants enforced in Rust, permission prompts, macOS TCC
awareness, an audit log, and change sets with diff preview, apply, and undo.

**Spec (normative):** [PERMISSIONS.md](../architecture/PERMISSIONS.md).
**Prereqs:** M04. **Exit demo:** grant `~/Downloads` (Read), deny `~/.ssh`, try a symlink escape
(blocked), apply a 3-file test change set from a debug command, undo it (bytes identical), and
see every step in the Activity log.

## Scope
- `nexus-fs`: `PermissionEngine`, deny list, canonicalization, `SafeFs` read helpers, `ChangeSet`,
  preview (diffs via `similar`), `Journal` apply/undo with rollback, Trash (`trash` crate),
  audit event type, OS probes (TCC EPERM detection), journal retention pruning.
- `nexus-store`: migration 0002 + repos (grants, deny_rules, audit_log, changesets).
- App: `AppState.perms`, permission-request broker (pending map: request_id → oneshot), commands
  (`fs_*`, `permission_respond`, `os_permissions_status`, `os_open_privacy_settings`,
  `audit_list`, `changesets_*`), and the events `permission:request` and `changeset:update`.
- UI: Permissions page (grants table, macOS privacy cards, protected locations, activity log),
  Permission modal, Changes page (list + preview + diff viewer + apply/undo/discard).

## Design notes
- **Broker:** `PermissionBroker::ask(req) -> Decision` inserts a `oneshot::Sender` into
  `DashMap<RequestId, Sender>`, emits `permission:request`, and awaits with a 5-minute timeout
  (→ Deny). `permission_respond` completes it. If the window is unfocused, also send an OS
  notification: "Folder Scout needs access to ~/Downloads".
- **Diff viewer (UI):** for text diffs, render unified hunks from Rust (`similar::TextDiff::unified_diff`)
  with Shiki-highlighted lines. Toggle split view with `@codemirror/merge` `MergeView` (read-only).
  Binary files show "binary change (size a → b)".
- **Undo conflicts dialog:** "This file changed after the change was applied." →
  [Keep current] [Restore original anyway] [Skip].
- **Journal layout:** `journal/<id>/manifest.json` + `files/<n>.orig` + `files/<n>.trashed`
  (copies of trashed files < 50 MB).
- **Property test** (`proptest`): random file trees + random edit sets → apply → undo → every
  file is byte-identical to the original.

## Steps
1. Grants and canonicalization + deny list + `check()` + exhaustive unit tests (PERMISSIONS §8).
2. Store migration 0002 + repos + engine persistence (load on startup, session grants in memory only).
3. Audit: an async batched writer (flush every 500 ms or 100 entries).
4. ChangeSet + preview + Journal apply/rollback/undo + Trash + property tests.
5. OS probes + deep links, with macOS TCC detection tested manually (document the steps in the test file header).
6. Broker + commands + events.
7. UI: Permissions page, Permission modal, Changes page with diff viewer.
8. Debug command `debug_make_test_changeset` (dev builds only) for the exit demo.

## Exit criteria
- [ ] All PERMISSIONS §8 tests pass, including the proptest (≥ 256 cases).
- [ ] Manual TCC test: revoke NanoNexus under Files & Folders → status shows "denied" with a working deep link.
- [ ] Undo after apply restores originals byte-for-byte. Conflict path tested.
- [ ] Nothing in the codebase writes files outside `Journal::apply`, the app's data dir, or
      the download temp dir (grep check `std::fs::write|File::create` documented in the review checklist).

## Pitfalls
- `std::fs::canonicalize` fails on non-existent paths. Use the ancestor algorithm.
- macOS path case: canonicalize returns the on-disk case. Always compare canonical paths.
- `trash::delete` on macOS uses Finder or NSFileManager. It's slow for many files, so batch with
  `trash::delete_all` and show progress.
- Cross-volume `rename` fails (EXDEV). Fall back to copy + verify + trash the source.
