-- Migration 10: include published reply & OGC drafts in analytics views immediately,
-- even before the first X metrics sync populates analytics_snapshots, and match OGC/reply
-- snapshots by published_url status ID fallback when published_post_id was not stored.
-- Pure view-only DDL so views::ensure_views can safely apply this file too.

DROP VIEW IF EXISTS v_analytics_winners_flops;
DROP VIEW IF EXISTS v_analytics_by_format;
DROP VIEW IF EXISTS v_analytics_by_action;
DROP VIEW IF EXISTS v_analytics_timing_tier;
DROP VIEW IF EXISTS v_analytics_by_model;
DROP VIEW IF EXISTS v_analytics_time_series;
DROP VIEW IF EXISTS v_analytics_snapshot_history;
DROP VIEW IF EXISTS v_analytics_post_performance;
DROP VIEW IF EXISTS v_analytics_account_overview;

CREATE VIEW v_analytics_account_overview AS
SELECT
    profile_id,
    captured_at,
    substr(captured_at, 1, 10) AS day,
    COALESCE(impressions, 0) AS impressions,
    ROUND(COALESCE(engagement_rate, 0.0), 2) AS engagement_rate,
    COALESCE(profile_visits, 0) AS profile_visits,
    COALESCE(followers_total, 0) AS followers_total,
    COALESCE(
        followers_total - LAG(followers_total) OVER (PARTITION BY profile_id ORDER BY captured_at ASC),
        0
    ) AS follower_delta
FROM analytics_snapshots
WHERE scope = 'account';

