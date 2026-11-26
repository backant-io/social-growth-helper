-- Social Growth Helper: SQLite schema, architecture-plan revision 2 (section 6).
-- No secrets live here; secrets stay in the OS keychain and are referenced by opaque keychain_ref ids.

CREATE TABLE profiles (
    id TEXT PRIMARY KEY,
    platform TEXT NOT NULL CHECK (platform IN ('x')),
    handle TEXT NOT NULL,
    display_name TEXT,
    browser_profile_dir TEXT NOT NULL,
    is_active INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL
);

-- SHL-10: v1 allows exactly one active profile per platform. profile_id stays on every domain table.
CREATE UNIQUE INDEX idx_profiles_one_active_per_platform ON profiles(platform) WHERE is_active = 1;

CREATE TABLE provider_accounts (
    id TEXT PRIMARY KEY,
    provider_type TEXT NOT NULL,
    label TEXT NOT NULL,
    keychain_ref TEXT,
    billing_lane TEXT NOT NULL,
    status TEXT NOT NULL,
    last_verified_at TEXT
);

CREATE TABLE task_model_routes (
    task_type TEXT PRIMARY KEY,
    provider_account_id TEXT REFERENCES provider_accounts(id),
    model_id TEXT,
    reasoning_effort TEXT,
    temperature REAL,
    fallback_account_id TEXT REFERENCES provider_accounts(id),
    fallback_model_id TEXT,
    max_calls_per_hour INTEGER,
    updated_at TEXT NOT NULL
);

