-- 0002_fs: folder grants, user deny rules, audit log, change sets (PERMISSIONS.md, DATA-AND-IPC §1).
-- Session grants are never stored (they vanish when the app quits).
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
CREATE INDEX changesets_created ON changesets(created_at DESC);