CREATE VIEW v_analytics_post_performance AS
WITH ranked_snaps AS (
    SELECT
        s.*,
        ROW_NUMBER() OVER (
            PARTITION BY s.profile_id, s.platform_post_id
            ORDER BY s.captured_at DESC, s.id DESC
        ) AS rn
    FROM analytics_snapshots s
    WHERE s.scope = 'post' AND s.platform_post_id IS NOT NULL
),
latest_snaps AS (
    SELECT * FROM ranked_snaps WHERE rn = 1
),
synced_base AS (
    SELECT
        ls.profile_id,
        ls.platform_post_id,
        COALESCE(
            ls.draft_kind,
            CASE
                WHEN rd.id IS NOT NULL THEN COALESCE(rd.draft_kind, 'reply')
                WHEN od.id IS NOT NULL THEN 'ogc'
                ELSE 'external'
            END
        ) AS draft_kind,
        COALESCE(ls.draft_id, rd.id, od.id) AS draft_id,
        ls.captured_at,
        COALESCE(rd.published_at, od.published_at, ls.captured_at) AS published_at,
        substr(COALESCE(rd.published_at, od.published_at, ls.captured_at), 1, 10) AS day,
        strftime('%Y-W%W', COALESCE(rd.published_at, od.published_at, ls.captured_at)) AS week,
        ROUND(COALESCE(ls.age_hours, 0.0), 2) AS age_hours,
        COALESCE(
            rd.edited_text,
            od.edited_text,
            json_extract(ls.raw_json, '$.text'),
            ''
        ) AS post_text,
        COALESCE(op.text, '') AS context_text,
        COALESCE(op.author_handle, '') AS context_author,
        COALESCE(rd.model_used, od.model_used, 'external') AS model_used,
        CASE
            WHEN lower(COALESCE(rd.model_used, od.model_used, '')) LIKE '%claude%'
              OR lower(COALESCE(rd.model_used, od.model_used, '')) LIKE '%sonnet%'
              OR lower(COALESCE(rd.model_used, od.model_used, '')) LIKE '%opus%'
              OR lower(COALESCE(rd.model_used, od.model_used, '')) LIKE '%haiku%' THEN 'Claude'
            WHEN lower(COALESCE(rd.model_used, od.model_used, '')) LIKE '%grok%'
              OR lower(COALESCE(rd.model_used, od.model_used, '')) LIKE '%xai%' THEN 'Grok'
            WHEN COALESCE(ls.draft_kind, '') = 'external' OR (rd.id IS NULL AND od.id IS NULL) THEN 'External'
            ELSE 'Other'
        END AS model_family,
        CASE
            WHEN od.id IS NOT NULL THEN 'ogc'
            WHEN rd.id IS NULL THEN 'external'
            WHEN rd.actions_json LIKE '%like%' AND rd.actions_json LIKE '%reply%' THEN 'like+reply'
            WHEN rd.actions_json LIKE '%quote%' THEN 'quote'
            WHEN rd.actions_json LIKE '%reply%' THEN 'reply-only'
            ELSE 'other'
        END AS actions_bundle,
        od.viral_format_id AS viral_format_id,
        COALESCE(vf.name, 'None') AS viral_format_name,
        COALESCE(vf.archetype, 'none') AS archetype,
        vf.observed_win_rate AS observed_win_rate,
        vf.personal_win_rate AS personal_win_rate,
        COALESCE(rd.edit_distance, od.edit_distance, 0) AS edit_distance,
        CASE
            WHEN rd.published_at IS NOT NULL AND op.posted_at IS NOT NULL
            THEN ROUND((julianday(rd.published_at) - julianday(op.posted_at)) * 1440.0, 1)
            ELSE NULL
        END AS reply_delay_minutes,
        op.author_followers AS author_followers,
        COALESCE(ls.impressions, 0) AS impressions,
        COALESCE(ls.likes, 0) AS likes,
        COALESCE(ls.replies, 0) AS replies,
        COALESCE(ls.reposts, 0) AS reposts,
        COALESCE(ls.bookmarks, 0) AS bookmarks,
        COALESCE(ls.profile_visits, 0) AS profile_visits,
        COALESCE(ls.link_clicks, 0) AS link_clicks,
        ROUND(
            COALESCE(
                ls.engagement_rate,
                CASE
                    WHEN COALESCE(ls.impressions, 0) > 0
                    THEN ((COALESCE(ls.likes, 0) + COALESCE(ls.replies, 0) + COALESCE(ls.reposts, 0) + COALESCE(ls.bookmarks, 0)) * 100.0) / ls.impressions
                    ELSE 0.0
                END
            ),
            2
        ) AS engagement_rate,
        CASE
            WHEN COALESCE(ls.draft_kind, '') = 'external' OR (rd.id IS NULL AND od.id IS NULL) THEN 1
            ELSE 0
        END AS is_external
    FROM latest_snaps ls
    LEFT JOIN reply_drafts rd
        ON rd.profile_id = ls.profile_id
        AND (
            rd.id = ls.draft_id
            OR rd.published_post_id = ls.platform_post_id
            OR (rd.published_url IS NOT NULL AND rd.published_url LIKE '%/status/' || ls.platform_post_id || '%')
        )
    LEFT JOIN ogc_drafts od
        ON od.profile_id = ls.profile_id
        AND (
            od.id = ls.draft_id
            OR od.published_post_id = ls.platform_post_id
            OR (od.published_url IS NOT NULL AND od.published_url LIKE '%/status/' || ls.platform_post_id || '%')
        )
    LEFT JOIN observed_posts op
        ON op.id = rd.observed_post_id
    LEFT JOIN viral_formats vf
        ON vf.id = od.viral_format_id
),
unsynced_replies AS (
    SELECT
        rd.profile_id,
        COALESCE(NULLIF(rd.published_post_id, ''), 'draft:' || rd.id) AS platform_post_id,
        COALESCE(rd.draft_kind, 'reply') AS draft_kind,
        rd.id AS draft_id,
        COALESCE(rd.published_at, rd.created_at) AS captured_at,
        COALESCE(rd.published_at, rd.created_at) AS published_at,
        substr(COALESCE(rd.published_at, rd.created_at), 1, 10) AS day,
        strftime('%Y-W%W', COALESCE(rd.published_at, rd.created_at)) AS week,
        0.0 AS age_hours,
        COALESCE(rd.edited_text, '') AS post_text,
        COALESCE(op.text, '') AS context_text,
        COALESCE(op.author_handle, '') AS context_author,
        COALESCE(rd.model_used, 'other') AS model_used,
        CASE
            WHEN lower(COALESCE(rd.model_used, '')) LIKE '%claude%'
              OR lower(COALESCE(rd.model_used, '')) LIKE '%sonnet%'
              OR lower(COALESCE(rd.model_used, '')) LIKE '%opus%'
              OR lower(COALESCE(rd.model_used, '')) LIKE '%haiku%' THEN 'Claude'
            WHEN lower(COALESCE(rd.model_used, '')) LIKE '%grok%'
              OR lower(COALESCE(rd.model_used, '')) LIKE '%xai%' THEN 'Grok'
            ELSE 'Other'
        END AS model_family,
        CASE
            WHEN rd.actions_json LIKE '%like%' AND rd.actions_json LIKE '%reply%' THEN 'like+reply'
            WHEN rd.actions_json LIKE '%quote%' THEN 'quote'
            WHEN rd.actions_json LIKE '%reply%' THEN 'reply-only'
            ELSE 'other'
        END AS actions_bundle,
        NULL AS viral_format_id,
        'None' AS viral_format_name,
        'none' AS archetype,
        NULL AS observed_win_rate,
        NULL AS personal_win_rate,
        COALESCE(rd.edit_distance, 0) AS edit_distance,
        CASE
            WHEN rd.published_at IS NOT NULL AND op.posted_at IS NOT NULL
            THEN ROUND((julianday(rd.published_at) - julianday(op.posted_at)) * 1440.0, 1)
            ELSE NULL
        END AS reply_delay_minutes,
        op.author_followers AS author_followers,
        NULL AS impressions,
        NULL AS likes,
        NULL AS replies,
        NULL AS reposts,
        NULL AS bookmarks,
        NULL AS profile_visits,
        NULL AS link_clicks,
        NULL AS engagement_rate,
        0 AS is_external
    FROM reply_drafts rd
    LEFT JOIN observed_posts op
        ON op.id = rd.observed_post_id
    WHERE rd.status = 'published'
      AND NOT EXISTS (
          SELECT 1 FROM latest_snaps ls
          WHERE ls.profile_id = rd.profile_id
            AND (
                ls.draft_id = rd.id
                OR (rd.published_post_id IS NOT NULL AND ls.platform_post_id = rd.published_post_id)
                OR (rd.published_url IS NOT NULL AND rd.published_url LIKE '%/status/' || ls.platform_post_id || '%')
            )
      )
),
unsynced_ogc AS (
    SELECT
        od.profile_id,
        COALESCE(NULLIF(od.published_post_id, ''), 'draft:' || od.id) AS platform_post_id,
        'ogc' AS draft_kind,
        od.id AS draft_id,
        COALESCE(od.published_at, od.created_at) AS captured_at,
        COALESCE(od.published_at, od.created_at) AS published_at,
        substr(COALESCE(od.published_at, od.created_at), 1, 10) AS day,
        strftime('%Y-W%W', COALESCE(od.published_at, od.created_at)) AS week,
        0.0 AS age_hours,
        COALESCE(od.edited_text, '') AS post_text,
        '' AS context_text,
        '' AS context_author,
        COALESCE(od.model_used, 'other') AS model_used,
        CASE
            WHEN lower(COALESCE(od.model_used, '')) LIKE '%claude%'
              OR lower(COALESCE(od.model_used, '')) LIKE '%sonnet%'
              OR lower(COALESCE(od.model_used, '')) LIKE '%opus%'
              OR lower(COALESCE(od.model_used, '')) LIKE '%haiku%' THEN 'Claude'
            WHEN lower(COALESCE(od.model_used, '')) LIKE '%grok%'
              OR lower(COALESCE(od.model_used, '')) LIKE '%xai%' THEN 'Grok'
            ELSE 'Other'
        END AS model_family,
        'ogc' AS actions_bundle,
        od.viral_format_id AS viral_format_id,
        COALESCE(vf.name, 'None') AS viral_format_name,
        COALESCE(vf.archetype, 'none') AS archetype,
        vf.observed_win_rate AS observed_win_rate,
        vf.personal_win_rate AS personal_win_rate,
        COALESCE(od.edit_distance, 0) AS edit_distance,
        NULL AS reply_delay_minutes,
        NULL AS author_followers,
        NULL AS impressions,
        NULL AS likes,
        NULL AS replies,
        NULL AS reposts,
        NULL AS bookmarks,
        NULL AS profile_visits,
        NULL AS link_clicks,
        NULL AS engagement_rate,
        0 AS is_external
    FROM ogc_drafts od
    LEFT JOIN viral_formats vf
        ON vf.id = od.viral_format_id
    WHERE od.status = 'published'
      AND NOT EXISTS (
          SELECT 1 FROM latest_snaps ls
          WHERE ls.profile_id = od.profile_id
            AND (
                ls.draft_id = od.id
                OR (od.published_post_id IS NOT NULL AND ls.platform_post_id = od.published_post_id)
                OR (od.published_url IS NOT NULL AND od.published_url LIKE '%/status/' || ls.platform_post_id || '%')
            )
      )
),
base AS (
    SELECT * FROM synced_base
    UNION ALL
    SELECT * FROM unsynced_replies
    UNION ALL
    SELECT * FROM unsynced_ogc
)
SELECT
    profile_id,
    platform_post_id,
    draft_kind,
    draft_id,
    captured_at,
    published_at,
    day,
    week,
    age_hours,
    post_text,
    context_text,
    context_author,
    model_used,
    model_family,
    actions_bundle,
    viral_format_id,
    viral_format_name,
    archetype,
    observed_win_rate,
    personal_win_rate,
    edit_distance,
    reply_delay_minutes,
    CASE
        WHEN reply_delay_minutes IS NULL THEN 'unknown'
        WHEN reply_delay_minutes < 15.0 THEN '<15m'
        WHEN reply_delay_minutes < 60.0 THEN '15-60m'
        WHEN reply_delay_minutes < 180.0 THEN '1-3h'
        ELSE '>3h'
    END AS timing_bucket,
    author_followers,
    CASE
        WHEN author_followers IS NULL THEN 'unknown'
        WHEN author_followers < 1000 THEN '<1k'
        WHEN author_followers < 10000 THEN '1k-10k'
        WHEN author_followers < 100000 THEN '10k-100k'
        ELSE '100k+'
    END AS follower_tier,
    impressions,
    likes,
    replies,
    reposts,
    bookmarks,
    profile_visits,
    link_clicks,
    engagement_rate,
    is_external
