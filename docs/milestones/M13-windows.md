# M13: Windows Build (MUST): Cross-Compiled on Your Apple Silicon Mac

**Goal:** produce Windows 11 builds of NanoNexus **on your Mac** (no Windows build machine),
with the same bundled-model behavior and bundle profiles as macOS. Three output formats:
**Installer** (NSIS `-setup.exe`), **Portable ZIP**, and **Offline kit** (installer +
`models-extra/` folder, for big multi-model bundles and studio PCs with no internet).

**Spec (normative):** [BUNDLING.md §7](../architecture/BUNDLING.md#7-windows-built-on-the-mac), ADR-014, ADR-015, [`guide.html`](../../guide.html) (Windows sections).
**Prereqs:** M12 (profiles + staging), spike S7 (done in M03).
**Exit demo:** `node scripts/app-builder.mjs` → targets Windows x64 + Windows arm64 → Build.
Then on a Windows 11 PC (x64) **offline**: run the installer → the app opens → Ready → chat works.
In a Windows 11 ARM VM (UTM/Parallels on the Mac): the arm64 portable ZIP runs → Ready.

## Scope
1. Toolchain on the Mac (doctor checks): `rustup target add x86_64-pc-windows-msvc aarch64-pc-windows-msvc`,
   `cargo install --locked cargo-xwin`, `brew install nsis llvm` (put the LLVM bin on PATH), `XWIN_CACHE_DIR`
   set to a shared cache. Verify `makensis -VERSION`, `clang-cl --version`, `lld-link --version`.
2. **Windows runtime:** `scripts/fetch-runtime-windows.mjs` downloads the **official llama.cpp
   release assets for the same pinned tag** (`b11552`): `llama-b11552-bin-win-vulkan-x64.zip`
   (default for x64: Vulkan GPU + CPU fallback), `llama-b11552-bin-win-cpu-x64.zip` (`--cpu-only`),
   and `llama-b11552-bin-win-cpu-arm64.zip` (arm64). It verifies each zip's SHA-256 against the
   GitHub release-asset `digest` (sha256), extracts into `vendor/runtime/windows-<arch>/`, prunes
   unneeded exes (keeps `llama-server.exe` + all `*.dll`), and writes `runtime.lock.json`.
   Cross-compiling llama.cpp itself is a stretch goal (spike S9).
3. **Platform configs:** `tauri.windows.conf.json` (no `externalBin`; the runtime folder is a
   *resource* `runtime/`, so its DLLs sit next to `llama-server.exe`), WebView2 install mode,
   NSIS settings (per-user install, `compression: "none"` for GGUF payloads, installer hooks for the
   offline kit), Windows icon, `windows_subsystem = "windows"`.
4. **Rust Windows specifics** (INFERENCE-CONTRACT §1 Windows notes): spawn with `CREATE_NO_WINDOW`,
   assign the child to a **Job Object with KILL_ON_JOB_CLOSE** (so the server dies even if the app
   crashes), resolve the server via `resource_dir()/runtime/llama-server.exe`, use `dunce` paths,
   Recycle Bin trash, Controlled Folder Access detection (error 5 on write), and the Windows deny list.
5. **UI on Windows:** native title bar (decorations on) + **Mica** backdrop (`window_vibrancy::apply_mica`),
   Ctrl-based shortcuts (the shortcut layer maps ⌘ → Ctrl), Segoe-friendly font fallback, and no
   traffic-light padding.
6. **`scripts/build-windows.sh`** `--profile <p> --arch x64|arm64 --format installer|portable|kit`:
   stage (shared with M12) → `pnpm tauri build --runner cargo-xwin --target <triple> --bundles nsis --config <overlay>`
   → post-process:
   - *installer*: `dist/<profile>/NanoNexus-<v>-<profile>-windows-<arch>-setup.exe`
   - *portable*: zip of `nano-nexus.exe` + `runtime/` + `models/` + `portable.flag` (the app stores data in
     `.\data\` when this flag exists next to the exe) + `README-WINDOWS.txt`
   - *kit*: basic installer (default model only) + `models-extra/` (all other profile models + `pack.json`) +
     `INSTALL.txt`. An NSIS post-install hook copies `$EXEDIR\models-extra\*` → `$INSTDIR\models-extra\`
     when present, and the app imports and verifies them on first launch.
   → `verify-payload.mjs --windows` (hash checks of everything staged + PE header arch check) → SHA256SUMS.txt.
7. **Testing:** a Windows 11 **ARM** VM on the Mac (UTM/Parallels) runs the arm64 build natively
   **and** the x64 build under Windows' Prism x64 emulation (CPU only, slower, but fine for smoke
   tests). Final performance and GPU (Vulkan) checks happen on real x64 hardware (the studio PC). A
   test checklist lives in `docs/WINDOWS-TESTING.md` (written in this milestone).

## Design notes
- **NSIS ~2 GB limit:** NSIS can't produce installers much over 2 GB. The builder computes the
  payload and **forces `kit` or `portable`** when models + runtime exceed **1.8 GB**. The default
  profile (1.1 GB model) fits in a normal installer.
- **WebView2:** Windows 11 includes it. For Windows 10, set `webviewInstallMode`:
  `{"type":"embedBootstrapper"}` for the installer (needs internet on first run if WebView2 is
  missing) or `offlineInstaller` (+~127 MB) for the offline kit. Use `offlineInstaller` for
  `kit` builds so studio PCs need no internet.
- **Unsigned binaries:** SmartScreen shows "Windows protected your PC" → *More info* → *Run anyway*.
  This is fine for personal use. Optional signing from the Mac via `osslsigncode` + a code-signing
  cert is covered in M16.
- **Vulkan fallback:** if `vulkan-1.dll` is missing (very old drivers), llama.cpp skips the Vulkan
  backend and uses CPU. Spike S8 verifies this on a PC without a GPU driver. The runtime panel
  shows the active backend (parsed from the server log: `using device Vulkan0` / `CPU`).
- Paths: data `%APPDATA%\dev.nanonexus.app\`, logs `%LOCALAPPDATA%\dev.nanonexus.app\logs\`,
  portable `.\data\`.

## Steps
1. Doctor checks for the Windows toolchain. 2. `fetch-runtime-windows.mjs` (+ test against a mocked GitHub API).
3. Platform configs + Rust Windows specifics (Job Object, CREATE_NO_WINDOW, paths, Mica, shortcuts).
4. `build-windows.sh` installer format → test on a PC. 5. Portable format + `portable.flag` data-dir switch → test.
6. Offline kit + NSIS hook + first-launch import → test offline. 7. App Builder integration (targets + formats).
8. `docs/WINDOWS-TESTING.md` checklist run on x64 + arm64. Update `guide.html`.

## Exit criteria
- [ ] Exit demo passes on x64 hardware and an arm64 VM.
- [ ] Killing the app via Task Manager → `llama-server.exe` is gone too (Job Object).
- [ ] No console window flashes when the server starts.
- [ ] A 3-model studio profile produces a working offline kit. The models are verified on import.

## Pitfalls
- Tauri cross-compilation is officially "not as tested". Pin tauri-cli and run S7 early (M03).
- C dependencies (rusqlite bundled SQLite, tree-sitter grammars, blake3) compile with clang-cl
  via cargo-xwin. Avoid crates that need MSVC-only tools or NASM. Use reqwest with **native-tls**
  (SChannel on Windows, Security.framework on macOS) instead of aws-lc-rs.
- MSI installers can't be built on macOS (WiX is Windows-only). Use NSIS only.
