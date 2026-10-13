# Data Model and IPC Contract

## 1. SQLite (`crates/nexus-store`)

Connection: `rusqlite` with the `bundled` feature (includes FTS5). Pragmas on open:
`journal_mode=WAL; synchronous=NORMAL; foreign_keys=ON; busy_timeout=5000`.
Migrations: `crates/nexus-store/migrations/NNNN_name.sql`, applied in order inside a transaction,
tracked with `PRAGMA user_version`. **Never edit an applied migration; add a new one.**
Timestamps are `INTEGER` unix milliseconds (UTC). IDs are UUIDv7 text (sortable).

```sql
-- 0001_init.sql
CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);           -- value = JSON

CREATE TABLE assistants (
  id TEXT PRIMARY KEY, source TEXT NOT NULL CHECK (source IN ('builtin_override','user')),
  toml TEXT NOT NULL, enabled INTEGER NOT NULL DEFAULT 1, sort INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL);
-- (built-ins come from the binary; this table stores user ones and per-builtin enabled/sort overrides)

CREATE TABLE conversations (
  id TEXT PRIMARY KEY, title TEXT NOT NULL DEFAULT 'New chat', assistant_id TEXT NOT NULL,
  pinned INTEGER NOT NULL DEFAULT 0, summary TEXT, summary_upto TEXT,        -- message id covered by summary
  created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, archived INTEGER NOT NULL DEFAULT 0);
CREATE INDEX conversations_updated ON conversations(updated_at DESC);

CREATE TABLE messages (
  id TEXT PRIMARY KEY, conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
  role TEXT NOT NULL CHECK (role IN ('system','user','assistant','tool','event')),
  content TEXT NOT NULL, reasoning TEXT,                                     -- thoughts (never re-sent)
  assistant_id TEXT, model_id TEXT,
  tool_trace TEXT,                                                           -- JSON array of {tool,args,summary,ok}
  status TEXT NOT NULL DEFAULT 'done' CHECK (status IN ('streaming','done','stopped','error')),
  error TEXT, tokens_prompt INTEGER, tokens_out INTEGER, tps REAL, ttft_ms INTEGER,
  created_at INTEGER NOT NULL);
CREATE INDEX messages_conv ON messages(conversation_id, created_at);
CREATE VIRTUAL TABLE messages_fts USING fts5(content, content='messages', content_rowid='rowid');
-- + triggers messages_ai / messages_ad / messages_au to keep FTS in sync

CREATE TABLE attachments (
  id TEXT PRIMARY KEY, message_id TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
  kind TEXT NOT NULL CHECK (kind IN ('file','folder','text','selection')),
  path TEXT, label TEXT NOT NULL, tokens INTEGER, snapshot TEXT);             -- snapshot = text actually sent (truncated)

-- 0002_fs.sql
CREATE TABLE grants (
  id TEXT PRIMARY KEY, root TEXT NOT NULL, level TEXT NOT NULL CHECK (level IN ('read','read_write','full')),
  recursive INTEGER NOT NULL DEFAULT 1, label TEXT, expires_at INTEGER, created_at INTEGER NOT NULL);
CREATE UNIQUE INDEX grants_root ON grants(root);
CREATE TABLE deny_rules (id TEXT PRIMARY KEY, root TEXT NOT NULL UNIQUE, created_at INTEGER NOT NULL);
CREATE TABLE audit_log (
  id INTEGER PRIMARY KEY AUTOINCREMENT, ts INTEGER NOT NULL, actor TEXT NOT NULL,   -- assistant id | 'editor' | 'user'
  action TEXT NOT NULL, path TEXT NOT NULL, allowed INTEGER NOT NULL, detail TEXT);
CREATE INDEX audit_ts ON audit_log(ts DESC);
CREATE TABLE changesets (
  id TEXT PRIMARY KEY, title TEXT NOT NULL, origin TEXT NOT NULL, ops TEXT NOT NULL,  -- JSON
  status TEXT NOT NULL CHECK (status IN ('planned','applied','rolled_back','undone','discarded','failed')),
  stats TEXT, error TEXT, created_at INTEGER NOT NULL, applied_at INTEGER, undone_at INTEGER);

-- 0003_models_jobs.sql
CREATE TABLE models (
  id TEXT PRIMARY KEY, source TEXT NOT NULL CHECK (source IN ('builtin','download','import')),
  name TEXT NOT NULL, repo TEXT, revision TEXT, file TEXT, path TEXT NOT NULL,
  sha256 TEXT, bytes INTEGER NOT NULL, quant TEXT, catalog_id TEXT, gguf_meta TEXT, added_at INTEGER NOT NULL);
CREATE TABLE downloads (
  id TEXT PRIMARY KEY, catalog_id TEXT, repo TEXT NOT NULL, revision TEXT NOT NULL, file TEXT NOT NULL,
  dest TEXT NOT NULL, sha256 TEXT NOT NULL, bytes INTEGER NOT NULL, done_bytes INTEGER NOT NULL DEFAULT 0,
  status TEXT NOT NULL CHECK (status IN ('queued','running','paused','failed','done','cancelled')),
  error TEXT, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL);
CREATE TABLE jobs (
  id TEXT PRIMARY KEY, kind TEXT NOT NULL, title TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('queued','running','done','failed','cancelled')),
  progress REAL, detail TEXT, result_ref TEXT, error TEXT, created_at INTEGER NOT NULL, finished_at INTEGER);

-- 0004_reports_notifications.sql
CREATE TABLE reports (
  id TEXT PRIMARY KEY, kind TEXT NOT NULL,                                  -- folder|file|rename|secrets|logs|data
  target TEXT NOT NULL, title TEXT NOT NULL, summary TEXT, json_path TEXT NOT NULL,
  ai_narrative TEXT, created_at INTEGER NOT NULL);
CREATE TABLE notifications (
  id TEXT PRIMARY KEY, level TEXT NOT NULL CHECK (level IN ('info','success','warning','error')),
  kind TEXT NOT NULL, title TEXT NOT NULL, body TEXT, action TEXT,          -- JSON {label, route, args}
  read INTEGER NOT NULL DEFAULT 0, created_at INTEGER NOT NULL);
CREATE TABLE watches (
  id TEXT PRIMARY KEY, root TEXT NOT NULL, schedule TEXT NOT NULL CHECK (schedule IN ('daily','weekly','on_change')),
  thresholds TEXT, last_report_id TEXT, last_run_at INTEGER, enabled INTEGER NOT NULL DEFAULT 1, created_at INTEGER NOT NULL);

-- 0005_rename.sql
CREATE TABLE rename_sessions (
  id TEXT PRIMARY KEY, root TEXT NOT NULL, langs TEXT NOT NULL, index_path TEXT NOT NULL,   -- cache/<id>/index.bin
  status TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL);
CREATE TABLE rename_items (
  session_id TEXT NOT NULL REFERENCES rename_sessions(id) ON DELETE CASCADE, symbol_id TEXT NOT NULL,
  old_name TEXT NOT NULL, kind TEXT NOT NULL, proposed TEXT, final TEXT, confidence REAL, reason TEXT,
  issues TEXT, flags TEXT, status TEXT NOT NULL CHECK (status IN ('candidate','proposed','accepted','rejected','needs_manual','applied')),
  include_strings INTEGER NOT NULL DEFAULT 0, PRIMARY KEY (session_id, symbol_id));
```

