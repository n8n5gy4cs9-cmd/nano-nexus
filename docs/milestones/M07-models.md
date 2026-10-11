# M07: Model Manager and Download View

**Goal:** browse, download (resumable and verified), import, activate, and delete models in a
beautiful Download view. The built-in model is listed and protected.

**Spec:** [MODEL-MANAGER.md](../architecture/MODEL-MANAGER.md), `catalog/models.json`, `schemas/model-catalog.schema.json`.
**Prereqs:** M04 (M06 is nice for per-model sampling in assistants).
**Exit demo:** download MiniCPM5-2B Q4_K_M, pause at ~30%, quit the app, reopen, resume, see it
verify, get a notification, click "Use it" → the runtime restarts on it → chat works. Delete it → space is freed.

## Scope
- `nexus-models`: catalog loader + validation, HF client (revision, tree, search), downloader
  (Range resume, SHA-256 streaming, retries, pause/cancel), GGUF header parser, RAM-fit
  estimator, installed registry (store), **bundled models registration from `resources/models/manifest.json` (1..n models)**,
  extra model folders scan, verify-bundled command. (Model pack import/export UI lands in M12.)
- Store migration 0003 (models, downloads, jobs).
- App: commands `models_*`, download progress Channels, one-at-a-time download queue, activation →
  `supervisor.restart`, notifications on done/failed, `privacy.allow_network_downloads` gate.
- UI: Models page (runtime panel, Installed, Discover, Search HF, model detail drawer with GGUF metadata).

## Design notes
- `sysinfo` for total RAM. On macOS also read `hw.memsize` via sysctl as a fallback.
- Disk space via `fs4::available_space` (fs4 1.x).
- Downloads survive restarts: on startup, find `downloads` rows with status `running` → set them
  to `paused` (never auto-resume metered downloads).
- HF search is debounced (400 ms) and cached for 10 minutes in memory.
- Model ids for downloads: `"{owner}__{repo}__{file}"` (slug, lowercase).
- On activation, if the GGUF has no chat template (`tokenizer.chat_template` missing), warn:
  "This model has no chat template; replies may be poor."

## Steps
1. GGUF parser + fixtures (generate small GGUF files in a test using a writer helper; also test
   against the real model behind an ignored test).
2. Catalog + schema validation test (all entries valid, exactly one `isDefault`).
3. HF client with wiremock tests (revision, tree with lfs, search, 401 gated, 404).
4. Downloader with an axum test server supporting Range (normal, resume, 200-instead-of-206,
   corrupt → checksum mismatch, network drop → retry).
5. RAM-fit + tests (known model numbers → expected badge).
6. Store + commands + queue + notifications + activation.
7. UI: runtime panel, cards, progress ring, drawer, empty and offline states, delete confirm.

## Exit criteria
- [ ] Exit demo passes. Corrupting a `.part` byte (debug command) leads to a checksum failure.
- [ ] Offline: Discover still lists the catalog and Search shows a friendly offline state.
- [ ] Built-in model can't be deleted. Activating a missing file fails gracefully.

## Pitfalls
- HF `resolve` redirects to a CDN host. Keep the `Range` header across redirects (reqwest keeps
  headers on same-origin redirects only, so follow redirects manually: read `Location`, re-issue
  with the headers).
- Don't send the HF token to the CDN redirect host (strip `Authorization` on cross-host redirect).