-- All seven task slots ship unset (provider_account_id NULL). browser_navigation has no fixed default by decision.
INSERT INTO task_model_routes (task_type, updated_at) VALUES
    ('browser_navigation', strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    ('post_scoring', strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    ('reply_drafting', strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    ('ogc_generation', strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    ('format_extraction', strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    ('learning_analysis', strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    ('dashboard_builder', strftime('%Y-%m-%dT%H:%M:%SZ', 'now'));

CREATE TABLE style_guides (
    id TEXT PRIMARY KEY,
    profile_id TEXT NOT NULL REFERENCES profiles(id),
    version INTEGER NOT NULL,
    free_text_guide TEXT NOT NULL,
    learned_rules_json TEXT NOT NULL DEFAULT '[]',
    banned_patterns_json TEXT NOT NULL DEFAULT '[]',
    is_active INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL
);

CREATE TABLE style_examples (
    id TEXT PRIMARY KEY,
    profile_id TEXT NOT NULL REFERENCES profiles(id),
    kind TEXT NOT NULL CHECK (kind IN ('reply', 'ogc')),
    source_context_text TEXT,
    example_text TEXT NOT NULL,
    why_it_works TEXT,
    tags_json TEXT NOT NULL DEFAULT '[]',
    pinned INTEGER NOT NULL DEFAULT 0,
    source TEXT NOT NULL,
    performance_score REAL,
    created_at TEXT NOT NULL
);

CREATE TABLE observed_posts (
    id TEXT PRIMARY KEY,
    profile_id TEXT NOT NULL REFERENCES profiles(id),
    platform_post_id TEXT NOT NULL,
    author_handle TEXT NOT NULL,
    author_display_name TEXT,
    author_followers INTEGER,
    author_verified INTEGER DEFAULT 0,
    text TEXT NOT NULL,
    has_media INTEGER NOT NULL DEFAULT 0,
    media_meta_json TEXT,
    feed_source TEXT NOT NULL,
    posted_at TEXT,
    first_seen_at TEXT NOT NULL,
    metrics_at_capture_json TEXT NOT NULL,
    velocity_score REAL,
    UNIQUE (profile_id, platform_post_id)
);

CREATE TABLE scout_evaluations (
    id TEXT PRIMARY KEY,
    observed_post_id TEXT NOT NULL REFERENCES observed_posts(id),
    rubric_version INTEGER NOT NULL,
    model_used TEXT NOT NULL,
    potential_score REAL NOT NULL,
    angle_summary TEXT NOT NULL,
    reasoning TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'queued',
    dismiss_reason TEXT,
    created_at TEXT NOT NULL
);

CREATE TABLE viral_formats (
    id TEXT PRIMARY KEY,
    profile_id TEXT NOT NULL REFERENCES profiles(id),
    name TEXT NOT NULL,
    archetype TEXT NOT NULL,
    structural_pattern TEXT NOT NULL,
    hook_formula TEXT NOT NULL,
    slot_schema_json TEXT NOT NULL,
    example_post_ids_json TEXT NOT NULL DEFAULT '[]',
    observed_win_rate REAL,
    personal_win_rate REAL,
    status TEXT NOT NULL DEFAULT 'active',
    detected_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE reply_drafts (
    id TEXT PRIMARY KEY,
    profile_id TEXT NOT NULL REFERENCES profiles(id),
    observed_post_id TEXT NOT NULL REFERENCES observed_posts(id),
    evaluation_id TEXT REFERENCES scout_evaluations(id),
    initial_text TEXT NOT NULL,
    edited_text TEXT NOT NULL,
    edit_distance INTEGER NOT NULL DEFAULT 0,
    tone_tag TEXT,
    model_used TEXT NOT NULL,
    style_guide_version INTEGER NOT NULL,
    draft_kind TEXT NOT NULL DEFAULT 'reply',
    actions_json TEXT NOT NULL DEFAULT '["like","reply"]',
    status TEXT NOT NULL DEFAULT 'draft',
    user_approved_at TEXT,
    published_at TEXT,
    published_post_id TEXT,
    published_url TEXT,
    error_message TEXT,
    created_at TEXT NOT NULL
);

CREATE TABLE ogc_drafts (
    id TEXT PRIMARY KEY,
    profile_id TEXT NOT NULL REFERENCES profiles(id),
    viral_format_id TEXT REFERENCES viral_formats(id),
    topic_seed TEXT NOT NULL,
    initial_text TEXT NOT NULL,
    edited_text TEXT NOT NULL,
    edit_distance INTEGER NOT NULL DEFAULT 0,
    media_paths_json TEXT NOT NULL DEFAULT '[]',
    model_used TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'draft',
    user_approved_at TEXT,
    published_at TEXT,
    published_post_id TEXT,
    published_url TEXT,
    error_message TEXT,
    created_at TEXT NOT NULL
);

CREATE TABLE publish_audit_log (
    id TEXT PRIMARY KEY,
    profile_id TEXT NOT NULL REFERENCES profiles(id),
    target_kind TEXT NOT NULL CHECK (target_kind IN ('reply', 'quote', 'ogc', 'actions_only', 'single_action')),
    target_draft_id TEXT,
    approval_ref TEXT NOT NULL,
    actions_executed_json TEXT NOT NULL,
    before_screenshot_path TEXT,
    after_screenshot_path TEXT,
    success INTEGER NOT NULL,
    error_message TEXT,
    created_at TEXT NOT NULL
);

CREATE TABLE analytics_snapshots (
    id TEXT PRIMARY KEY,
    profile_id TEXT NOT NULL REFERENCES profiles(id),
    scope TEXT NOT NULL CHECK (scope IN ('account', 'post')),
    platform_post_id TEXT,
    draft_kind TEXT,
    draft_id TEXT,
    captured_at TEXT NOT NULL,
    age_hours REAL,
    impressions INTEGER,
    likes INTEGER,
    replies INTEGER,
    reposts INTEGER,
    bookmarks INTEGER,
    profile_visits INTEGER,
    link_clicks INTEGER,
    followers_total INTEGER,
    engagement_rate REAL,
    raw_json TEXT
);

CREATE TABLE learning_rubrics (
    version INTEGER PRIMARY KEY AUTOINCREMENT,
    profile_id TEXT NOT NULL REFERENCES profiles(id),
    scout_criteria_text TEXT NOT NULL,
    weights_json TEXT NOT NULL DEFAULT '{}',
    is_active INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL
);

CREATE TABLE learning_insights (
    id TEXT PRIMARY KEY,
    profile_id TEXT NOT NULL REFERENCES profiles(id),
    cycle_date TEXT NOT NULL,
    category TEXT NOT NULL,
    hypothesis TEXT NOT NULL,
    evidence_json TEXT NOT NULL,
    confidence REAL NOT NULL,
    proposed_patch_json TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'proposed',
    decided_at TEXT,
    created_at TEXT NOT NULL
);

CREATE TABLE profile_preferences (
    profile_id TEXT PRIMARY KEY REFERENCES profiles(id),
    niche_topics_json TEXT NOT NULL DEFAULT '[]',
    prefer_accounts_json TEXT NOT NULL DEFAULT '[]',
    avoid_accounts_json TEXT NOT NULL DEFAULT '[]',
    languages_json TEXT NOT NULL DEFAULT '["en"]',
    updated_at TEXT NOT NULL
);

CREATE TABLE app_acknowledgements (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    notice_version INTEGER NOT NULL,
    acknowledged_at TEXT NOT NULL
);

CREATE TABLE scout_sessions (
    id TEXT PRIMARY KEY,
    profile_id TEXT NOT NULL REFERENCES profiles(id),
    feed_source TEXT NOT NULL,
    max_duration_sec INTEGER NOT NULL,
    max_posts INTEGER NOT NULL,
    stop_after_queued INTEGER,
    started_at TEXT NOT NULL,
    ended_at TEXT,
    ended_reason TEXT,
    posts_scanned INTEGER NOT NULL DEFAULT 0,
    posts_queued INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE dashboard_layouts (
    id TEXT PRIMARY KEY,
    profile_id TEXT NOT NULL REFERENCES profiles(id),
    name TEXT NOT NULL,
    version INTEGER NOT NULL,
    is_active INTEGER NOT NULL DEFAULT 0,
    is_starter INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL
);

CREATE TABLE dashboard_widgets (
    id TEXT PRIMARY KEY,
    layout_id TEXT NOT NULL REFERENCES dashboard_layouts(id),
    title TEXT NOT NULL,
    spec_json TEXT NOT NULL,
    position_json TEXT NOT NULL,
    source_prompt TEXT,
    created_at TEXT NOT NULL
);

CREATE INDEX idx_observed_seen ON observed_posts(profile_id, first_seen_at DESC);
CREATE INDEX idx_eval_queue ON scout_evaluations(status, potential_score DESC);
CREATE INDEX idx_reply_status ON reply_drafts(profile_id, status, created_at DESC);
CREATE INDEX idx_analytics_post ON analytics_snapshots(profile_id, platform_post_id, captured_at DESC);
