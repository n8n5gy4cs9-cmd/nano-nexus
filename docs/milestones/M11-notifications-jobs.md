# M11: Notifications, Jobs Center, Watches

**Goal:** long-running work is visible and controllable, notifications are smart and
configurable, and folders can be watched with change reports.

**Spec:** PRD F9, F6.7; ANALYZER §4; DATA-AND-IPC (`jobs`, `notifications`, `watches`).
**Prereqs:** M08 (plumbing from M03). **Exit demo:** start a scan, a download, and a rename
proposal batch → all appear in the Jobs panel with progress → cancel one → the others finish →
OS notifications arrive while the window is in the background. Add a weekly watch on Downloads
→ "Run now" → a delta notification.

## Scope
- `JobRegistry` (app): create/update/finish/cancel, persisted to `jobs`, `job:update` events
  throttled to 4 Hz, recovery on startup (running → failed "interrupted by restart").
- Jobs panel UI: a title-bar indicator (spinner + count) → popover list with progress bars,
  cancel, and open-result buttons.
- Notification preferences per kind (Settings → Notifications), quiet hours (optional),
  "only when unfocused" (default), and click-through routing (notification action → route + args;
  OS notification click focuses the window and navigates if the plugin supports actions, otherwise
  it's in-app only).
- Watches: store, scheduler (minute tick), `notify` 8.x for on-change with a 10-min debounce,
  `diff_reports`, threshold rules (size delta ≥ 500 MB or ≥ 10%, new files ≥ 1 GB, new duplicate
  groups), and the notification text template.
- Optional: launch at login (`tauri-plugin-autostart`) and a menu-bar tray icon with the runtime
  status and "Quick scan of watched folders".

## Steps
1. JobRegistry + persistence + events + tests. 2. Migrate existing long ops (scan, download, propose, report narrative) to jobs.
3. Jobs UI. 4. Notification prefs + routing. 5. Watches backend + tests (diff_reports). 6. Watches UI (in Analyzer: "Watch this folder"). 7. Tray + autostart (optional).

## Exit criteria
- [ ] Exit demo passes. Restarting during a job leaves no zombie "running" rows.
- [ ] Notifications respect per-kind toggles (test the routing function).

## Pitfalls
- macOS shows no OS notification for apps that aren't signed/bundled. Test in the built `.app`.
- `notify` on big trees: use FSEvents (macOS default backend) with recursive mode, and never watch `~` entirely.
