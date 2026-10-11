# Bundling: Runtime, Models, Profiles, App Builder, macOS and Windows

This is the Rust/Tauri version of the Godot pipeline
(`setup-runtime.sh` → `select-game-model.cjs` → `export_plugin.gd` → `build.sh` → `verify-app-payload.cjs`),
extended with **bundle profiles** (choose which models ship), an **App Builder UI**,
**model packs** (offline sideloading), and **Windows builds cross-compiled on the Mac**.

| Godot piece | NanoNexus equivalent |
|---|---|
| `scripts/setup-runtime.sh` (pinned llama.cpp, Metal build, arm64 check) | `scripts/setup-runtime.sh` (macOS source build; `GGML_METAL_EMBED_LIBRARY=ON`, so no metallib) · `scripts/fetch-runtime-windows.mjs` (official pinned Windows binaries, digest-verified) |
| `scripts/select-game-model.cjs` (HF download by pinned revision + SHA-256, `vendor/lock.json`) | `scripts/fetch-model.mjs` (one or many models, catalog/HF/local) → `vendor/models/<id>/` + `lock.json` + `index.json` |
| `addons/bundled_ai/export_plugin.gd` (verify lock, add model as a real file, helpers) | `scripts/stage-bundle.mjs` (profile → staged files + manifest + generated Tauri overlay) |
| `build.sh` (validate checksums → export → re-hash → codesign) | `scripts/build-app.sh` (macOS) · `scripts/build-windows.sh` (Windows from the Mac) · `scripts/app-builder.mjs` (UI on top of both) |
| `verify-app-payload.cjs` | `scripts/verify-payload.mjs` |
| Paths relative to `OS.get_executable_path()` | `current_exe()` / Tauri `resource_dir()` |

---

## 1. `vendor/` layout (gitignored, produced on your Mac)

```
vendor/
  llama.cpp-src/                       pinned source checkout (macOS build)
  runtime/
    macos-arm64/  llama-server-aarch64-apple-darwin  runtime.lock.json  licenses/
    windows-x64/  llama-server.exe  *.dll  runtime.lock.json  licenses/      (official b11552 vulkan-x64 zip)
    windows-arm64/ llama-server.exe *.dll  runtime.lock.json  licenses/      (official b11552 cpu-arm64 zip)
  models/
    index.json                         {"default": "<id>", "models": {"<id>": {file, sha256, bytes, repo, revision}}}
    <id>/<file>.gguf                   one folder per model id (catalog id or your custom id)
    <id>/lock.json                     pinned repo/revision/file/sha256/bytes/license
  bundle/<profile>/<target>/           staged payload for one build (hardlinks into models/)
    models/manifest.json
    models/<id>/<file>.gguf
    runtime/…                          (Windows targets only)
    licenses/…
```

## 2. Build the macOS runtime: `scripts/setup-runtime.sh`

Already written. It:
1. Reads its pins from `scripts/runtime.pins` (`LLAMA_CPP_REPO`, `LLAMA_CPP_TAG=b11552`,
   `LLAMA_CPP_COMMIT=23b0202a189c44a54625aadcb37a946dd1d6278d`).
2. Fetches **exactly that commit** into `vendor/llama.cpp-src/` (`git init` + `git fetch --depth 1 origin <sha>`).
   Git verifies every object by hash, so source integrity is cryptographic.
3. Configures and builds `llama-server`:
   ```
   -DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=OFF
   -DLLAMA_BUILD_SERVER=ON -DLLAMA_BUILD_TESTS=OFF -DLLAMA_BUILD_EXAMPLES=OFF
   -DLLAMA_CURL=OFF -DLLAMA_OPENSSL=OFF   # no network code in the server
   -DGGML_METAL=ON -DGGML_METAL_EMBED_LIBRARY=ON
   -DGGML_NATIVE=OFF                      # portable across M1…M5
   -DCMAKE_OSX_ARCHITECTURES=arm64 -DCMAKE_OSX_DEPLOYMENT_TARGET=13.3
   ```
4. Verifies `lipo -archs` == `arm64` and that `otool -L` lists only `/usr/lib/*` and
   `/System/Library/Frameworks/*`, then runs `--version`.
5. Installs to `vendor/runtime/macos-arm64/llama-server-aarch64-apple-darwin` (the Tauri triple
   name) + `runtime.lock.json` + licenses.

## 3. Windows runtime: `scripts/fetch-runtime-windows.mjs` (M13)

