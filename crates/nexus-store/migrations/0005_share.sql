-- LAN Share (LAN-SHARE §5). Numbered 0005 because the migration list is gapless; the spec's "0006" is this file.
CREATE TABLE share_devices (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  token_hash TEXT NOT NULL UNIQUE,
  policy TEXT NOT NULL CHECK (policy IN ('chat_only','chat_read','full')),
  user_agent TEXT,
  last_ip TEXT,
  created_at INTEGER NOT NULL,
  last_seen_at INTEGER,
  expires_at INTEGER NOT NULL
);

ALTER TABLE conversations ADD COLUMN origin TEXT NOT NULL DEFAULT 'desktop';  -- 'desktop' | 'remote:<device_id>'
