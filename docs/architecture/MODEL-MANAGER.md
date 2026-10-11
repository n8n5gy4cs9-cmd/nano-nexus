# Model Manager and Download View (`crates/nexus-models`)

## 1. Sources of models

| Source | Where it lives | Deletable |
|---|---|---|
| **Bundled** (1..n models, chosen by the bundle profile at build time) | `<resources>/models/manifest.json` + `<resources>/models/<id>/<file>.gguf` | No (read-only bundle) |
| **Offline kit extras** | `<exe_dir>/models-extra/pack.json` + files (Windows offline kit); verified and registered on first launch | No (part of the install) |
| **Model pack import** (USB, offline) | copied into `<data>/models/<id>/`, or referenced in place | Yes |
| **Extra folders** (user-added, e.g. a NAS share) | referenced in place (Read grant), discovered by scanning for `*.gguf` (+ optional `pack.json`) | Unlink only |
| **Downloaded** (catalog or HF search) | `<data>/models/<owner>__<repo>/<file>.gguf` | Yes |
| **Imported** (user picked a local `.gguf`) | copied into `<data>/models/local/`, or referenced in place (requires a Read grant) | Yes (copy) / Unlink (reference) |

Every installed model is a row in `models` (see DATA-AND-IPC.md) with `sha256`, `bytes`,
`gguf_meta` (JSON), `source`, `repo`, `revision`, `file`, and `added_at`. `settings.active_model_id`
picks the one the supervisor runs.

## 2. Catalog (`catalog/models.json`, embedded)

Schema: `schemas/model-catalog.schema.json`. Each entry:
```json
{
  "id": "minicpm5-1b-fable5-v2-q8",
  "name": "MiniCPM5 1B · Opus-Fable5 V2",
  "family": "MiniCPM5", "params_b": 1.08, "quant": "Q8_0",
  "repo": "GnLOLot/MiniCPM5-1B-Claude-Opus-Fable5-V2-Thinking-GGUF",
  "revision": "main",
  "file": "MiniCPM5-1B-Claude-Opus-Fable5-V2-Thinking-Q8_0.gguf",
  "file_pattern": "(?i)Q8_0\\.gguf$",
  "approx_bytes": 1100000000,
  "license": "apache-2.0 (base); fine-tune data licensing unclear: personal use",
  "context_max": 131072, "context_default": 8192,
  "capabilities": ["chat", "code", "thinking", "tools-xml"],
  "supports_thinking_toggle": true,
  "sampling": { "no_think": {"temperature":0.7,"top_p":0.95,"min_p":0.0},
                "think":    {"temperature":0.9,"top_p":0.95,"min_p":0.0} },
  "min_ram_gb": 8, "tier": "tiny", "isDefault": true, "verified": true,
  "notes": "…"
}
```
`revision: "main"` is resolved to a commit SHA at download time and stored in the `models` row.
The catalog never hard-codes SHA-256 values; they come from the HF LFS oid at download time
(the authoritative hash of the stored content).

Entries with `"verified": false` are best guesses (repo or file name not confirmed when the
catalog was written). The downloader resolves the real file via `file_pattern` against the repo
tree, and the UI shows "community: unverified" until the first successful download.

## 3. Hugging Face API (no SDK; plain `reqwest`)

| Purpose | Request | Fields used |
|---|---|---|
| Resolve revision | `GET {HF}/api/models/{repo}/revision/{rev}` | `sha` (commit), `cardData.license`, `gated` |
| List files | `GET {HF}/api/models/{repo}/tree/{sha}?recursive=true` | `path`, `size`, `lfs.oid` (sha256), `lfs.size` |
| Search | `GET {HF}/api/models?search={q}&filter=gguf&sort=downloads&direction=-1&limit=30` | `id`, `downloads`, `likes`, `lastModified`, `tags` |
| Download | `GET {HF}/{repo}/resolve/{sha}/{file}` (follows redirects to the CDN) with `Range: bytes=N-` | `Content-Length`, `Content-Range`, `ETag` |

`{HF}` = `https://huggingface.co` (overridable with `HF_ENDPOINT` for mirrors). Optional
`Authorization: Bearer <hf_token>` for gated repos. The token is stored in the OS keychain via
`keyring` and is never written to SQLite.

Search results show GGUF files grouped by quant with size. Files over 1 GB that are split
(`-00001-of-00003.gguf`) are shown as one item, and all parts are downloaded (llama.cpp loads
split GGUFs from the first part).

## 4. Downloader (resumable, verified)

```rust
pub struct DownloadSpec { pub repo: String, pub revision_sha: String, pub file: String,
                          pub expected_sha256: String, pub expected_bytes: u64, pub dest: PathBuf }
pub async fn download(spec: DownloadSpec, progress: impl Fn(DownloadProgress), cancel: CancellationToken)
    -> Result<InstalledFile, DownloadError>;
pub struct DownloadProgress { pub bytes: u64, pub total: u64, pub bytes_per_sec: f64, pub eta_secs: Option<u64>, pub phase: Phase }
pub enum Phase { Preparing, Downloading, Verifying, Finalizing }
```

1. **Pre-flight:** free disk space ≥ `expected_bytes × 1.05 + 500 MB` (`fs2::available_space`),
   or fail with `insufficient_disk { need, have }`.
2. **Part file:** `<dest>.part` + `<dest>.part.json` `{url, sha256, bytes, etag, revision}`. If a
   part exists and the meta matches, hash the existing bytes first (Phase `Preparing`, with
   progress) and resume with `Range`.
