-- Phase 6: learning loop. Additive only: new columns and tables, no existing data is rewritten.

ALTER TABLE learning_insights ADD COLUMN original_patch_json TEXT;
ALTER TABLE learning_insights ADD COLUMN sample_size INTEGER NOT NULL DEFAULT 0;
ALTER TABLE learning_insights ADD COLUMN low_confidence INTEGER NOT NULL DEFAULT 0;
ALTER TABLE learning_insights ADD COLUMN applied_version INTEGER;
ALTER TABLE learning_insights ADD COLUMN source TEXT NOT NULL DEFAULT 'rules';
ALTER TABLE learning_insights ADD COLUMN dedupe_key TEXT;

CREATE INDEX idx_insights_status ON learning_insights(profile_id, status, created_at DESC);
CREATE UNIQUE INDEX idx_insights_dedupe ON learning_insights(profile_id, dedupe_key) WHERE dedupe_key IS NOT NULL;

CREATE TABLE learning_runs (
    id TEXT PRIMARY KEY,
    profile_id TEXT NOT NULL REFERENCES profiles(id),
    mode TEXT NOT NULL CHECK (mode IN ('light', 'deep')),
    started_at TEXT NOT NULL,
    published_items INTEGER NOT NULL,
    min_sample INTEGER NOT NULL,
    insights_created INTEGER NOT NULL,
    used_llm INTEGER NOT NULL DEFAULT 0,
    note TEXT
);

CREATE TABLE learning_changelog (
    id TEXT PRIMARY KEY,
    profile_id TEXT NOT NULL REFERENCES profiles(id),
    kind TEXT NOT NULL CHECK (kind IN ('rubric', 'style', 'examples')),
    action TEXT NOT NULL CHECK (action IN ('accepted', 'rolled_back')),
    from_version INTEGER,
    to_version INTEGER,
    insight_id TEXT,
    created_at TEXT NOT NULL
);

-- Rubric versions are immutable: only is_active may change. A change means a new row.
CREATE TRIGGER learning_rubrics_immutable
BEFORE UPDATE OF scout_criteria_text, weights_json, profile_id, created_at ON learning_rubrics
BEGIN
    SELECT RAISE(ABORT, 'rubric versions are immutable; create a new version instead');
END;
