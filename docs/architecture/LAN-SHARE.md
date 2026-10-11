# LAN Share: Use NanoNexus from Your Phone and Other Computers (`crates/nexus-share`)

> One button: **Start sharing**. NanoNexus serves a responsive web app (mobile + desktop) on
> your local network at `http://<your-mac-ip>:9889`. Scan the QR code with your phone, and you
> can chat with your local assistants from the couch. It's **off by default**, needs pairing,
> and file changes stay on the desktop.

## 1. What gets exposed (and what never does)

```
 Phone / laptop browser ──HTTP(S) LAN :9889──►  NanoNexus app (axum server, nexus-share)
                                                  │  same service layer as Tauri commands
                                                  ▼
                                   Scheduler ─► llama-server @ 127.0.0.1:<random>   ← NEVER exposed
```
- **Exposed:** a NanoNexus web API + web UI, with authentication and per-device sessions.
- **Never exposed:** `llama-server` (stays on loopback with its secret token), the SQLite file,
  the raw filesystem, and the change-set Apply/Undo or grant management (unless the remote
  policy below allows it).

## 2. User experience

**Desktop:** title bar → **📡 Share** pill (grey = off). Click → popover:
```
┌───────────────────────────────────────────────┐
│ 📡 LAN Share                         [ ● ON ] │   ← big start/stop switch
│ Open on your phone:                           │
│   http://192.168.1.20:9889                    │
│   http://nanonexus.local:9889  (Bonjour)      │
│        ┌─────────┐                            │
│        │ QR CODE │  Scan to pair (expires 5m) │
│        └─────────┘  or enter PIN: 482 913     │
│ Paired devices (2): iPhone · iPad   [Manage]  │
│ ⚙ Sharing settings…                           │
└───────────────────────────────────────────────┘
```
When on, the pill turns green (`📡 2`, the device count) and the tray menu shows "Sharing on :9889".

**Phone:** open the link or scan the QR → the pairing screen pre-fills the token → name the device
("Kim's iPhone") → done. Optional "Add to Home Screen" (PWA manifest, standalone display, app icon).
The mobile UI is chat-first with a bottom bar: **Chat · Assistants · History · Reports · More**.

**Settings → Sharing** (full page):
| Setting | Default | Notes |
|---|---|---|
| Sharing enabled | Off | Same as the pill switch |
| Start sharing when NanoNexus launches | Off | |
| Port | **9889** | 1024–65535. Validated. Conflict → "Port in use; pick another" with the next free port suggested |
| Listen on | All local networks (0.0.0.0) | or one specific interface/IP, or "This computer only" (127.0.0.1, for testing) |
| Advertise as `nanonexus.local` (Bonjour/mDNS) | On | `mdns-sd` crate. Name editable |
| Security mode | Pairing (QR/PIN) | always on; no unauthenticated mode |
| HTTPS (self-signed) | Off | Optional. Browsers show a one-time warning. Fingerprint shown for verification |
| Remote devices can… | **Chat + read granted folders** | `chat_only` · `chat_read` (default) · `full` (can also apply changes and grant folders, with confirmation on the desktop) |
| Assistants available remotely | All | Checklist |
| Session lifetime | 30 days | Per device; revocable |
| Max devices | 5 | |
| Paired devices | list | name, last seen, IP, user agent, [Rename] [Revoke] |
| Notify me when a new device pairs | On | |
| Show remote activity in the desktop chat list | On | Remote conversations are tagged with the device name |

## 3. Security model

1. **Off by default.** Turning it on is explicit and shows a one-time explainer ("Anyone on this
   Wi-Fi who pairs can chat with your assistants").
2. **Pairing.** Starting the server creates a **pairing secret** (32 random bytes, base64url) shown
   as QR (`http://ip:9889/pair#<secret>`, in the URL fragment so it never hits server logs) and a
   **6-digit PIN** derived per attempt window. Pairing secrets expire after 5 minutes or one use;
   "Show new code" rotates them.
   PIN attempts are rate-limited: 5 per minute per IP, then a 10-minute lockout and a desktop notification.
3. **Device sessions.** Pairing returns a device token (32 bytes). The server stores only its
   SHA-256 (in `share_devices`). It's kept as an `HttpOnly; SameSite=Strict; Path=/` cookie (plus a
   `Secure` flag in HTTPS mode). Every request is checked, and revoking deletes the row.
