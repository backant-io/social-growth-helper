-- Phase 5: viral format detection and the OGC creator. Additive only.
ALTER TABLE viral_formats ADD COLUMN skeleton TEXT NOT NULL DEFAULT '';
ALTER TABLE viral_formats ADD COLUMN media_hint TEXT NOT NULL DEFAULT 'text_only';
ALTER TABLE viral_formats ADD COLUMN observation_count INTEGER NOT NULL DEFAULT 1;
ALTER TABLE viral_formats ADD COLUMN sources_json TEXT NOT NULL DEFAULT '[]';
ALTER TABLE viral_formats ADD COLUMN example_texts_json TEXT NOT NULL DEFAULT '[]';
ALTER TABLE viral_formats ADD COLUMN origin TEXT NOT NULL DEFAULT 'detected';

ALTER TABLE ogc_drafts ADD COLUMN batch_id TEXT;
ALTER TABLE ogc_drafts ADD COLUMN similarity REAL;

-- Posts flagged as viral candidates by Scout, a watchlist scan, or a manual add (OGC-01).
-- Holds its own copy of the text and metrics so detection works without a scout run.
CREATE TABLE format_candidates (
    id TEXT PRIMARY KEY,
    profile_id TEXT NOT NULL REFERENCES profiles(id),
    platform_post_id TEXT NOT NULL,
    author_handle TEXT NOT NULL,
    author_followers INTEGER,
    text TEXT NOT NULL,
    metrics_json TEXT NOT NULL,
    posted_at TEXT,
    source TEXT NOT NULL CHECK (source IN ('scout', 'watchlist', 'manual')),
    virality_score REAL NOT NULL,
    reasons_json TEXT NOT NULL DEFAULT '[]',
    status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'extracted', 'dismissed', 'failed')),
    format_id TEXT REFERENCES viral_formats(id),
    error_message TEXT,
    created_at TEXT NOT NULL,
    UNIQUE (profile_id, platform_post_id)
);

CREATE TABLE creator_watchlist (
    id TEXT PRIMARY KEY,
    profile_id TEXT NOT NULL REFERENCES profiles(id),
    handle TEXT NOT NULL,
    followers INTEGER,
    added_at TEXT NOT NULL,
    last_scanned_at TEXT,
    UNIQUE (profile_id, handle)
);

CREATE INDEX idx_candidates_status ON format_candidates(profile_id, status, virality_score DESC);
CREATE INDEX idx_formats_status ON viral_formats(profile_id, status, updated_at DESC);
CREATE INDEX idx_ogc_batch ON ogc_drafts(profile_id, batch_id, created_at DESC);
