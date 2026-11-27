-- Migration 12: state for the background analytics worker.
-- One row per tracked post (next due time, backoff, retirement) and a rolling log of browser/AI work units
-- so the hourly and daily budgets survive restarts.

CREATE TABLE analytics_sync_state (
    profile_id TEXT NOT NULL REFERENCES profiles(id),
    platform_post_id TEXT NOT NULL,
    next_due_at INTEGER NOT NULL DEFAULT 0,
    last_synced_at INTEGER,
    last_attempt_at INTEGER,
    fail_count INTEGER NOT NULL DEFAULT 0,
    missing_count INTEGER NOT NULL DEFAULT 0,
    last_heal_at INTEGER,
    retired INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (profile_id, platform_post_id)
);

CREATE TABLE analytics_budget_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    profile_id TEXT NOT NULL REFERENCES profiles(id),
    at INTEGER NOT NULL,
    units INTEGER NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('dom', 'ai_ok', 'ai_fail'))
);
CREATE INDEX idx_analytics_budget_at ON analytics_budget_log(profile_id, at);