FROM base;

CREATE VIEW v_analytics_time_series AS
SELECT
    profile_id,
    captured_at,
    day,
    week,
    draft_kind,
    model_family,
    impressions,
    likes,
    replies,
    reposts,
    bookmarks,
    engagement_rate,
    edit_distance,
    platform_post_id
FROM v_analytics_post_performance;

CREATE VIEW v_analytics_by_model AS
SELECT
    profile_id,
    captured_at,
    day,
    week,
    model_family,
    model_used,
    draft_kind,
    impressions,
    likes,
    replies,
    reposts,
    bookmarks,
    engagement_rate,
    edit_distance,
    platform_post_id
FROM v_analytics_post_performance;

CREATE VIEW v_analytics_timing_tier AS
SELECT
    profile_id,
    captured_at,
    day,
    timing_bucket,
    follower_tier,
    reply_delay_minutes,
    author_followers,
    draft_kind,
    model_family,
    impressions,
    engagement_rate,
    likes,
    replies,
    platform_post_id
FROM v_analytics_post_performance
WHERE draft_kind IN ('reply', 'quote');

CREATE VIEW v_analytics_by_action AS
SELECT
    profile_id,
    captured_at,
    day,
    draft_kind,
    actions_bundle,
    model_family,
    impressions,
    engagement_rate,
    likes,
    replies,
    reposts,
    bookmarks,
    edit_distance,
    platform_post_id
