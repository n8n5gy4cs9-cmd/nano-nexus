# M03: App Shell and Design System

**Goal:** a beautiful, empty NanoNexus window. Rust state is wired, the runtime starts with the
app, the runtime pill shows live status, themes work, notifications plumbing exists, and a
**walking-skeleton `.app`** proves bundling end to end early.

**Specs:** [UI-DESIGN.md](../design/UI-DESIGN.md) §2–3, [BUNDLING.md](../architecture/BUNDLING.md) §3–4,
[DATA-AND-IPC.md](../architecture/DATA-AND-IPC.md).
**Prereqs:** M02. **Exit demo:** `pnpm tauri dev` opens the shell with a green "Ready" pill within 5 s;
`pnpm tauri build --bundles app --config src-tauri/tauri.bundle.conf.json` produces an `.app`
that, **moved to /Applications and opened with Wi-Fi off**, also turns Ready.

## Scope
1. Scaffold `apps/desktop` (Vite + React 19 + TS strict) and `src-tauri` (Tauri 2.12), and add it to the Cargo workspace.
2. Frontend foundation: Tailwind v4, tokens.css, fonts, shadcn/ui init, lucide, router, zustand, TanStack Query.
3. Tauri foundation: AppState, setup hook (paths, DB open + migrations stub, supervisor start),
   exit handling, window vibrancy, capabilities, CSP, single-instance, log plugin, window-state.
4. Type bindings via tauri-specta (spike S6) → `src/bindings.ts`.
5. Shell UI: title bar, sidebar nav (all 8 sections as placeholder pages), runtime pill + popover,
   theme switcher, empty states.
6. Notifications plumbing: `notification` plugin, a `notify()` service in Rust (DB insert + emit +
   OS notification if unfocused), and the bell popover in the UI.
7. Walking-skeleton bundle (overlay config + Info.plist + entitlements) and a manual test.
8. **Platform configs** from day one: `tauri.macos.conf.json` (overlay title bar, externalBin) and
   `tauri.windows.conf.json` (native decorations, NSIS, runtime as a resource). See BUNDLING §6.
9. **Spike S7: Windows walking skeleton built on the Mac.** Install the cross toolchain
   (`scripts/doctor.sh --windows`), run `node scripts/fetch-runtime-windows.mjs --arch x64` (write it
   here, per BUNDLING §3), and cross-compile the shell with `--runner cargo-xwin`. Run the installer
   in a Windows 11 ARM VM (x64 emulation) and on a PC if available → the window opens → Ready.
   Record every problem and its fix in `docs/SPIKES.md` §S7.

## Design notes

### Frontend structure
```
apps/desktop/
  index.html  vite.config.ts  tsconfig.json  eslint.config.js  components.json (shadcn)
  src/
    main.tsx                    QueryClientProvider, RouterProvider, Toaster, ThemeProvider
    bindings.ts                 GENERATED (tauri-specta) — never edit
    lib/ipc.ts                  thin helpers: unwrap Result → throw IpcError; channel helpers
    lib/format.ts               bytes, durations, relative time
    styles/tokens.css           design tokens (UI-DESIGN §2)
    styles/globals.css          tailwind import, base styles, scrollbar, selection color
    stores/ui.ts                zustand: sidebar, inspector, theme, palette open
    routes.tsx                  createHashRouter: / (chat), /assistants, /models, /analyzer, /rename, /editor, /changes, /permissions, /settings
    components/ui/*             shadcn primitives (generated, then themed)
    components/shell/           AppShell, TitleBar, Sidebar, NavItem, RuntimePill, NotificationBell, Inspector
    features/<area>/…           one folder per screen area (chat, assistants, models, analyzer, rename, editor, changes, permissions, settings)
```
- Use `createHashRouter`, which is robust under Tauri's custom protocol.
- `@fontsource-variable/inter` + `@fontsource-variable/jetbrains-mono`, imported in `main.tsx`.
- Tailwind v4: `@import "tailwindcss";` + `@theme inline { --color-bg: var(--bg); … }` so
  utilities like `bg-bg` and `text-muted` map to the tokens.
- Title bar: a `data-tauri-drag-region` div 44 px tall, left-padded 78 px for the traffic lights.