Same tag `b11552`, official release assets. A source build isn't required (cross-compiling
llama.cpp is optional spike S9, M16):
| Arch | Asset | Why |
|---|---|---|
| x64 (default) | `llama-b11552-bin-win-vulkan-x64.zip` | Vulkan GPU (NVIDIA/AMD/Intel) + CPU backends |
| x64 (`--cpu-only`) | `llama-b11552-bin-win-cpu-x64.zip` | Smallest, no GPU |
| arm64 | `llama-b11552-bin-win-cpu-arm64.zip` | Windows on ARM (and testing in a VM on the Mac) |

Verification: `GET https://api.github.com/repos/ggml-org/llama.cpp/releases/tags/b11552` →
each asset's `digest` (`sha256:…`) → download → compare → record in `runtime.lock.json`. Keep
`llama-server.exe` and **all** `*.dll` (backends load as DLLs) plus `LICENSE*`. Delete other exes.

## 4. Models: `scripts/fetch-model.mjs`

Already written. Mirrors `select-game-model.cjs`, but handles many models:
```
node scripts/fetch-model.mjs                                  # the catalog default model
node scripts/fetch-model.mjs --id minicpm5-2b-q4              # catalog entries (repeatable)
node scripts/fetch-model.mjs --profile bundles/studio.json    # every model in a bundle profile
node scripts/fetch-model.mjs --repo owner/name --quant Q4_K_M --as my-model   # any HF GGUF repo
node scripts/fetch-model.mjs --local ~/Models/x.gguf --as my-local            # your own GGUF file
node scripts/fetch-model.mjs --verify [--id X]                # re-hash against the locks
node scripts/fetch-model.mjs --list                           # what's in vendor/models
```
For HF models: revision → commit SHA (`/api/models/{repo}/revision/{rev}`), file list with
`lfs.oid` (SHA-256) (`/api/models/{repo}/tree/{sha}`), then a Range-resumable download with
streaming hashing, size + SHA-256 + `GGUF` magic verification, an atomic move, and `lock.json` +
`index.json`. `HF_ENDPOINT` and `HF_TOKEN` are honored. Local files are hashed and hard-linked
(or copied) in.

## 5. Bundle profiles (`bundles/*.json`, schema `schemas/bundle-profile.schema.json`)

```json
{ "schema": 1, "name": "basic",
  "description": "Default build: one small model.",
  "models": ["minicpm5-1b-fable5-v2-q8"],
  "default": "minicpm5-1b-fable5-v2-q8",
  "targets": ["macos-arm64", "windows-x64"],
  "windows": { "format": "auto", "webview": "embedBootstrapper", "runtime": "vulkan" } }
```
- `bundles/basic.json`: committed. The **basic build** (default model only).
- `bundles/studio.example.json`: committed example of a multi-model offline bundle.
- `bundles/local/*.json`: **your** profiles (gitignored). The App Builder saves here.
- A model entry can be a catalog id, `{"repo": "...", "quant": "Q4_K_M", "as": "my-id"}`, or `{"local": "/path/x.gguf", "as": "my-id"}`.
- `windows.format`: `installer | portable | kit | auto` (auto = installer if payload ≤ 1.8 GB, else kit).

## 6. Staging and Tauri configuration

`scripts/stage-bundle.mjs --profile <p> --target <t>`:
1. Ensures every model is fetched and verified (calls fetch-model logic) and the runtime for `<t>` exists.
2. Hard-links models into `vendor/bundle/<p>/<t>/models/<id>/<file>` and writes `models/manifest.json`
   (MODEL-MANAGER §9 has the format). For Windows targets it also copies `runtime/` and licenses.
3. Writes a **generated overlay** `apps/desktop/src-tauri/gen/tauri.bundle.<p>.<t>.json`:
   ```json
   { "bundle": { "resources": {
       "../../../vendor/bundle/<p>/<t>/models/": "models/",
       "../../../vendor/bundle/<p>/<t>/licenses/": "licenses/",
       "../../../vendor/bundle/<p>/<t>/runtime/": "runtime/"      // windows only
   } } }
   ```

Base config `tauri.conf.json` (shared): productName `NanoNexus`, identifier `dev.nanonexus.app`,
CSP (OVERVIEW §7), `bundle.active=true`, icons, `category: "DeveloperTool"`.