FROM v_analytics_post_performance;

CREATE VIEW v_analytics_by_format AS
SELECT
    profile_id,
    captured_at,
    day,
    viral_format_id,
    viral_format_name,
    archetype,
    model_family,
    impressions,
    engagement_rate,
    likes,
    replies,
    bookmarks,
    COALESCE(observed_win_rate, 0.0) AS observed_win_rate,
    COALESCE(personal_win_rate, 0.0) AS personal_win_rate,
    platform_post_id
FROM v_analytics_post_performance
WHERE draft_kind = 'ogc' OR viral_format_id IS NOT NULL;

CREATE VIEW v_analytics_winners_flops AS
WITH ranked AS (
    SELECT
        p.*,
        ROW_NUMBER() OVER (PARTITION BY p.profile_id ORDER BY p.impressions DESC, p.engagement_rate DESC) AS rank_desc,
        ROW_NUMBER() OVER (PARTITION BY p.profile_id ORDER BY p.impressions ASC, p.engagement_rate ASC) AS rank_asc,
        COUNT(*) OVER (PARTITION BY p.profile_id) AS total_posts
    FROM v_analytics_post_performance p
    WHERE p.impressions IS NOT NULL
)
SELECT
    profile_id,
    captured_at,
    platform_post_id,
    CASE
        WHEN total_posts >= 2 AND rank_desc <= MAX(1, CAST((total_posts + 9) / 10 AS INTEGER)) THEN 'winner'
        WHEN total_posts >= 2 AND rank_asc <= MAX(1, CAST((total_posts + 9) / 10 AS INTEGER)) THEN 'flop'
        WHEN total_posts = 1 THEN 'winner'
        ELSE 'middle'
    END AS tier,
    rank_desc,
    total_posts,
    draft_kind,
    model_family,
    post_text,
    context_text,
    context_author,
    impressions,
    likes,
    replies,
    reposts,
    bookmarks,
    engagement_rate,
    edit_distance,
    is_external
FROM ranked;

CREATE VIEW v_analytics_snapshot_history AS
SELECT
    profile_id,
    captured_at,
    substr(captured_at, 1, 10) AS day,
    platform_post_id,
    COALESCE(draft_kind, 'external') AS draft_kind,
    ROUND(COALESCE(age_hours, 0.0), 2) AS age_hours,
    COALESCE(impressions, 0) AS impressions,
    COALESCE(likes, 0) AS likes,
    COALESCE(replies, 0) AS replies,
    COALESCE(reposts, 0) AS reposts,
    COALESCE(bookmarks, 0) AS bookmarks,
    ROUND(COALESCE(engagement_rate, 0.0), 2) AS engagement_rate
FROM analytics_snapshots
WHERE scope = 'post' AND platform_post_id IS NOT NULL;
