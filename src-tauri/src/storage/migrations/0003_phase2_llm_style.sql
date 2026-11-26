-- Phase 2: LLM router metadata, prompt templates, usage logs, style guide rules, and FTS5 example search.

ALTER TABLE provider_accounts ADD COLUMN config_json TEXT NOT NULL DEFAULT '{}';
ALTER TABLE provider_accounts ADD COLUMN last_error TEXT;

ALTER TABLE style_guides ADD COLUMN max_reply_chars INTEGER NOT NULL DEFAULT 220;
ALTER TABLE style_guides ADD COLUMN emoji_policy TEXT NOT NULL DEFAULT 'minimal';
ALTER TABLE style_guides ADD COLUMN hashtag_policy TEXT NOT NULL DEFAULT 'none';
ALTER TABLE style_guides ADD COLUMN note TEXT;

-- FTS5 index for few-shot example retrieval (STY-03). Synced by style::repo.
CREATE VIRTUAL TABLE style_examples_fts USING fts5(
    example_id UNINDEXED,
    profile_id UNINDEXED,
    kind UNINDEXED,
    source_context_text,
    example_text,
    why_it_works,
    tags_text
);

-- Local per-task call & token accounting + rate guard (LLM-12, LLM-17).
CREATE TABLE llm_usage_logs (
    id TEXT PRIMARY KEY,
    task_type TEXT NOT NULL,
    provider_account_id TEXT REFERENCES provider_accounts(id) ON DELETE SET NULL,
    provider_type TEXT NOT NULL,
    model_id TEXT NOT NULL,
    prompt_version INTEGER,
    input_tokens INTEGER NOT NULL DEFAULT 0,
    output_tokens INTEGER NOT NULL DEFAULT 0,
    latency_ms INTEGER NOT NULL DEFAULT 0,
    used_fallback INTEGER NOT NULL DEFAULT 0,
    success INTEGER NOT NULL DEFAULT 1,
    error_kind TEXT,
    created_at TEXT NOT NULL
);

CREATE INDEX idx_llm_usage_task_time ON llm_usage_logs(task_type, created_at DESC);
CREATE INDEX idx_llm_usage_time ON llm_usage_logs(created_at DESC);

-- Versioned prompt templates per task with local overrides (LLM-19).
CREATE TABLE prompt_templates (
    id TEXT PRIMARY KEY,
    task_type TEXT NOT NULL,
    version INTEGER NOT NULL,
    system_prompt TEXT NOT NULL,
    user_template TEXT NOT NULL,
    is_override INTEGER NOT NULL DEFAULT 0,
    is_active INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL,
    UNIQUE (task_type, version)
);
