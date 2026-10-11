-- 0001_init: bootstrap schema. T04.01 extends this file with assistants, conversations, messages,
-- FTS5 and attachments (no released database exists yet, so 0001 may still grow until then).
CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);   -- value = JSON

-- Notifications arrive with the app shell (T03.12) so the bell works from day one. T08.16's
-- migration 0004 therefore only adds reports and watches.
CREATE TABLE notifications (
  id TEXT PRIMARY KEY, level TEXT NOT NULL CHECK (level IN ('info','success','warning','error')),
  kind TEXT NOT NULL, title TEXT NOT NULL, body TEXT, action TEXT,          -- JSON {label, route}
  read INTEGER NOT NULL DEFAULT 0, created_at INTEGER NOT NULL);
CREATE INDEX notifications_created ON notifications(created_at DESC);
