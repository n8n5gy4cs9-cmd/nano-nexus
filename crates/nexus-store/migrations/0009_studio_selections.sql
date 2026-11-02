CREATE TABLE studio_selections (
    kind TEXT PRIMARY KEY NOT NULL CHECK(kind IN ('chat','image','speech','tts')),
    model_id TEXT NOT NULL REFERENCES studio_models(id) ON DELETE CASCADE
);
