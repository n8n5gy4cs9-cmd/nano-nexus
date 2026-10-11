# Bundling: Runtime, Model, and the `.app`

This is the Rust/Tauri version of the Godot pipeline
(`setup-runtime.sh` → `select-game-model.cjs` → `export_plugin.gd` → `build.sh` → `verify-app-payload.cjs`).

| Godot piece | NanoNexus equivalent |
|---|---|
| `scripts/setup-runtime.sh` (pinned llama.cpp, Metal build, arm64 check) | `scripts/setup-runtime.sh`, same idea, plus `GGML_METAL_EMBED_LIBRARY=ON` so no metallib is needed |
| `scripts/select-game-model.cjs` (HF download by pinned revision + SHA-256, `vendor/lock.json`) | `scripts/fetch-model.mjs` (Node 22, no deps) → `vendor/models/model.gguf` + `vendor/models/lock.json` |
| `addons/bundled_ai/export_plugin.gd` (verify lock, add model as a real file, helpers into `Contents/Helpers`) | `tauri.conf.json` `externalBin` + `tauri.bundle.conf.json` `resources` + `scripts/verify-payload.mjs` pre-check |
| `build.sh` (validate checksums → export → re-hash → codesign) | `scripts/build-app.sh` (M12) |
| Paths relative to `OS.get_executable_path()` | `current_exe()` / Tauri `resource_dir()` |

---

## 1. Build the runtime: `scripts/setup-runtime.sh`

Already written (see the script). What it does:

1. Reads its pins from `scripts/runtime.pins` (`LLAMA_CPP_REPO`, `LLAMA_CPP_COMMIT`, `LLAMA_CPP_TAG`).
2. Fetches **exactly that commit** into `vendor/llama.cpp-src/` (`git init` + `git fetch --depth 1 origin <sha>`).
   Git verifies every object by hash, so the source integrity is cryptographic (no tarball hash needed).
3. Configures CMake:
   ```
   -DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=OFF
   -DLLAMA_BUILD_SERVER=ON -DLLAMA_BUILD_TESTS=OFF -DLLAMA_BUILD_EXAMPLES=OFF
   -DLLAMA_CURL=OFF -DLLAMA_OPENSSL=OFF   # no network code; unknown-var warnings are fine
   -DGGML_METAL=ON -DGGML_METAL_EMBED_LIBRARY=ON
   -DGGML_NATIVE=OFF                      # portable across M1…M5 (Metal does the heavy lifting)
   -DCMAKE_OSX_ARCHITECTURES=arm64 -DCMAKE_OSX_DEPLOYMENT_TARGET=13.3
   ```
   Then `cmake --build … --target llama-server -j`.
4. Verifies: `lipo -archs` == `arm64` only; `otool -L` lists **only** `/usr/lib/*` and
   `/System/Library/Frameworks/*`; `llama-server --version` runs.
5. Copies the result to `vendor/runtime/llama-server-aarch64-apple-darwin` (the Tauri triple
   name), plus `vendor/runtime/licenses/llama.cpp-LICENSE` and `vendor/runtime/runtime.lock.json`
   = `{repo, tag, commit, built_at, sha256, cmake_flags, host}`.

Linux and Windows paths in the same script (optional): Linux builds CPU-only (or `GGML_VULKAN=ON`
with the `--vulkan` flag) → `llama-server-x86_64-unknown-linux-gnu`. On Windows, use the
documented manual steps in M14.

**Upgrading llama.cpp:** change the pins, re-run the script, run `cargo test -p nexus-llm -- --ignored`
(the real-model smoke test), and only commit the pin change if it passes.

## 2. Fetch the model: `scripts/fetch-model.mjs`

Already written. It mirrors `select-game-model.cjs`:

```
node scripts/fetch-model.mjs                       # default from catalog (isDefault)
node scripts/fetch-model.mjs --id minicpm5-1b-q4   # another catalog entry
node scripts/fetch-model.mjs --repo owner/name --quant Q4_K_M [--revision <sha>]
node scripts/fetch-model.mjs --verify              # re-hash vendor/models/model.gguf against the lock
```

