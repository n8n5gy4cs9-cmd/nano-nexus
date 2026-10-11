-- 0001_init: bootstrap schema. T04.01 extends this file with assistants, conversations, messages,
-- FTS5 and attachments (no released database exists yet, so 0001 may still grow until then).
CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);   -- value = JSON