### Rust app structure
```
apps/desktop/src-tauri/
  Cargo.toml (package name "nano-nexus")   build.rs (tauri_build::build())
  tauri.conf.json  tauri.bundle.conf.json  Info.plist  entitlements.plist  capabilities/default.json
  src/main.rs          fn main() { nano_nexus::run() }
  src/lib.rs           run(): Builder + plugins + setup + invoke_handler(specta builder) + run(event handler)
  src/state.rs         AppState { paths, db, supervisor, scheduler, settings: RwLock<Settings>, jobs, perms (M05), registry (M06) }
  src/runtime_paths.rs server_binary(), default_model()  (BUNDLING §4)
  src/notify.rs        notify(app, NotificationInput) → DB + emit + OS (focus-aware)
  src/commands/mod.rs  app.rs, runtime.rs, settings.rs, notifications.rs  (more per milestone)
  src/events.rs        bridge: supervisor.state() watch → app.emit("runtime:status")
```
Plugins: `tauri-plugin-notification`, `-dialog`, `-opener`, `-single-instance`, `-log`,
`-window-state` (all 2.x). Capabilities (`capabilities/default.json`): `core:default`,
`notification:default`, `dialog:allow-open`, `dialog:allow-save`, `opener:allow-open-url`
(scope `https://*`), `window-state:default`. **No fs, no shell.**

Vibrancy (macOS): `window_vibrancy::apply_vibrancy(&window, NSVisualEffectMaterial::Sidebar, None, None)`
in setup. The webview background must be transparent (`transparent: true` + `body { background: transparent }`),
with the main content area painting `--bg` itself.

Exit handling:
```rust
.build(ctx)?.run(|app, event| match event {
    RunEvent::ExitRequested { .. } | RunEvent::Exit => {
        let st = app.state::<AppState>();
        tauri::async_runtime::block_on(st.supervisor.shutdown());
    }
    _ => {}
});
```

### Notifications plumbing
- Rust `notify()` inserts into `notifications` (needs migration 0004 early, or just that table now),
  emits `notification:new`, and if `!window.is_focused()` and the kind is enabled, sends an OS
  notification via `app.notification().builder().title(..).body(..).show()`.
- UI: a bell with an unread count (TanStack Query + invalidate on event) and a popover list.
- macOS: notifications show reliably only from the **bundled, signed** app. In `tauri dev` they
  may appear as Terminal or not at all. Test them in the walking skeleton.

## Steps
1. `pnpm create vite apps/desktop --template react-ts`, then add Tauri with `pnpm tauri init`.
   Fix the identifiers per BUNDLING §3. Add `apps/desktop/src-tauri` to workspace members.
2. Tailwind v4 + tokens + fonts + globals. Install shadcn/ui and add the primitives listed in UI-DESIGN §6.
3. AppState + setup + supervisor start using `default_model()` (fall back to the "no model" state) + exit handling.
4. tauri-specta: commands `app_info`, `runtime_status`, `runtime_restart`, `runtime_logs`,
   `settings_get`, `settings_set`, and events. Export bindings in debug builds on startup
   (`builder.export(Typescript::default(), "../src/bindings.ts")`). **Spike S6:** if it fails,
   use the ts-rs fallback (ADR-006).
5. Shell UI: AppShell, Sidebar with nav + placeholder conversation list, TitleBar, RuntimePill
   (+ popover with logs), theme switch (data-theme on `<html>`, persisted in settings), placeholder pages.
6. Notifications: plugin, `notify()`, migration for `notifications`, commands
   `notifications_list/mark_read/clear`, and the bell popover. Add a debug-menu item
   "Send test notification".
7. Walking skeleton: create `tauri.bundle.conf.json`, `Info.plist`, `entitlements.plist`, and the
   app icon (placeholder hexagon). Build, move to /Applications, test offline, test a notification,
   and quit → confirm no orphan with `pgrep llama-server`.

## Exit criteria
- [ ] Dev: Ready in < 5 s. Runtime pill transitions are visible (Starting → Loading → Ready).
- [ ] Killing `llama-server` manually → the pill goes red → auto-restart → green.
- [ ] Theme toggle works and persists. Vibrancy is visible on macOS.
- [ ] The bundled `.app` works offline after being moved. A notification is visible. No orphan after quit.
- [ ] `src/bindings.ts` is generated and the frontend uses only typed calls.
- [ ] S7: the Windows skeleton installer (built on the Mac) opens and reaches Ready on Windows (x64 emulation or real PC).

## Pitfalls
- `externalBin` must exist at build time for **every** `tauri dev`. Run `scripts/setup-runtime.sh` first.
- Use `@tauri-apps/api` **2.x**. Never mix in 3.0 alphas.
- Transparent windows plus vibrancy need `macOSPrivateApi: true` in tauri.conf.json.
- Don't block the setup hook on server readiness. Start it in a spawned task so the UI opens immediately.
