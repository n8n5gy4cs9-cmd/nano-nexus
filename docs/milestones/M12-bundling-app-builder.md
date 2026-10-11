# M12: Bundling, Bundle Profiles, App Builder (macOS)

**Goal:** a one-command, verified, signed `NanoNexus.app`, plus a **super easy way to choose
which models are bundled**. "Basic" ships only the default model. "Custom" ships any set of
catalog, Hugging Face, or local GGUF models, for offline machines such as a studio PC. Also
**model packs**: add models to an installed app later via USB, without rebuilding.

**Spec (normative):** [BUNDLING.md](../architecture/BUNDLING.md) §§1–8, [MODEL-MANAGER.md](../architecture/MODEL-MANAGER.md) §9,
`schemas/bundle-profile.schema.json`, `bundles/*.json`, user guide [`guide.html`](../../guide.html).
**Prereqs:** M07 (multi-model registry) and all feature milestones you want in the release.
**Exit demo:**
1. `node scripts/app-builder.mjs` opens the App Builder in your browser → tick 3 models → target
   macOS → Build → the log streams → "✅ NanoNexus.app (4.1 GB, 3 models)" → Reveal in Finder.
2. `scripts/build-app.sh --profile basic` builds the 1-model app headlessly.
3. On a second Mac with Wi-Fi **off**, the 3-model app shows all 3 as "Bundled" and switches between them.
4. Export a model pack to a folder → import it on the basic app with Wi-Fi off → the models appear, verified.

## Scope
1. **Bundle profiles** (`bundles/<name>.json`, schema-validated): `basic.json` (committed),
   `studio.example.json` (committed example). Your own profiles go in `bundles/local/*.json`
   (gitignored).
2. **`scripts/stage-bundle.mjs`**: profile + target → `vendor/bundle/<profile>/<target>/` (hardlinked
   GGUFs, `models/manifest.json`, runtime files, licenses) + a generated Tauri overlay
   `apps/desktop/src-tauri/gen/tauri.bundle.<profile>.<target>.json` (gitignored).
3. **`scripts/build-app.sh`**: `--profile <name|path> --target macos-arm64` → doctor → verify
   locks → stage → `tauri build --config <generated overlay>` → `verify-payload.mjs` → codesign →
   zip → `dist/<profile>/NanoNexus-<version>-<profile>-macos-arm64.zip`.
4. **`scripts/verify-payload.mjs`**: re-hash every bundled model and runtime file in the built
   `.app` against `manifest.json` and the locks. Also checks `lipo`, `otool`, and the Info.plist keys.
5. **`scripts/app-builder.mjs`, the App Builder** (Node 22, zero dependencies):
   - Starts `http://127.0.0.1:<random>/` with a random URL token and opens the browser.
     Serves one embedded HTML page (`scripts/app-builder/index.html`, designed with the app's tokens).
   - Page: **Models** (catalog list with checkboxes, size, RAM badge for a chosen "target machine
     RAM", license; "＋ Add from Hugging Face" (repo + quant); "＋ Add local .gguf" (path input, hashed on add)),
     **Default model** radio, **Targets** (macOS arm64 · Windows x64 · Windows arm64 (VM testing)),
     **Windows format** (Installer · Portable ZIP · Offline kit; auto-suggests Offline kit when > 1.8 GB),
     **Profile** (save/load named profiles), and a live **size summary** with warnings.
   - **Build** runs `fetch-model.mjs` for missing models, then `build-app.sh` and/or `build-windows.sh`
     per target. stdout/stderr stream to the page over Server-Sent Events. A results panel lists
     artifacts with sizes and SHA-256s, plus Reveal-in-Finder links.
   - CLI mode for scripts: `node scripts/app-builder.mjs --profile studio --target all --yes` (no browser).
6. **App runtime support for multi-model bundles:** read `resource_dir()/models/manifest.json` → register
   every bundled model (source `builtin`, read-only) → the default comes from the manifest. Also
   scan **extra model folders**: `<exe_dir>/models-extra/` (portable/offline kit), `<data>/models/`,
   and user-added folders (Settings → Models → Extra folders).
7. **Model packs** (export/import):
   - Export (Models page → select models → "Export model pack…") → a folder `NanoNexus-ModelPack-<date>/`
     with `pack.json` (ids, files, sha256, bytes, sampling presets, licenses) + `models/<id>/<file>.gguf`.
     Also available via CLI: `node scripts/make-model-pack.mjs --profile studio --out /Volumes/USB`.
   - Import (Models → "Import model pack…" → pick the folder) → verify every SHA-256 (progress) →
     copy (or reference in place with a Read grant) → register. Fully offline.

## Design notes
- **Hardlinks** (`fs.linkSync`) when staging, so multi-GB models aren't duplicated on disk. Fall
  back to a copy across volumes.
- **Manifest** (`models/manifest.json` inside the bundle):
  ```json
  {"schema":1,"profile":"studio","default":"minicpm5-1b-fable5-v2-q8","built_at":"…",
   "models":[{"id":"…","name":"…","file":"minicpm5-1b-fable5-v2-q8/MiniCPM5-…-Q8_0.gguf","sha256":"…","bytes":…,
              "repo":"…","revision":"…","license":"…","sampling":{…},"capabilities":[…]}]}
  ```
- **Size guard:** warn when the macOS `.app` is > 8 GB (copying takes long; still works). For Windows, see M13 (NSIS ~2 GB limit).
- The App Builder binds **127.0.0.1 only**, requires the random token in the URL (copied into a
  cookie), and exits after 30 minutes idle or on Ctrl-C.
- **Version stamping:** the build writes `bundle.json` into Resources with the profile, git
  commit, and build date. It's shown in Settings → About.

## Steps
1. Profile schema + `bundles/basic.json` + `bundles/studio.example.json` (done in planning; validate in a test).
2. `fetch-model.mjs` multi-model support (done in planning; harden it on the Mac).
3. App: manifest reader + multi-builtin registration + extra folders scan (`nexus-models`).
4. `stage-bundle.mjs` + generated overlay + tests (stage a fake 10 MB "model" profile).
5. `build-app.sh` + `verify-payload.mjs` + codesign + zip.
6. `app-builder.mjs` + `scripts/app-builder/index.html` (UI) + SSE log streaming + CLI mode.
7. Model packs: `make-model-pack.mjs` + in-app export/import + verification + tests.
8. Update `guide.html` from "planned" to "available" for each section that now works.

## Exit criteria
- [ ] Exit demo steps 1–4 pass.
- [ ] Tampering with one byte of a bundled model → `verify-payload` fails, and in-app "Verify" flags it.
- [ ] The app moved anywhere still finds its runtime and models. No orphan processes after quit.

## Pitfalls
- `tauri build` copies resources into the bundle, so staging uses hardlinks, but the final `.app`
  is a real copy. You need free disk ≥ 2× the model total.
- Keep generated overlays out of git (`apps/desktop/src-tauri/gen/`).
