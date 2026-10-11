# Known Gotchas (read before the matching milestone)

Each entry: **symptom → cause → fix**. Add new ones when you hit them.

## Toolchain and scripts
- **A script works on Linux but fails on the Mac** → macOS ships **bash 3.2** and BSD tools. → No
  associative arrays, `mapfile`, `${var,,}`, `sed -i` without a suffix argument (`sed -i ''` on
  BSD), `readlink -f`, `date -d`, or `grep -P`. Prefer Node scripts for anything complex.
- **`sort -V` / `realpath` missing on older macOS** → use the helpers in `scripts/doctor.sh` (`ver_ge`).
- **Node `fetch` ignores `HTTPS_PROXY`** → set `NODE_USE_ENV_PROXY=1` (Node ≥ 22.21) when behind a proxy.

## llama.cpp / runtime
- **The model repeats itself endlessly** → llama.cpp's default `min_p` of 0.05 hurts MiniCPM5. → Send `min_p: 0` (catalog presets do).
- **`<think>` appears in replies or JSON** → the template ignores `enable_thinking`, or the server
  didn't extract it. → Use `--reasoning-format deepseek`, the client-side ThinkSplitter, thinking
  off for JSON calls, and spike S1.
- **"Either json_schema or grammar can be specified, but not both"** (older builds) → don't send `grammar` together with `response_format`.
- **400 "exceeds the available context size"** → map it to `ContextOverflow`, re-budget, and retry once.
- **Orphan `llama-server` after a crash** → macOS has no parent-death signal. → Use `server.json`
  stale reaping at launch. Windows uses a Job Object. Always call `shutdown()` in `RunEvent::Exit`.
- **A console window flashes on Windows** → missing `CREATE_NO_WINDOW` (0x08000000).
- **The first load is ~1 s slower** → `GGML_METAL_EMBED_LIBRARY` compiles shaders at load. This is expected.
- **The health check passes, but another process answered** → `/health` is unauthenticated, so always confirm with an authenticated `/v1/models`.

## Tauri
- **`tauri dev` fails: external binary not found** → run `scripts/setup-runtime.sh` first (externalBin must exist).
- **Every `tauri dev` copies 1 GB** → the model was put in the base config. Only the generated overlay (`--config`) adds models.
- **Notifications don't show in dev on macOS** → expected. Test in the bundled, signed app.
- **Vibrancy isn't visible** → you need `transparent: true`, `macOSPrivateApi: true`, and a transparent `body`.
- **Streaming chat stutters** → too many Channel messages. Coalesce in Rust (16 ms / 32 chars) and flush in React per animation frame.
- **Don't mix `@tauri-apps/api` 2.x with 3.0 alphas** → pin `2.x` everywhere (ADR-001).
- **tauri-specta RC breaks** → pin the exact version. The fallback is `ts-rs` (ADR-006).

## macOS permissions
- **macOS asks for Documents access again after every rebuild** → ad-hoc signing changes the
  identity. → Sign with the stable self-signed "NanoNexus Local Signing" certificate (BUNDLING §10).
- **EPERM (os error 1) on ~/Desktop or ~/Documents** → TCC denied. Map it to `os_permission_denied`
  and deep-link to Privacy settings. EACCES (13) is ordinary POSIX permissions.
- **Probing TCC status triggers a prompt** → only probe on user action, never at startup.

## Paths and files
- **`canonicalize` fails for new files** → canonicalize the nearest existing ancestor, then re-append the rest.
- **Windows paths look like `\\?\C:\…`** → use `dunce::canonicalize`. Compare case-insensitively.
- **Cross-volume `rename` fails (EXDEV)** → copy + verify + trash the source.
- **`trash` is slow for thousands of files** → batch with `trash::delete_all` and show progress.

## Analyzer / Rename Forge
- **Renamed symbols break non-code references** → imports, config/manifest entries (`package.json`, `tsconfig`, `Cargo.toml`, …), and string lookups (`getattr`, reflection) must all be updated (ANALYZER §5.4).
- **Renamed files and folders break imports** → every import path and config reference must move with them; case-only renames need a temp name on case-insensitive volumes.
- **Entry points, dunders, tests, and overrides were renamed** → they're in the reserved lists. Never propose them.
- **The tree-sitter grammar's ABI doesn't match** → pin grammar versions that match `tree-sitter` 0.27 (spike S4).
- **`pdf-extract` panics** → run it under `catch_unwind` in `spawn_blocking`, and fall back to `pdftotext` if it's installed.

## Windows builds from the Mac
- **MSI requested** → impossible off Windows (WiX). Use NSIS only.
- **The installer build fails above ~2 GB** → the NSIS limit. Use the kit or portable format (auto above 1.8 GB).
- **aws-lc / NASM errors during cross-compile** → use reqwest with `native-tls`, not rustls with aws-lc.
- **Platform config not applied when cross-compiling** → pass `--config src-tauri/tauri.windows.conf.json` explicitly (spike S7).
- **"Windows protected your PC"** → unsigned build. Click *More info* → *Run anyway* (or sign it, M16).
- **No GPU acceleration on the studio PC** → Vulkan drivers are missing, so it falls back to CPU (spike S8). Update the GPU drivers.

## LAN Share
- **The phone can't connect** → firewall prompt denied, wrong network (guest Wi-Fi), or router
  client isolation. See guide.html troubleshooting.
- **iOS drops the stream when the screen locks** → reconnect on `visibilitychange` and refetch the conversation.
- **Port 9889 is busy** → `port_in_use`. The UI suggests the next free port.
