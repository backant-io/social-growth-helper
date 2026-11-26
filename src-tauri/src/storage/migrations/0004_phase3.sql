-- Phase 3: scout, reply drafts, publish. Additive only.

-- Every generated variant is kept (DRF-08); the draft row holds the chosen and edited text.
CREATE TABLE draft_variants (
    id TEXT PRIMARY KEY,
    draft_id TEXT NOT NULL REFERENCES reply_drafts(id) ON DELETE CASCADE,
    text TEXT NOT NULL,
    tone TEXT,
    model_used TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX idx_variants_draft ON draft_variants(draft_id, created_at);

-- Per-step state of a draft's action bundle. Retry runs only the steps that are not done (DRF-16).
CREATE TABLE draft_steps (
    draft_id TEXT NOT NULL REFERENCES reply_drafts(id) ON DELETE CASCADE,
    action TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('pending', 'done', 'skipped', 'failed', 'uncertain')),
    detail TEXT,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (draft_id, action)
);

-- One row per executed engagement step; pacing caps count these.
CREATE TABLE action_events (
    id TEXT PRIMARY KEY,
    profile_id TEXT NOT NULL REFERENCES profiles(id),
    platform_post_id TEXT,
    action TEXT NOT NULL,
    source TEXT NOT NULL CHECK (source IN ('publish', 'action_click')),
    ok INTEGER NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX idx_action_events_day ON action_events(profile_id, action, created_at);

ALTER TABLE scout_sessions ADD COLUMN threshold REAL NOT NULL DEFAULT 72;
ALTER TABLE scout_sessions ADD COLUMN summary_json TEXT;
ALTER TABLE scout_evaluations ADD COLUMN session_id TEXT;
CREATE UNIQUE INDEX idx_eval_one_per_post ON scout_evaluations(observed_post_id);
CREATE INDEX idx_action_events_post ON action_events(profile_id, platform_post_id, created_at);

-- Browsable history of scout runs, publishes, single actions, challenges and errors (SHL-21).
CREATE TABLE activity_log (
    id TEXT PRIMARY KEY,
    profile_id TEXT NOT NULL REFERENCES profiles(id),
    kind TEXT NOT NULL CHECK (kind IN ('scout', 'publish', 'action', 'challenge', 'error')),
    title TEXT NOT NULL,
    detail TEXT,
    ok INTEGER,
    screenshot TEXT,
    created_at TEXT NOT NULL
);
CREATE INDEX idx_activity_created ON activity_log(profile_id, created_at);

-- Result of the repeatable live action check, stored with the selector registry version (DRF-30).
CREATE TABLE live_check_reports (
    id TEXT PRIMARY KEY,
    profile_id TEXT NOT NULL REFERENCES profiles(id),
    registry_version INTEGER NOT NULL,
    post_url TEXT NOT NULL,
    items_json TEXT NOT NULL,
    created_at TEXT NOT NULL
);

-- Quote card and thread-parent text captured with a post, given to the model as context (SCT-20).
ALTER TABLE observed_posts ADD COLUMN context_json TEXT;
