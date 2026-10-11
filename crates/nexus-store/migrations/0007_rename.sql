-- 0007_rename.sql: Rename Forge sessions and their per-symbol decisions (M09).
CREATE TABLE rename_sessions (
  id TEXT PRIMARY KEY,
  root TEXT NOT NULL,
  langs TEXT NOT NULL,                          -- JSON array of language names found in the index
  index_path TEXT NOT NULL,                     -- cache/<id>/index.bin (postcard)
  status TEXT NOT NULL,                         -- indexing|ready|proposing|planned|applied|failed
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
CREATE INDEX rename_sessions_root ON rename_sessions(root, updated_at);

CREATE TABLE rename_items (
  session_id TEXT NOT NULL REFERENCES rename_sessions(id) ON DELETE CASCADE,
  symbol_id TEXT NOT NULL,
  old_name TEXT NOT NULL,
  kind TEXT NOT NULL,
  proposed TEXT,
  final TEXT,
  confidence REAL,
  reason TEXT,
  issues TEXT,                                  -- JSON array of rule ids
  flags TEXT,                                   -- JSON array: public_api, ambiguous, …
  status TEXT NOT NULL CHECK (status IN ('candidate','proposed','accepted','rejected','needs_manual','applied')),
  include_strings INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (session_id, symbol_id)
);
CREATE INDEX rename_items_status ON rename_items(session_id, status);