### Settings keys (typed in `nexus-core::settings::Settings`, stored as one JSON per key)
| Key | Default |
|---|---|
| `ui.theme` | `"system"` (`light`/`dark`/`system`) |
| `ui.accent` | `"violet"` |
| `ui.show_perf_stats` | `true` |
| `runtime.ctx_size` | `8192` |
| `runtime.gpu_layers` | `"all"` |
| `runtime.flash_attn` | `"auto"` |
| `runtime.threads` | `null` (auto) |
| `app.quit_on_window_close` | `false` on macOS (the app stays in the Dock and the server keeps running until ⌘Q), `true` on Windows/Linux |
| `runtime.idle_unload_minutes` | `0` (never). If > 0, stop the server after N idle minutes and restart it on the next request |
| `models.active_model_id` | `"builtin"` |
| `privacy.allow_network_downloads` | `true` |
| `tools.web_enabled` | `false` (ADR-016: lets `search_web`/`fetch_url` run) |
| `chat.auto_apply_proposals` | `false` (opt-in: write what an assistant proposes right away, via the journaled apply; off = Apply button) |
| `tools.disabled` | `{}` (assistant id → tool names switched off) |
| `notifications.os_when_unfocused` | `true` |
| `notifications.kinds` | all `true` |
| `journal.retention_days` | `30` |
| `journal.max_bytes` | `2147483648` |
| `models.extra_dirs` | `[]` |
| `share.enabled` / `share.autostart` | `false` / `false` |
| `share.port` | `9889` |
| `share.bind` | `"0.0.0.0"` |
| `share.mdns` / `share.mdns_name` | `true` / `"nanonexus"` |
| `share.https` | `false` |
| `share.default_policy` | `"chat_read"` |
| `share.allowed_assistants` | `["*"]` |
| `share.session_days` / `share.max_devices` / `share.notify_pairing` | `30` / `5` / `true` |

## 2. Commands (Rust → TS via tauri-specta)