1. Resolves the revision to a **commit SHA** via `GET /api/models/{repo}/revision/{rev}`.
2. Lists files via `GET /api/models/{repo}/tree/{commit}`, picks the file by exact name or quant
   suffix, and reads `lfs.oid` (the SHA-256) and `size`.
3. Downloads `https://huggingface.co/{repo}/resolve/{commit}/{file}` into `vendor/models/.download/<file>.part`
   with HTTP Range **resume**, hashing as it streams (and re-hashing the existing part on resume).
4. Verifies the size and SHA-256 against the LFS oid, checks the GGUF magic `GGUF`, and moves
   the file to `vendor/models/model.gguf`.
5. Writes `vendor/models/lock.json`:
   ```json
   { "schema": 1, "id": "minicpm5-1b-fable5-v2-q8", "repo": "GnLOLot/…-GGUF", "revision": "<commit sha>",
     "file": "…-Q8_0.gguf", "sha256": "…", "bytes": 1100000000, "license": "apache-2.0",
     "fetched_at": "2026-…", "source_url": "https://huggingface.co/…" }
   ```
6. **Only one GGUF is kept in `vendor/models/`** (as in Godot). Other models are downloaded at
   runtime by the app into App Support.

`HF_ENDPOINT` (a mirror) and `HF_TOKEN` (gated repos) env vars are honored.

## 3. Tauri configuration

`apps/desktop/src-tauri/tauri.conf.json` (base, used by `tauri dev`):

```jsonc
{
  "productName": "NanoNexus",
  "identifier": "dev.nanonexus.app",
  "build": { "frontendDist": "../dist", "devUrl": "http://localhost:1420",
             "beforeDevCommand": "pnpm dev", "beforeBuildCommand": "pnpm build" },
  "app": {
    "windows": [{ "label": "main", "title": "NanoNexus", "width": 1280, "height": 820,
                  "minWidth": 900, "minHeight": 600, "titleBarStyle": "Overlay",
                  "hiddenTitle": true, "transparent": true }],
    "macOSPrivateApi": true,
    "security": { "csp": "default-src 'self'; connect-src ipc: http://ipc.localhost; img-src 'self' asset: data:; style-src 'self' 'unsafe-inline'; font-src 'self' data:" }
  },
  "bundle": {
    "active": true,
    "targets": ["app"],
    "category": "DeveloperTool",
    "icon": ["icons/32x32.png", "icons/128x128.png", "icons/128x128@2x.png", "icons/icon.icns", "icons/icon.ico"],
    "externalBin": ["../../../vendor/runtime/llama-server"],
    "macOS": { "minimumSystemVersion": "13.0", "signingIdentity": null, "infoPlist": "Info.plist" }
  }
}
```

`apps/desktop/src-tauri/tauri.bundle.conf.json` (overlay, merged by `tauri build --config …`):

```json
{
  "bundle": {
    "resources": {
      "../../../vendor/models/model.gguf": "models/model.gguf",
      "../../../vendor/models/lock.json": "models/lock.json",
      "../../../vendor/runtime/licenses/": "licenses/"
    }
  }
}
```

`apps/desktop/src-tauri/Info.plist` (merged into the generated one):

```xml
<dict>
  <key>NSDesktopFolderUsageDescription</key>   <string>NanoNexus reads files on your Desktop only when you ask an assistant to analyze them.</string>
  <key>NSDocumentsFolderUsageDescription</key> <string>NanoNexus reads files in Documents only when you ask an assistant to analyze them.</string>
  <key>NSDownloadsFolderUsageDescription</key> <string>NanoNexus scans Downloads only when you ask it to (for example, a Folder Scout report).</string>
  <key>NSRemovableVolumesUsageDescription</key><string>NanoNexus can analyze folders on external drives you select.</string>
  <key>NSNetworkVolumesUsageDescription</key>  <string>NanoNexus can analyze folders on network drives you select.</string>
  <key>LSApplicationCategoryType</key>          <string>public.app-category.developer-tools</string>
</dict>
```