4. **CSRF:** SameSite=Strict + a required `X-Nexus-CSRF` header (double-submit token) on mutating
   requests and the WebSocket upgrade. Same-origin only (no CORS).
5. **Remote policy enforcement** happens in the service layer: each call carries
   `Caller::Desktop | Caller::Remote{device_id, policy}`. `chat_read` remotes can use tools on
   **existing** grants (read access only). New grant requests from a remote return "Approve on the
   desktop": the desktop shows the normal permission modal, labeled "from Kim's iPhone".
   `apply`/`undo`/`grant` commands are rejected for `chat_only`/`chat_read`.
6. **Transport:** plain HTTP by default on the LAN (simple, works everywhere). Optional HTTPS
   uses a self-signed cert generated with `rcgen` and stored in the data dir, with SAN = LAN IPs +
   `nanonexus.local`. The UI shows the SHA-256 fingerprint for manual verification.
7. **Headers:** CSP `default-src 'self'; connect-src 'self' ws: wss:` · `X-Frame-Options: DENY` ·
   `Referrer-Policy: no-referrer` · `Cache-Control: no-store` for API responses.
8. **Limits:** request body ≤ 2 MB (attachments from the phone: text files ≤ 1 MB, images later),
   ≤ 4 concurrent WebSocket connections per device, and an idle timeout of 10 minutes.
