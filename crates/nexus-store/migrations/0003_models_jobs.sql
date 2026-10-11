-- 0003_models_jobs: installed models, download queue, background jobs (DATA-AND-IPC §1, MODEL-MANAGER §1).
-- `discovered` models live in a user-added folder and are referenced in place.
CREATE TABLE models (
  id TEXT PRIMARY KEY, source TEXT NOT NULL CHECK (source IN ('builtin','download','import','discovered')),
  name TEXT NOT NULL, repo TEXT, revision TEXT, file TEXT, path TEXT NOT NULL,
  sha256 TEXT, bytes INTEGER NOT NULL, quant TEXT, catalog_id TEXT, gguf_meta TEXT,
  license TEXT, extra TEXT,                                                   -- extra = JSON {sampling, capabilities, ...}
  added_at INTEGER NOT NULL);
CREATE INDEX models_source ON models(source);

CREATE TABLE downloads (
  id TEXT PRIMARY KEY, catalog_id TEXT, repo TEXT NOT NULL, revision TEXT NOT NULL, file TEXT NOT NULL,
  dest TEXT NOT NULL, sha256 TEXT NOT NULL, bytes INTEGER NOT NULL, done_bytes INTEGER NOT NULL DEFAULT 0,
  status TEXT NOT NULL CHECK (status IN ('queued','running','paused','failed','done','cancelled')),
  error TEXT, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL);
CREATE INDEX downloads_status ON downloads(status);

CREATE TABLE jobs (
  id TEXT PRIMARY KEY, kind TEXT NOT NULL, title TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('queued','running','done','failed','cancelled')),
  progress REAL, detail TEXT, result_ref TEXT, error TEXT, created_at INTEGER NOT NULL, finished_at INTEGER);