3. Stream chunks: write with `BufWriter` (1 MiB) and update `sha2::Sha256` incrementally.
   Progress is throttled to 4 Hz, and speed is an EMA over 3 s.
4. Server ignores Range (200 instead of 206) → restart from 0 (truncate the part and reset the hasher).
5. Network errors → retry with backoff (1, 2, 4, 8, 16 s; max 5), resuming each time.
6. **Pause** = cancel while keeping the part. **Cancel** = cancel and delete the part.
7. Verify: bytes == expected and the hex sha256 matches. On mismatch, delete the part and fail
   with `checksum_mismatch`.
8. Validate GGUF (§5). Atomically rename to the final path and insert the `models` row.
9. Notify: `download:progress` final event + notification "MiniCPM5-2B is ready. [Use it]".

Only **one active download at a time** (queue the rest). This avoids disk thrash and makes ETA honest.

## 5. GGUF parsing (`gguf.rs`)

Read the header only (no tensors):
```
magic "GGUF" (u32 LE 0x46554747) · version u32 (2 or 3) · tensor_count u64 · kv_count u64
then kv_count × { key: gguf_string, type: u32, value }   // gguf_string = u64 len + bytes
```
Value types: `UINT8..INT64, FLOAT32/64, BOOL, STRING, ARRAY(type,u64 n,items)`. Skip big arrays
(tokenizer vocab) by type and length without allocating. Extract:
`general.architecture`, `general.name`, `general.file_type` (→ quant name), `{arch}.context_length`,
`{arch}.block_count`, `{arch}.embedding_length`, `{arch}.attention.head_count(_kv)`,
`tokenizer.chat_template` (present? length? contains `enable_thinking`?), and `general.license`.
Guard every length (max 64 MB for a string, max 10M array items) so a corrupt file can't OOM us.

## 6. RAM-fit estimate (`fit.rs`)

```
weights      = file_bytes
kv_cache     = 2 (K and V) × n_layers × ctx × n_kv_heads × head_dim × 2 bytes (f16)
               (head_dim = embedding_length / head_count)
overhead     = 350 MB (compute buffers, runtime)
need         = weights + kv_cache + overhead
budget (mac) = total_ram × 0.70     // Metal's recommendedMaxWorkingSetSize is ~65–75% of unified memory
budget (pc)  = available_ram × 0.85
badge: need ≤ 0.6·budget → "Runs great" · ≤ budget → "Tight" · else "Too big"
```
Show the numbers in a tooltip: "needs ≈ 1.6 GB of 11.2 GB usable".

## 7. Activation

`models_activate(id)` → validate that the file exists and the hash is cached (full re-hash is
optional via "Verify now") → `settings.active_model_id = id` → `supervisor.restart(config)` →
the UI shows the runtime status → `model:changed` event → assistants pick up the model's sampling preset.

## 8. Download view UI (summary; full design in UI-DESIGN.md)

Tabs **Installed · Discover · Search**. Model cards show the name, family chip, params, quant,
size, RAM-fit badge, capabilities chips, license, and Download / Pause / Resume / Cancel / Use /
Delete. A progress ring + bar shows speed and ETA. The active model gets a glowing "Active" pill.
The runtime panel at the top shows status, ctx size, GPU layers, tokens/s from the last answer,
and Restart / Logs buttons.

## 9. Bundled-model manifest and model packs (ADR-015)

### `models/manifest.json` (inside the app; written by `scripts/stage-bundle.mjs`)
```json
{
  "schema": 1, "profile": "studio", "built_at": "2026-10-20T12:00:00Z", "app_version": "0.1.0",
  "default": "minicpm5-1b-fable5-v2-q8",
  "models": [
    { "id": "minicpm5-1b-fable5-v2-q8", "name": "MiniCPM5 1B · Opus-Fable5 V2", "path": "minicpm5-1b-fable5-v2-q8/MiniCPM5-1B-Claude-Opus-Fable5-V2-Thinking-Q8_0.gguf",
      "sha256": "<64 hex>", "bytes": 1100000000, "repo": "GnLOLot/…", "revision": "<commit>", "license": "…",
      "catalog_id": "minicpm5-1b-fable5-v2-q8", "sampling": { "no_think": {…}, "think": {…} }, "capabilities": ["chat","code","thinking"] }
  ]
}
```
Startup: read the manifest → upsert `models` rows (source `builtin`, path absolute) → if
`settings.active_model_id` is missing or invalid, use `default`. Hashes are **not** recomputed at
every launch (1+ GB). "Verify bundled models" in the Models page does it on demand.

### Model pack (`NanoNexus-ModelPack-<name>/pack.json` + `models/<id>/<file>.gguf`)
Same `models[]` entries as the manifest (paths relative to the pack folder), plus `"kind": "model-pack"`.
- **Export** (app): select installed models → choose a destination folder → copy with progress →
  write `pack.json` (+ `README.txt` with import steps) → notification.
- **Export** (CLI, from `vendor/models`): `node scripts/make-model-pack.mjs --profile <p> | --id <id>… --out <dir>`.
- **Import** (app, offline): pick the folder → validate `pack.json` (schema `schemas/model-pack.schema.json`)
  → for each model: free space check → stream-copy while hashing (or hash in place for "reference")
  → reject any mismatch → register (`source = 'import'`) → notification "3 models imported".
- **Offline kit (Windows):** the NSIS hook copies `models-extra/` next to the exe, and the first
  launch runs the same import pipeline in "reference in place" mode.