`tauri.macos.conf.json` (merged automatically on macOS targets):
```json
{ "app": { "windows": [{ "label": "main", "title": "NanoNexus", "width": 1280, "height": 820, "minWidth": 900, "minHeight": 600,
                         "titleBarStyle": "Overlay", "hiddenTitle": true, "transparent": true }],
           "macOSPrivateApi": true },
  "bundle": { "targets": ["app"], "externalBin": ["../../../vendor/runtime/macos-arm64/llama-server"],
              "macOS": { "minimumSystemVersion": "13.0", "infoPlist": "Info.plist", "entitlements": "entitlements.plist" } } }
```

`tauri.windows.conf.json` (merged on Windows targets):
```json
{ "app": { "windows": [{ "label": "main", "title": "NanoNexus", "width": 1280, "height": 820, "minWidth": 900, "minHeight": 600,
                         "decorations": true, "transparent": false }] },
  "bundle": { "targets": ["nsis"],
              "windows": { "webviewInstallMode": { "type": "embedBootstrapper" },
                           "nsis": { "installMode": "currentUser", "compression": "none",
                                     "installerHooks": "windows/hooks.nsh", "displayLanguageSelector": false } } } }
```
(Spike S7 confirms that the platform config is picked by the **target** when cross-compiling. If
not, pass `--config tauri.windows.conf.json` explicitly in `build-windows.sh`.)

`Info.plist` (macOS usage strings; TCC needs them):
```xml
<dict>
  <key>NSDesktopFolderUsageDescription</key>   <string>NanoNexus reads files on your Desktop only when you ask an assistant to analyze them.</string>
  <key>NSDocumentsFolderUsageDescription</key> <string>NanoNexus reads files in Documents only when you ask an assistant to analyze them.</string>
  <key>NSDownloadsFolderUsageDescription</key> <string>NanoNexus scans Downloads only when you ask it to (for example, a Folder Scout report).</string>
  <key>NSRemovableVolumesUsageDescription</key><string>NanoNexus can analyze folders and import model packs from external drives you select.</string>
  <key>NSNetworkVolumesUsageDescription</key>  <string>NanoNexus can analyze folders on network drives you select.</string>
</dict>
```

### Resulting layouts
```
macOS  NanoNexus.app/Contents/
         MacOS/nano-nexus, MacOS/llama-server
         Resources/models/manifest.json, Resources/models/<id>/<file>.gguf …, Resources/licenses/, Resources/bundle.json
Windows %LOCALAPPDATA%\Programs\NanoNexus\            (per-user NSIS install; portable = the unzipped folder)
         nano-nexus.exe
         runtime\llama-server.exe, runtime\*.dll
         models\manifest.json, models\<id>\<file>.gguf …
         models-extra\ (offline kit only)   licenses\   bundle.json   [portable.flag + data\ in portable mode]
```

## 7. Windows, built on the Mac

Toolchain (one time): `rustup target add x86_64-pc-windows-msvc aarch64-pc-windows-msvc` ·
`cargo install --locked cargo-xwin` · `brew install nsis llvm` (add `$(brew --prefix llvm)/bin`
to PATH) · `export XWIN_CACHE_DIR=~/.cache/xwin`.

Build: `pnpm -C apps/desktop tauri build --runner cargo-xwin --target x86_64-pc-windows-msvc --bundles nsis --config <overlay>`.
Output: `target/x86_64-pc-windows-msvc/release/bundle/nsis/NanoNexus_<v>_x64-setup.exe`.
`scripts/build-windows.sh` wraps this and produces the three formats (M13 §6):

| Format | When | Contents | Internet needed on target |
|---|---|---|---|
| **Installer** | payload ≤ 1.8 GB (e.g. basic) | `…-setup.exe` with models inside | Only if WebView2 is missing (Win10) |
| **Portable ZIP** | No install rights, USB use | Unzip → run `nano-nexus.exe`; data in `.\data\` | No (Windows 11) |
| **Offline kit** | Big multi-model bundles, studio PCs | `setup.exe` (default model) + `models-extra\` + WebView2 offline installer + `INSTALL.txt` | **No** |

`windows/hooks.nsh` (offline kit):
```nsis
!macro NSIS_HOOK_POSTINSTALL
  IfFileExists "$EXEDIR\models-extra\pack.json" 0 +3
    CreateDirectory "$INSTDIR\models-extra"
    CopyFiles /SILENT "$EXEDIR\models-extra\*.*" "$INSTDIR\models-extra"