Naming: `domain_action`. All are `async` and return `Result<T, IpcError>`.

| Domain | Command | Args → Returns |
|---|---|---|
| app | `app_info` | → `{version, os, arch, data_dir, log_dir}` |
| app | `settings_get` / `settings_set` | `key?` → `Settings` / `{key, value}` → `()` |
| runtime | `runtime_status` | → `RuntimeState` |
| runtime | `runtime_restart` | → `()` |
| runtime | `runtime_logs` | `{lines}` → `Vec<String>` |
| chat | `chat_send` | `{conversation_id?, assistant_id, text, attachments: Vec<AttachmentInput>, think: bool, on_event: Channel<ChatEvent>}` → `{conversation_id, user_message_id, assistant_message_id}` |
| chat | `chat_stop` | `{assistant_message_id}` → `()` |
| chat | `chat_regenerate` | `{assistant_message_id, on_event}` → ids |
| chat | `chat_edit_resend` | `{user_message_id, text, on_event}` → ids |
| conv | `conversations_list` | `{query?, limit, before?}` → `Vec<ConversationSummary>` |
| conv | `conversation_get` | `{id}` → `{conversation, messages}` |
| conv | `conversation_update` | `{id, title?, pinned?, assistant_id?, archived?}` → `()` |
| conv | `conversation_delete` / `conversation_export_md` | `{id}` → `()` / `String` |
| assistants | `assistants_list` / `assistant_get` | → `Vec<AssistantCard>` / `AssistantSpec` |
| assistants | `assistant_save` | `{toml}` → `AssistantSpec` (validated) |
| assistants | `assistant_delete` / `assistant_duplicate` / `assistant_export` / `assistant_import` | … |
| assistants | `assistant_validate` | `{toml}` → `Vec<FieldError>` (live builder validation + token count) |
| models | `models_catalog` / `models_installed` | → lists |
| models | `models_search_hf` | `{query}` → `Vec<HfModelHit>` |
| models | `models_repo_files` | `{repo}` → `Vec<HfFile>` |
| models | `models_download_start` | `{catalog_id? | repo+file, on_progress: Channel<DownloadProgress>}` → `download_id` |
| models | `models_download_pause` / `_resume` / `_cancel` | `{download_id}` |
| models | `models_activate` / `models_delete` / `models_import` / `models_verify` | … |
| fs | `fs_grants_list` / `fs_grant_add` / `fs_grant_update` / `fs_grant_revoke` | … |
| fs | `fs_pick_folder` | `{level}` → `Option<Grant>` (opens NSOpenPanel, then creates the grant) |
| fs | `fs_list_dir` / `fs_read_text` / `fs_stat` | permission-checked reads for the editor and file tree |
| fs | `permission_respond` | `{request_id, decision}` → `()` |
| fs | `os_permissions_status` / `os_open_privacy_settings` | → `OsPermissionStatus` / `{pane}` |
| fs | `audit_list` | `{limit, before?}` → `Vec<AuditEntry>` |
| changes | `changesets_list` / `changeset_preview` / `changeset_apply` / `changeset_undo` / `changeset_discard` | … |
| editor | `editor_save` | `{path, contents, base_hash}` → `ChangeSetId` (single-file Write op; the UI confirms, then apply) |
| editor | `editor_ai_action` | `{action, path?, selection, language, on_event}` → ids |
| analyze | `analyze_folder_start` | `{root, options, on_progress: Channel<ScanProgress>}` → `job_id` (result: report id) |
| analyze | `analyze_file` | `{path}` → `FileReport` |
| analyze | `reports_list` / `report_get` / `report_export` / `report_delete` | … |
| analyze | `report_narrative` | `{report_id, on_event}` → streams the AI narrative |
| analyze | `cleanup_plan` | `{report_id, selection}` → `ChangeSetId` |
| rename | `rename_session_create` | `{root, langs?, on_progress}` → `session_id` (indexes) |
| rename | `rename_items` | `{session_id, filter}` → `Vec<RenameItem>` |
| rename | `rename_propose` | `{session_id, item_ids?, on_progress}` → `job_id` |
| rename | `rename_item_update` | `{session_id, symbol_id, final?, status?, include_strings?}` → `RenameItem` (revalidated) |
| rename | `rename_plan` | `{session_id}` → `ChangeSetId` |
| rename | `rename_verify` | `{session_id, on_output: Channel<String>}` → `VerifyResult` |
| jobs | `jobs_list` / `job_cancel` | … |
| notify | `notifications_list` / `notifications_mark_read` / `notifications_clear` | … |
| watches | `watches_list` / `watch_add` / `watch_update` / `watch_delete` / `watch_run_now` | … |
| models | `models_pack_export` / `models_pack_import` / `models_pack_cancel` / `models_pick_folder` | `{job_id, ids, dest, name?, on_event: Channel<PackProgress>}` → `PackExport` / `{job_id, path, copy, on_event}` → `PackImport` (one bad file does not stop the others) / `{job_id}` / `()`. Event `models:list_changed` when models are added in the background (offline kit). Desktop caller only. |
| models | `models_extra_dirs_set` / `models_verify_bundled` | … |
| share | `share_status` / `share_start` / `share_stop` / `share_settings_set` / `share_pairing_code` / `share_devices_list` / `share_device_update` / `share_device_revoke` | see LAN-SHARE §6 (desktop caller only) |