Result layout (compare with Godot's `buddy bits.app`):

```
NanoNexus.app/Contents/
  MacOS/            nano-nexus (app), llama-server (sidecar; triple suffix stripped)
  Resources/
    models/         model.gguf, lock.json
    licenses/       llama.cpp-LICENSE, MiniCPM-LICENSE (Apache-2.0), THIRD-PARTY.md
    icon.icns
  Info.plist        (with usage descriptions)
  _CodeSignature/
```

## 4. Runtime path resolution (`apps/desktop/src-tauri/src/runtime_paths.rs`)

```rust
pub fn server_binary() -> PathBuf {
    // Tauri copies externalBin next to the main executable in both dev (target/debug) and bundle (Contents/MacOS)
    let exe = std::env::current_exe().expect("current_exe");
    exe.parent().unwrap().join(if cfg!(windows) { "llama-server.exe" } else { "llama-server" })
}

pub fn default_model(app: &tauri::AppHandle) -> Option<PathBuf> {
    if let Ok(p) = std::env::var("NEXUS_MODEL_PATH") { return Some(p.into()); }
    if let Ok(dir) = app.path().resource_dir() {
        let p = dir.join("models/model.gguf");
        if p.exists() { return Some(p); }
    }
    #[cfg(debug_assertions)]
    { let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../vendor/models/model.gguf");
      if p.exists() { return Some(p); } }
    None // → onboarding shows "Download a model"
}
```

llama.cpp opens the GGUF **by filesystem path** (mmap). Never use `include_bytes!` or copy it to
temp. Bundle resources are real files, so this works.

## 5. Build script (M12): `scripts/build-app.sh`

1. `scripts/doctor.sh --quiet`
2. `node scripts/fetch-model.mjs --verify` (re-hash the model against the lock; abort on mismatch)
3. Verify `vendor/runtime/runtime.lock.json` sha256 == `shasum -a 256 vendor/runtime/llama-server-*`
4. `pnpm --dir apps/desktop tauri build --bundles app --config src-tauri/tauri.bundle.conf.json`
5. `node scripts/verify-payload.mjs target/release/bundle/macos/NanoNexus.app`: re-hash the
   **exported** `Resources/models/model.gguf` and `MacOS/llama-server` against the locks, check
   `lipo` and `otool` again, and check that `Info.plist` has the usage keys.
6. `codesign --force --deep --options runtime --sign "$NEXUS_SIGN_IDENTITY" NanoNexus.app`
   (Tauri can do this when `signingIdentity` is set; keep one place, in Tauri config or the
   script, not both). Then `codesign --verify --deep --strict`.
7. Optional: `ditto -c -k --keepParent NanoNexus.app NanoNexus.zip`.

### Stable signing identity (important for TCC)

TCC ties folder grants to the app's code-signing identity. **Ad-hoc signing (`-`) produces a new
identity on every build**, so macOS asks for Documents/Desktop access again after each rebuild.
Fix (one time):
1. Keychain Access → Certificate Assistant → *Create a Certificate…* → Name `NanoNexus Local Signing`,
   Identity Type *Self-Signed Root*, Certificate Type **Code Signing**.
2. `export NEXUS_SIGN_IDENTITY="NanoNexus Local Signing"` (put it in your shell profile).
3. Gatekeeper: the app is built locally and not quarantined, so it opens normally. If you copy
   it to another Mac, run `xattr -dr com.apple.quarantine NanoNexus.app` there.

Notarization isn't needed for personal use. It's documented as optional in M14.

## 6. Hardened runtime and entitlements

Set `--options runtime` (hardened runtime). Entitlements (`src-tauri/entitlements.plist`):
- `com.apple.security.cs.allow-jit` = true (the WKWebView JS JIT, which Tauri apps commonly need with hardened runtime)
- `com.apple.security.cs.disable-library-validation` = **not needed** (the sidecar is statically linked)
- No App Sandbox keys (ADR-010).

Spawning `llama-server` from a hardened app is fine. It's signed by the same identity (Tauri
signs externalBins with the app identity).