!macroend
```
On first launch, the app finds `<exe_dir>\models-extra\pack.json`, verifies each SHA-256 (with
a progress UI), and registers the models as bundled.

Runtime resolution on Windows: `resource_dir()/runtime/llama-server.exe` (DLLs load from the
exe's own folder). On macOS: `current_exe().parent()/llama-server`.

## 8. App Builder: `scripts/app-builder.mjs` (M12)

```
node scripts/app-builder.mjs                     # opens http://127.0.0.1:<port>/?t=<token> in your browser
node scripts/app-builder.mjs --profile studio --target all --yes   # headless
```
- Pick models (catalog checkboxes; **＋ Hugging Face** repo + quant; **＋ Local .gguf**), set the
  default model, and pick targets (macOS arm64, Windows x64, Windows arm64) and the Windows format.
- A live size summary per target warns about the NSIS limit and free disk space.
- Save/load profiles (`bundles/local/<name>.json`).
- **Build** streams logs, then shows the artifacts with sizes, SHA-256, and Reveal buttons.
- Security: 127.0.0.1 only, random URL token, and it exits when idle.

## 9. Model packs (no rebuild; offline)
- **Make one on the Mac:** `node scripts/make-model-pack.mjs --profile studio --out /Volumes/USB`
  (or `--id a --id b`), or in the app: Models → select → **Export model pack…**.
- **Install it on any NanoNexus** (macOS or Windows): Models → **Import model pack…** → choose the
  folder → every file is SHA-256-verified → copied into the data dir (or referenced in place).
- Pack format: `NanoNexus-ModelPack-<name>/pack.json` + `models/<id>/<file>.gguf` (same entries as
  `manifest.json`). See MODEL-MANAGER §9.

## 10. macOS build, verification, signing (`scripts/build-app.sh`, M12)

1. `scripts/doctor.sh --quiet` → 2. `node scripts/fetch-model.mjs --profile <p> --verify` →
3. runtime lock check → 4. `node scripts/stage-bundle.mjs --profile <p> --target macos-arm64` →
5. `pnpm -C apps/desktop tauri build --bundles app --config src-tauri/gen/tauri.bundle.<p>.macos-arm64.json` →
6. `node scripts/verify-payload.mjs <NanoNexus.app> --manifest …` (re-hash every model and the runtime,
   check `lipo` + `otool` + Info.plist keys) → 7. `codesign --force --deep --options runtime --sign "$NEXUS_SIGN_IDENTITY"`
   + `codesign --verify --deep --strict` → 8. `ditto -c -k --keepParent` → `dist/<p>/NanoNexus-<v>-<p>-macos-arm64.zip` + `SHA256SUMS.txt`.

### Stable signing identity (important for TCC)
TCC ties folder grants to the app's code-signing identity. **Ad-hoc signing (`-`) creates a new
identity on every build**, so macOS asks for Documents/Desktop access again after each rebuild.
One-time fix: Keychain Access → Certificate Assistant → *Create a Certificate…* → Name
`NanoNexus Local Signing`, Identity Type *Self-Signed Root*, Certificate Type **Code Signing**.
Then `export NEXUS_SIGN_IDENTITY="NanoNexus Local Signing"`. Copying the app to another Mac:
`xattr -dr com.apple.quarantine NanoNexus.app` there (or notarize, M16).

### Hardened runtime and entitlements (`entitlements.plist`)
- `com.apple.security.cs.allow-jit` = true (WKWebView JS JIT under hardened runtime).
- No `disable-library-validation` (the sidecar is static and signed with the same identity).
- No App Sandbox keys (ADR-010).

## 11. Dev mode
`tauri dev` doesn't copy models. In debug builds the model resolves in this order:
`$NEXUS_MODEL_PATH` → `vendor/models/index.json` default → none (onboarding shows "Download a
model"). The runtime resolves next to the exe (Tauri copies `externalBin` into `target/debug/`).
Run `scripts/setup-runtime.sh` before the first `tauri dev`.

`runtime_paths.rs` (sketch):
```rust
pub fn server_binary(app: &AppHandle) -> PathBuf {
    #[cfg(windows)] { app.path().resource_dir().unwrap().join("runtime").join("llama-server.exe") }
    #[cfg(not(windows))] { std::env::current_exe().unwrap().parent().unwrap().join("llama-server") }
}
pub fn bundled_manifest(app: &AppHandle) -> Option<BundleManifest> { /* resource_dir()/models/manifest.json */ }
pub fn extra_model_dirs(app: &AppHandle, settings: &Settings) -> Vec<PathBuf> { /* <exe_dir>/models-extra, user folders */ }
```
llama.cpp opens GGUF files **by path** (mmap). Never use `include_bytes!` and never copy them to temp.
