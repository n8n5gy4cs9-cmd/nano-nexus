-- Knowledge libraries (M17). Numbered 0006 because the migration list is gapless; the spec's "0007" is this file.
-- Purely additive: nothing else reads these tables unless a library is enabled.
CREATE TABLE kb_libraries (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  source TEXT NOT NULL CHECK (source IN ('bundled','user')),
  path TEXT NOT NULL,                          -- folder holding the documents
  mode TEXT NOT NULL DEFAULT 'copy' CHECK (mode IN ('copy','reference')),
  enabled INTEGER NOT NULL DEFAULT 1,
  doc_count INTEGER NOT NULL DEFAULT 0,
  chunk_count INTEGER NOT NULL DEFAULT 0,
  bytes INTEGER NOT NULL DEFAULT 0,
  indexed_at INTEGER,
  created_at INTEGER NOT NULL
);

CREATE TABLE kb_documents (
  id INTEGER PRIMARY KEY,
  library_id TEXT NOT NULL REFERENCES kb_libraries(id) ON DELETE CASCADE,
  path TEXT NOT NULL,                          -- relative to the library folder, '/' separators
  title TEXT,
  hash TEXT NOT NULL,                          -- BLAKE3 of the file content (incremental indexing)
  kind TEXT NOT NULL,
  pages INTEGER NOT NULL DEFAULT 0,
  bytes INTEGER NOT NULL DEFAULT 0,
  status TEXT NOT NULL CHECK (status IN ('indexed','skipped','failed')),
  note TEXT,
  UNIQUE (library_id, path)
);

CREATE TABLE kb_chunks (
  id INTEGER PRIMARY KEY,
  document_id INTEGER NOT NULL REFERENCES kb_documents(id) ON DELETE CASCADE,
  ord INTEGER NOT NULL,
  heading_path TEXT NOT NULL,                  -- breadcrumb: "Reaper manual > Routing > Sends"
  page INTEGER,
  text TEXT NOT NULL,
  tokens INTEGER NOT NULL
);
CREATE INDEX kb_chunks_doc ON kb_chunks(document_id, ord);

-- External-content FTS5: BM25 with the heading path weighted above the body (see nexus-knowledge search).
CREATE VIRTUAL TABLE kb_chunks_fts USING fts5(
  heading_path, text,
  content='kb_chunks', content_rowid='id',
  tokenize='porter unicode61'
);
CREATE TRIGGER kb_chunks_ai AFTER INSERT ON kb_chunks BEGIN
  INSERT INTO kb_chunks_fts(rowid, heading_path, text) VALUES (new.id, new.heading_path, new.text);
END;
CREATE TRIGGER kb_chunks_ad AFTER DELETE ON kb_chunks BEGIN
  INSERT INTO kb_chunks_fts(kb_chunks_fts, rowid, heading_path, text) VALUES ('delete', old.id, old.heading_path, old.text);
END;
