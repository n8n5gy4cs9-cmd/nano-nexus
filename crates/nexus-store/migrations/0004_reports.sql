-- 0004_reports: saved analysis reports and folder watches (DATA-AND-IPC §1, ANALYZER §4).
-- The report body lives in `<data>/reports/<id>.json`; this table is the index.
CREATE TABLE reports (
  id TEXT PRIMARY KEY, kind TEXT NOT NULL,                                  -- folder|file|rename|secrets|logs|data
  target TEXT NOT NULL, title TEXT NOT NULL, summary TEXT, json_path TEXT NOT NULL,
  ai_narrative TEXT, created_at INTEGER NOT NULL);
CREATE INDEX reports_created ON reports(created_at DESC);
CREATE INDEX reports_target ON reports(target, created_at DESC);

CREATE TABLE watches (
  id TEXT PRIMARY KEY, root TEXT NOT NULL, schedule TEXT NOT NULL CHECK (schedule IN ('daily','weekly','on_change')),
  last_report_id TEXT, enabled INTEGER NOT NULL DEFAULT 1, created_at INTEGER NOT NULL);