### Streaming payloads
```rust
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChatEvent {
  Started { assistant_message_id: String },
  Status { text: String },                                 // "Reading ~/Downloads…"
  ToolCall { tool: String, args: serde_json::Value },
  ToolResult { tool: String, summary: String, ok: bool },
  PermissionNeeded { request_id: String, path: String, needed: String },
  Reasoning { delta: String },
  Content { delta: String },
  Done { finish_reason: String, tokens_out: u32, tps: f32, ttft_ms: u32 },
  Error { code: ErrorCode, message: String },
}
```

## 3. Global events (`app.emit`)
| Event | Payload | Emitted when |
|---|---|---|
| `runtime:status` | `RuntimeState` | Any supervisor state change |
| `model:changed` | `{model_id, name}` | Activation finished |
| `permission:request` | `PermissionRequest` | A tool needs a grant (also mirrored in ChatEvent) |
| `job:update` | `JobSummary` | Job created, progress (≤ 4 Hz), or finished |
| `notification:new` | `Notification` | Any notification inserted |
| `changeset:update` | `{id, status}` | Applied, undone, or failed |
| `share:status` | `{running, port, urls, devices, error?}` | Sharing started, stopped, a device paired or revoked, or an error |

## 4. Error codes (`nexus-core::error::ErrorCode`)

| Code | User copy (UI) | Suggested action |
|---|---|---|
| `runtime_unavailable` | "The model isn't running yet." | Show runtime status / Retry |
| `model_missing` | "No model installed." | Open Models |
| `permission_denied` | "NanoNexus doesn't have access to this folder." | Grant access… |
| `os_permission_denied` | "macOS blocked access to {location}." | Open Privacy Settings |
| `path_denied` | "This location is protected and can't be used." | — |
| `not_found` | "Not found." | — |
| `invalid_input` | field message | — |
| `llm_invalid_output` | "The model gave an unusable answer." | Retry / try a bigger model |
| `context_overflow` | "That's too much text for this model at once." | Summarize first / raise ctx |
| `cancelled` | (silent) | — |
| `conflict` | "The file changed since the plan was made." | Re-plan |
| `checksum_mismatch` | "Download corrupted; the file was removed." | Retry |
| `insufficient_disk` | "Not enough disk space: need {need}, have {have}." | — |
| `network` | "Network error." | Retry |
| `busy` | "Busy with another task." | Wait / cancel the job |
| `port_in_use` | "Port {port} is already in use." | Use the suggested free port |
| `unauthorized` | "This device isn't paired (or was revoked)." | Pair again |
| `forbidden_remote` | "That needs the NanoNexus app on your computer." | — |
| `internal` | "Something went wrong." | View logs |

`IpcError { code, message, detail }`: `detail` contains the debug chain (shown only in an
expandable "Details" section and in logs).

## M17 addendum: knowledge libraries
Tables (`0006_knowledge.sql`): `kb_libraries`, `kb_documents`, `kb_chunks`, and the external-content FTS5 table
`kb_chunks_fts(heading_path, text)` (porter + unicode61, `bm25(…, 3.0, 1.0)`).
Commands: `kb_list`, `kb_detail`, `kb_add(name?, paths, reference)`, `kb_reindex(jobId, libraryId, onEvent: KbProgress) -> KbIndexResult`,
`kb_cancel(jobId)`, `kb_set_enabled`, `kb_remove`, `kb_search(query, libraryIds?, k)`, `kb_passage(chunkId)`.
Settings: `models.use_prompt_hints` (bool, default true: the catalog's optional `prompt_hint` is appended to the system prompt for that model), `knowledge.chat_context` (bool, default false), `knowledge.max_tokens` (default 1200), `knowledge.answer_style` (`strict` default | `guide`; `guide` lets chats that inject library passages explain and write adapted code, labelled; `kb_search` and the Librarian are always strict).
Citation links in answers use `https://nanonexus.local/passage/<chunk_id>`; the chat opens them as a passage dialog.
