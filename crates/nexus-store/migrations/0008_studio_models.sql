-- Shared USB model registry. Paths are relative to DIST, never a host mount path.
CREATE TABLE studio_assets (
    sha256 TEXT PRIMARY KEY NOT NULL,
    path TEXT NOT NULL UNIQUE,
    bytes INTEGER NOT NULL CHECK(bytes > 0)
);
CREATE TABLE studio_models (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    kind TEXT NOT NULL CHECK(kind IN ('chat', 'image', 'speech', 'tts')),
    variant TEXT NOT NULL,
    metadata TEXT NOT NULL
);
CREATE TABLE studio_model_assets (
    model_id TEXT NOT NULL REFERENCES studio_models(id) ON DELETE CASCADE,
    role TEXT NOT NULL,
    sha256 TEXT NOT NULL REFERENCES studio_assets(sha256),
    PRIMARY KEY(model_id, role)
);