9. **Audit:** remote calls appear in the audit log with actor `remote:<device name>`.
10. **OS prompts:** macOS Application Firewall may ask "Allow incoming network connections?" on
    the first start. Click **Allow** (it's remembered thanks to the stable signing identity).
    Windows Defender Firewall asks for "Private networks". Allow it, and keep Public unchecked.
    macOS 15+ **Local Network** privacy covers Bonjour advertising, so Info.plist adds
    `NSLocalNetworkUsageDescription` and `NSBonjourServices = ["_http._tcp"]`.

## 4. Architecture

### 4.1 One service layer, two transports
Refactor rule (applies from M04 on): **Tauri commands are thin wrappers** around
`nano_nexus::service::*` async functions that take `(&AppCtx, Caller, Args) -> Result<T, IpcError>`.
Streaming functions take a `Sink<T>` trait object (implemented by Tauri `Channel<T>` and by the WebSocket).

```
apps/desktop/src-tauri/src/
  service/        chat.rs, conversations.rs, assistants.rs, models.rs, reports.rs, notifications.rs, …  (transport-agnostic)
  commands/       #[tauri::command] fn x(state, args, channel) { service::x(&ctx, Caller::Desktop, args, ChannelSink(channel)) }
crates/nexus-share/
  src/server.rs   axum Router, bind/stop, graceful shutdown (CancellationToken)
  src/auth.rs     pairing secrets, PIN, device sessions, CSRF, rate limiter (governor-style token bucket)
  src/rpc.rs      WebSocket protocol + dispatch table: cmd name → handler (from an injected `ShareApi` trait)
  src/static.rs   embedded web UI (include_dir! of apps/desktop/dist-web), SPA fallback, cache headers
  src/mdns.rs     Bonjour advertise (mdns-sd)
  src/tls.rs      rcgen self-signed cert + rustls config (optional)
```
`nexus-share` does **not** depend on Tauri. The app implements `ShareApi` by delegating to
`service::*` with `Caller::Remote`.

### 4.2 Wire protocol (WebSocket `GET /api/ws`, JSON text frames)
```jsonc
// client → server
{"id": 7, "cmd": "chat_send", "args": {...}}           // same names and arg shapes as Tauri commands
{"id": 7, "cancel": true}                              // cancel a stream/job (maps to chat_stop etc.)
// server → client
{"id": 7, "ok": {...}}                                 // final result
{"id": 7, "err": {"code": "permission_denied", "message": "...", "detail": null}}
{"id": 7, "event": {"type": "content", "delta": "Hel"}} // streamed ChatEvent / progress
{"push": "runtime:status", "data": {...}}              // global events the client subscribed to
```
REST extras: `GET /api/health` (public, no data), `POST /api/pair` (secret or PIN → device cookie),
`POST /api/logout`, `GET /manifest.webmanifest`, `GET /` (SPA).

Allow-listed commands for remotes (start small, grow deliberately): `app_info`, `runtime_status`,
`assistants_list`, `assistant_get`, `chat_send`, `chat_stop`, `chat_regenerate`,
`conversations_list`, `conversation_get`, `conversation_update` (title/pin), `conversation_export_md`,
`reports_list`, `report_get`, `models_installed`, `models_activate` (only `full`),
`notifications_list`, `notifications_mark_read`, `fs_grants_list` (read-only view).

### 4.3 Frontend: one codebase, two builds
- `apps/desktop/src/lib/transport/` defines a `Transport` interface:
  `call<T>(cmd, args): Promise<T>`, `stream<E>(cmd, args, onEvent): Promise<Result>`, `on(event, cb)`.
  - `tauriTransport.ts` → `invoke` + `Channel` (uses the generated bindings).
  - `wsTransport.ts` → the WebSocket protocol above (auto-reconnect with backoff, resubscribe, offline banner).
- Feature code calls `api.chat.send(...)` (a typed facade over `Transport`), never `invoke` directly.
  **This is a rule from M04 onward** so the web build is cheap later.
- Vite builds twice: `pnpm build` (desktop, `dist/`) and `pnpm build:web` (`--mode web`, `dist-web/`,
  with `VITE_TARGET=web`). Desktop-only routes (Editor, Rename Forge, Changes apply, Permissions
  edit) render a friendly "Open NanoNexus on your computer for this" card in web mode.
- **Responsive layout** (Tailwind breakpoints): `< 768px` = mobile layout (bottom tab bar,
  conversation drawer, full-width chat, composer pinned to the bottom with
  `env(safe-area-inset-bottom)`, 44 px touch targets, no hover-only actions, long-press menus);
  `≥ 768px` = the desktop layout (sidebar). PWA: `manifest.webmanifest`, icons, theme color, and
  `display: standalone`. A tiny service worker for the app shell only (never cache API data).

## 5. Data
```sql
-- 0006_share.sql
CREATE TABLE share_devices (
  id TEXT PRIMARY KEY, name TEXT NOT NULL, token_hash TEXT NOT NULL UNIQUE,
  policy TEXT NOT NULL CHECK (policy IN ('chat_only','chat_read','full')),
  user_agent TEXT, last_ip TEXT, created_at INTEGER NOT NULL, last_seen_at INTEGER, expires_at INTEGER NOT NULL);
ALTER TABLE conversations ADD COLUMN origin TEXT NOT NULL DEFAULT 'desktop';  -- 'desktop' | 'remote:<device_id>'
```
Settings keys: `share.enabled` (false), `share.autostart` (false), `share.port` (9889),
`share.bind` ("0.0.0.0"), `share.mdns` (true), `share.mdns_name` ("nanonexus"), `share.https` (false),
`share.default_policy` ("chat_read"), `share.allowed_assistants` (["*"]), `share.session_days` (30),
`share.max_devices` (5), `share.notify_pairing` (true).

## 6. Commands (desktop only)
`share_status` → `{running, port, urls[], devices, https_fingerprint?}` · `share_start` · `share_stop` ·
`share_settings_set` · `share_pairing_code` (new QR + PIN) · `share_devices_list` ·
`share_device_update` (rename/policy) · `share_device_revoke` · event `share:status`.

## 7. Tests
- auth: pairing secret is single-use and expires; PIN rate limit and lockout; revoked device → 401;
  CSRF header required; cookie flags.
- policy: remote `chat_read` can't call `changeset_apply` (403) and can't create grants (desktop prompt path).
- rpc: request/response, streaming, cancel, unknown cmd → error, malformed frame → close 1003.
- server: start on 9889, port conflict → `port_in_use` error, stop releases the port, graceful shutdown with open sockets.
- web build: Playwright (Chromium, mobile viewport 390×844 + desktop 1280×800) runs pair → chat → stream → stop → history.
