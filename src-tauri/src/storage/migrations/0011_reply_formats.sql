-- Meta-level reply formats distilled from high-performing replies (structure only, never exact wording).
CREATE TABLE reply_formats (
    id TEXT PRIMARY KEY,
    profile_id TEXT NOT NULL REFERENCES profiles(id),
    name TEXT NOT NULL,
    pattern TEXT NOT NULL,
    when_to_use TEXT NOT NULL DEFAULT '',
    why_it_works TEXT NOT NULL DEFAULT '',
    source_replies INTEGER NOT NULL DEFAULT 0,
    avg_likes REAL NOT NULL DEFAULT 0,
    enabled INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL
);

CREATE INDEX idx_reply_formats_profile ON reply_formats(profile_id, enabled, created_at DESC);
