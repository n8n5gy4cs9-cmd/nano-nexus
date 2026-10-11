# M14: LAN Share and Mobile/Desktop Web UI

**Goal:** a simple **Start/Stop sharing** button that serves NanoNexus to your local network on
port **9889** (changeable), with a responsive web UI that works great on phones and desktop
browsers, secure pairing (QR/PIN), and a full Sharing settings page.

**Spec (normative):** [LAN-SHARE.md](../architecture/LAN-SHARE.md).
**Prereqs:** M04 (chat), M06 (assistants), M08 (reports, for the viewer), M11 (notifications).
The **transport-agnostic service layer + Transport facade rule** applies from M04 on, so this
milestone mostly adds a server and a web build.
**Exit demo:** desktop → 📡 Share → ON → scan the QR with your iPhone → pair → pick Nexus →
chat streams on the phone → stop from the phone → the conversation shows on the desktop
tagged "Kim's iPhone" → revoke the device on the desktop → the phone gets "Session ended" →
change the port to 9890 in Settings → it restarts → the new QR works. Turn sharing OFF → the port is closed.

## Scope
1. `crates/nexus-share`: axum 0.8 + tokio-tungstenite (via `axum::extract::ws`), auth (pairing,
   PIN, sessions, CSRF, rate limits), RPC dispatch over `ShareApi`, static web UI embedding with
   SPA fallback, mDNS (`mdns-sd`), optional TLS (`rcgen` + `rustls` via `axum-server`), graceful stop.
2. App: `ShareController` in AppState (start/stop/restart on settings change, status watch →
   `share:status` event), `ShareApi` impl delegating to `service::*` with `Caller::Remote`, a remote
   policy guard, desktop permission modal for remote grant requests, notifications on pairing,
   tray menu item.
3. Store migration 0006 (`share_devices`, `conversations.origin`).
4. Frontend: the `Transport` interface + `wsTransport`, `pnpm build:web`, the responsive mobile
   layout (bottom tabs, drawer, safe areas, touch targets), the pairing screen, PWA manifest +
   icons + minimal service worker, desktop-only route guards, and an offline/reconnecting banner.
5. Desktop UI: the 📡 Share pill + popover (switch, URLs, QR via the `qrcode` crate → SVG string,
   PIN, devices) and the Settings → Sharing page (all settings in LAN-SHARE §2).
6. Info.plist additions (`NSLocalNetworkUsageDescription`, `NSBonjourServices`) and
   Windows/macOS firewall guidance text in the UI and `guide.html`.
7. Tests (LAN-SHARE §7) including Playwright on mobile and desktop viewports.

## Design notes
- Default port **9889**. On start, if it's taken: error `port_in_use`, and the UI offers "Use 9890"
  (the next free port found by probing upward ≤ 20).
- LAN IP discovery: the `if-addrs` crate lists non-loopback IPv4 addresses on up interfaces.
  Show all, with the private-range (192.168/10/172.16) one first. IPv6 link-local is skipped in v1.
- The QR encodes `http://<ip>:<port>/pair#<secret>`. The pairing page reads the fragment, POSTs
  it, and removes it from the history (`history.replaceState`).
- Remote chat uses the same Scheduler (Interactive). Desktop and phone requests queue fairly
  (FIFO within the Interactive priority).
- Bundle the web build into the binary: `include_dir!("$CARGO_MANIFEST_DIR/../../apps/desktop/dist-web")`
  behind a `build.rs` check that the folder exists (otherwise an embedded "web UI not built" page).
  `scripts/build-app.sh` and `build-windows.sh` run `pnpm build:web` first.

## Steps
1. Service layer refactor check (all commands delegate to `service::*`; add `Caller`).
2. `nexus-share` auth + tests. 3. RPC over WebSocket + allow-list + policy guard + tests.
4. Static embedding + SPA + headers. 5. Server lifecycle + ShareController + settings + port conflict handling.
6. mDNS + Info.plist keys. 7. Optional HTTPS (rcgen) + fingerprint UI.
8. Frontend Transport + wsTransport + web build. 9. Mobile responsive layout + PWA + pairing screen.
10. Desktop pill + popover + Settings → Sharing + devices management. 11. Playwright e2e (mobile + desktop viewports).
12. Update `guide.html` (Sharing section) and `docs/WINDOWS-TESTING.md` (Windows firewall prompt).

## Exit criteria
- [ ] Exit demo passes on iPhone Safari and Android Chrome (or emulated in Playwright + one real phone).
- [ ] Security tests pass. No unauthenticated endpoint returns user data.
- [ ] The llama-server port is still loopback-only (`lsof -iTCP -sTCP:LISTEN` shows only `:9889` on LAN).
- [ ] Works on Windows builds too (firewall prompt documented).

## Pitfalls
- iOS Safari kills background WebSockets. Reconnect on `visibilitychange` and refetch the
  conversation to recover a missed stream (the server keeps the finished message in the DB).
- Don't put the device token in localStorage (XSS-exposed). Use only HttpOnly cookies.
- Some routers isolate Wi-Fi clients ("AP/client isolation"). The guide's troubleshooting covers it.
