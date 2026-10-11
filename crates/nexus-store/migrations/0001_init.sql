-- 0001_init: settings, notifications, assistants, conversations, messages (+FTS5), attachments.
-- Timestamps are INTEGER unix milliseconds (UTC); ids are UUIDv7 text. Never edit once released.
CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);   -- value = JSON

-- Notifications arrive with the app shell (T03.12) so the bell works from day one. T08.16's
-- migration 0004 therefore only adds reports and watches.
CREATE TABLE notifications (
  id TEXT PRIMARY KEY, level TEXT NOT NULL CHECK (level IN ('info','success','warning','error')),
  kind TEXT NOT NULL, title TEXT NOT NULL, body TEXT, action TEXT,          -- JSON {label, route}
  read INTEGER NOT NULL DEFAULT 0, created_at INTEGER NOT NULL);
CREATE INDEX notifications_created ON notifications(created_at DESC);

CREATE TABLE assistants (
  id TEXT PRIMARY KEY, source TEXT NOT NULL CHECK (source IN ('builtin_override','user')),
  toml TEXT NOT NULL, enabled INTEGER NOT NULL DEFAULT 1, sort INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL);
-- Built-ins come from the binary; this table stores user ones and per-builtin enabled/sort overrides.

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
CREATE TRIGGER messages_ai AFTER INSERT ON messages BEGIN
  INSERT INTO messages_fts(rowid, content) VALUES (new.rowid, new.content);
END;
CREATE TRIGGER messages_ad AFTER DELETE ON messages BEGIN
  INSERT INTO messages_fts(messages_fts, rowid, content) VALUES ('delete', old.rowid, old.content);
END;
CREATE TRIGGER messages_au AFTER UPDATE OF content ON messages BEGIN
  INSERT INTO messages_fts(messages_fts, rowid, content) VALUES ('delete', old.rowid, old.content);
  INSERT INTO messages_fts(rowid, content) VALUES (new.rowid, new.content);
END;

CREATE TABLE attachments (
  id TEXT PRIMARY KEY, message_id TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
  kind TEXT NOT NULL CHECK (kind IN ('file','folder','text','selection')),
  path TEXT, label TEXT NOT NULL, tokens INTEGER, snapshot TEXT);             -- snapshot = text actually sent (truncated)
CREATE INDEX attachments_message ON attachments(message_id);
