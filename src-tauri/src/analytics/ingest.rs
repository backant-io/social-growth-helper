//! Analytics ingestor orchestrating X analytics sync via BrowserDriver hooks (ANL-01..ANL-06, ANL-12, ANL-16, ANL-17, BRW-13).

use super::extract::{
    parse_account_overview, parse_post_metrics, parse_profile_timeline, PostMetrics,
    ACCOUNT_ANALYTICS_DOM_JS, ACCOUNT_ANALYTICS_URL, ACCOUNT_GQL_FILTERS, POST_ANALYTICS_DOM_JS,
    POST_GQL_FILTERS, PROFILE_GQL_FILTERS, PROFILE_TIMELINE_DOM_JS,
};
use super::health::{check_post_metrics_drift, empty_ok_report, save_last_health, HealthIssue, HealthSeverity, SyncHealthReport};
use super::schedule::{compute_schedule, hours_since, now_unix_secs};
use crate::browser::service::BrowserService;
use crate::browser::{AnalyticsPageCapture, AnalyticsPageRequest, BrowserError};
use crate::error::{AppError, AppResult};
use crate::lifecycle::JobRegistry;
use crate::util::{new_id, now_rfc3339};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Mutex;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SyncOptions {
    /// When > 0, visits profile Posts & Replies tabs to backfill own posts from the last N days (ANL-16, ANL-17).
    #[serde(default)]
    pub backfill_days: u32,
    /// Optional explicit X handle override when the profile row does not yet store one.
    #[serde(default)]
    pub handle_override: Option<String>,
    /// Max tracked posts to visit per sync pass (keeps sync responsive).
    #[serde(default)]
    pub max_posts: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncReport {
    pub health: SyncHealthReport,
    pub used_dedicated_tab: bool,
    pub stopped_early: bool,
    pub duration_ms: u64,
}

/// Narrow async boundary so AnalyticsIngestor talks only to the BrowserDriver hook (or a test double).
#[async_trait::async_trait]
pub trait AnalyticsPageFetcher: Send + Sync {
    async fn fetch_page(&self, req: AnalyticsPageRequest) -> Result<AnalyticsPageCapture, BrowserError>;
}

#[async_trait::async_trait]
impl AnalyticsPageFetcher for BrowserService {
    async fn fetch_page(&self, req: AnalyticsPageRequest) -> Result<AnalyticsPageCapture, BrowserError> {
        self.capture_analytics_page(req).await
    }
}

fn store_account_snapshot(
    conn: &Connection,
    profile_id: &str,
    impressions: Option<i64>,
    engagement_rate: Option<f64>,
    profile_visits: Option<i64>,
    followers_total: Option<i64>,
    raw_json: &serde_json::Value,
) -> AppResult<()> {
    conn.execute(
        "INSERT INTO analytics_snapshots
         (id, profile_id, scope, platform_post_id, draft_kind, draft_id, captured_at, age_hours,
          impressions, likes, replies, reposts, bookmarks, profile_visits, link_clicks, followers_total, engagement_rate, raw_json)
         VALUES (?1, ?2, 'account', NULL, NULL, NULL, ?3, NULL, ?4, NULL, NULL, NULL, NULL, ?5, NULL, ?6, ?7, ?8)",
        params![
            new_id(),
            profile_id,
            now_rfc3339(),
            impressions,
            profile_visits,
            followers_total,
            engagement_rate,
            raw_json.to_string(),
        ],
    )?;
    Ok(())
}

fn prior_post_impressions(conn: &Connection, profile_id: &str, platform_post_id: &str) -> AppResult<Option<i64>> {
    Ok(conn
        .query_row(
            "SELECT impressions FROM analytics_snapshots
             WHERE profile_id = ?1 AND scope = 'post' AND platform_post_id = ?2 AND impressions IS NOT NULL
             ORDER BY captured_at DESC LIMIT 1",
            params![profile_id, platform_post_id],
            |r| r.get(0),
        )
        .optional()?)
}

fn store_post_snapshot(
    conn: &Connection,
    profile_id: &str,
    m: &PostMetrics,
    draft_kind: &str,
    draft_id: Option<&str>,
    age_hours: Option<f64>,
    issues: &mut Vec<HealthIssue>,
) -> AppResult<()> {
    let prior = prior_post_impressions(conn, profile_id, &m.platform_post_id)?;
    check_post_metrics_drift(&m.platform_post_id, m.impressions, m.engagement_rate, prior, issues);

    let mut raw = m.raw_json.clone();
    if let Some(obj) = raw.as_object_mut() {
        if let Some(t) = &m.text {
            obj.entry("text").or_insert_with(|| serde_json::Value::String(t.clone()));
        }
        obj.insert("extraction_source".into(), serde_json::Value::String(m.source.clone()));
    }

    conn.execute(
        "INSERT INTO analytics_snapshots
         (id, profile_id, scope, platform_post_id, draft_kind, draft_id, captured_at, age_hours,
          impressions, likes, replies, reposts, bookmarks, profile_visits, link_clicks, followers_total, engagement_rate, raw_json)
         VALUES (?1, ?2, 'post', ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, NULL, ?15, ?16)",
        params![
            new_id(),
            profile_id,
            m.platform_post_id,
            draft_kind,
            draft_id,
            now_rfc3339(),
            age_hours,
            m.impressions,
            m.likes,
            m.replies,
            m.reposts,
            m.bookmarks,
            m.profile_visits,
            m.link_clicks,
            m.engagement_rate,
            raw.to_string(),
        ],
    )?;
    Ok(())
}

/// Resolves whether a discovered post belongs to a tracked reply/OGC draft or is an `external` post (ANL-17).
fn match_draft_for_post(
    conn: &Connection,
    profile_id: &str,
    platform_post_id: &str,
    text: Option<&str>,
) -> AppResult<(String, Option<String>)> {
    if let Some((id, kind)) = conn
        .query_row(
            "SELECT id, COALESCE(draft_kind, 'reply') FROM reply_drafts
             WHERE profile_id = ?1 AND published_post_id = ?2 LIMIT 1",
            params![profile_id, platform_post_id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        )
        .optional()?
    {
        return Ok((kind, Some(id)));
    }
    if let Some(id) = conn
        .query_row(
            "SELECT id FROM ogc_drafts WHERE profile_id = ?1 AND published_post_id = ?2 LIMIT 1",
            params![profile_id, platform_post_id],
            |r| r.get::<_, String>(0),
        )
        .optional()?
    {
        return Ok(("ogc".into(), Some(id)));
    }

    // Fallback: match by exact trimmed text for drafts whose permalink wasn't captured at publish time (DRF-18).
    if let Some(raw_text) = text.map(str::trim).filter(|t| t.len() >= 12) {
        if let Some((id, kind)) = conn
            .query_row(
                "SELECT id, COALESCE(draft_kind, 'reply') FROM reply_drafts
                 WHERE profile_id = ?1 AND status = 'published' AND published_post_id IS NULL AND trim(edited_text) = ?2
                 LIMIT 1",
                params![profile_id, raw_text],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            )
            .optional()?
        {
            let _ = conn.execute(
                "UPDATE reply_drafts SET published_post_id = ?1 WHERE id = ?2",
                params![platform_post_id, id],
            );
            return Ok((kind, Some(id)));
        }
        if let Some(id) = conn
            .query_row(
                "SELECT id FROM ogc_drafts
                 WHERE profile_id = ?1 AND status = 'published' AND published_post_id IS NULL AND trim(edited_text) = ?2
                 LIMIT 1",
                params![profile_id, raw_text],
                |r| r.get::<_, String>(0),
            )
            .optional()?
        {
            let _ = conn.execute(
                "UPDATE ogc_drafts SET published_post_id = ?1 WHERE id = ?2",
                params![platform_post_id, id],
            );
            return Ok(("ogc".into(), Some(id)));
        }
    }

    Ok(("external".into(), None))
}

fn profile_handle(conn: &Connection, profile_id: &str) -> AppResult<Option<String>> {
    let h: Option<String> = conn
        .query_row("SELECT handle FROM profiles WHERE id = ?1", [profile_id], |r| r.get(0))
        .optional()?;
    Ok(h.map(|s| s.trim().trim_start_matches('@').to_string()).filter(|s| !s.is_empty()))
}

fn update_profile_handle_if_empty(conn: &Connection, profile_id: &str, handle: &str) {
    let clean = handle.trim().trim_start_matches('@');
    if !clean.is_empty() {
        let _ = conn.execute(
            "UPDATE profiles SET handle = ?1 WHERE id = ?2 AND (handle IS NULL OR handle = '')",
            params![clean, profile_id],
        );
    }
}

/// Runs a complete analytics sync pass using the browser driver hook (ANL-01..ANL-06, ANL-12, ANL-16, ANL-17).
pub async fn sync_stats(
    db: &Mutex<Connection>,
    profile_id: &str,
    fetcher: &dyn AnalyticsPageFetcher,
    jobs: &JobRegistry,
    opts: SyncOptions,
) -> AppResult<SyncReport> {
    let _job_guard = jobs.register("Analytics sync");
    let started = std::time::Instant::now();
    let now_unix = now_unix_secs();

    let mut health = empty_ok_report("graphql", "premium");
    let mut used_dedicated_tab = false;
    let mut stopped_early = false;

    // 1. Account overview capture (`x.com/i/account_analytics`)
    let acc_cap = fetcher
        .fetch_page(AnalyticsPageRequest {
            url: ACCOUNT_ANALYTICS_URL.to_string(),
            network_url_filters: ACCOUNT_GQL_FILTERS.iter().map(|s| s.to_string()).collect(),
            dom_script: ACCOUNT_ANALYTICS_DOM_JS.to_string(),
            settle_ms: 2500,
        })
        .await
        .map_err(AppError::Browser)?;
    used_dedicated_tab |= acc_cap.used_dedicated_tab;

    let account = parse_account_overview(&acc_cap.network_responses, &acc_cap.dom_result);
    if account.is_logged_out {
        health.issues.push(HealthIssue {
            code: "logged_out".into(),
            severity: HealthSeverity::Error,
            message: "X session is logged out; analytics sync halted.".into(),
            hint: "Open the X window from Settings and sign in to X, then run Sync Stats again.".into(),
        });
        health.recompute_status();
        let conn = db.lock().unwrap();
        let _ = save_last_health(&conn, &health);
        return Ok(SyncReport {
            health,
            used_dedicated_tab,
            stopped_early: true,
            duration_ms: started.elapsed().as_millis() as u64,
        });
    }

    let mut detected_handle = opts
        .handle_override
        .clone()
        .map(|h| h.trim().trim_start_matches('@').to_string())
        .filter(|h| !h.is_empty())
        .or_else(|| account.handle.clone());

    {
        let conn = db.lock().unwrap();
        if let Some(h) = &detected_handle {
            update_profile_handle_if_empty(&conn, profile_id, h);
        } else {
            detected_handle = profile_handle(&conn, profile_id)?;
        }
    }

    let mut non_premium_mode = false;
    match account.source.as_str() {
        "graphql" => {
            health.extraction_mode = "graphql".into();
            health.tier_mode = "premium".into();
            health.account_synced = true;
            let conn = db.lock().unwrap();
            store_account_snapshot(
                &conn,
                profile_id,
                account.impressions,
                account.engagement_rate,
                account.profile_visits,
                account.followers_total,
                &account.raw_json,
            )?;
        }
        "dom_fallback" => {
            health.extraction_mode = "dom_fallback".into();
            health.tier_mode = "premium".into();
            health.account_synced = true;
            health.issues.push(HealthIssue {
                code: "account_dom_fallback".into(),
                severity: HealthSeverity::Warning,
                message: "AccountOverviewQuery GraphQL was not captured; fell back to DOM extraction.".into(),
                hint: "Metrics were still captured. If values look incomplete, check X analytics page layout.".into(),
            });
            let conn = db.lock().unwrap();
            store_account_snapshot(
                &conn,
                profile_id,
                account.impressions,
                account.engagement_rate,
                account.profile_visits,
                account.followers_total,
                &account.raw_json,
            )?;
        }
        "non_premium_upsell" => {
            non_premium_mode = true;
            health.extraction_mode = "public_counters".into();
            health.tier_mode = "non_premium_fallback".into();
            health.issues.push(HealthIssue {
                code: "non_premium_fallback".into(),
                severity: HealthSeverity::Warning,
                message: "X account_analytics requires X Premium; switched to public profile & post counters (ANL-06).".into(),
                hint: "Dashboard metrics will populate from public view counts and engagement counters.".into(),
            });
        }
        _ => {
            health.extraction_mode = "empty".into();
            health.issues.push(HealthIssue {
                code: "account_extraction_empty".into(),
                severity: HealthSeverity::Warning,
                message: "Could not extract account overview numbers from X analytics.".into(),
                hint: "X may have updated its analytics DOM or the page was still loading.".into(),
            });
        }
    }

    // 2. Per-post metric ingest for tracked posts prioritized by time-series schedule (ANL-03, ANL-05)
    let schedule = {
        let conn = db.lock().unwrap();
        compute_schedule(&conn, profile_id, now_unix)?
    };
    let max_posts = opts.max_posts.unwrap_or(25).clamp(1, 100);
    let mut synced_post_ids: HashSet<String> = HashSet::new();

    for item in schedule.items.iter().take(max_posts) {
        if jobs.stop_requested() {
            stopped_early = true;
            break;
        }
        let target_url = if let Some(u) = &item.published_url {
            if non_premium_mode {
                u.trim_end_matches('/').to_string()
            } else {
                format!("{}/analytics", u.trim_end_matches('/'))
            }
        } else if let Some(h) = &detected_handle {
            if non_premium_mode {
                format!("https://x.com/{h}/status/{}", item.platform_post_id)
            } else {
                format!("https://x.com/{h}/status/{}/analytics", item.platform_post_id)
            }
        } else {
            format!("https://x.com/i/web/status/{}", item.platform_post_id)
        };

        let capture = match fetcher
            .fetch_page(AnalyticsPageRequest {
                url: target_url,
                network_url_filters: POST_GQL_FILTERS.iter().map(|s| s.to_string()).collect(),
                dom_script: POST_ANALYTICS_DOM_JS.to_string(),
                settle_ms: 1800,
            })
            .await
        {
            Ok(c) => c,
            Err(e) => {
                health.issues.push(HealthIssue {
                    code: "post_fetch_failed".into(),
                    severity: HealthSeverity::Warning,
                    message: format!("Failed to capture post {}: {e}", item.platform_post_id),
                    hint: "Will retry on the next sync pass.".into(),
                });
                continue;
            }
        };
        used_dedicated_tab |= capture.used_dedicated_tab;

        let metrics = parse_post_metrics(&item.platform_post_id, &capture.network_responses, &capture.dom_result);
        if metrics.source == "empty" {
            health.issues.push(HealthIssue {
                code: "post_metrics_empty".into(),
                severity: HealthSeverity::Warning,
                message: format!("No metrics found for post {}.", item.platform_post_id),
                hint: "Post may have been deleted or X selectors drifted.".into(),
            });
            continue;
        }

        let age = hours_since(&item.published_at, now_unix);
        let conn = db.lock().unwrap();
        store_post_snapshot(
            &conn,
            profile_id,
            &metrics,
            &item.draft_kind,
            Some(&item.draft_id),
            Some((age * 10.0).round() / 10.0),
            &mut health.issues,
        )?;
        health.posts_synced += 1;
        synced_post_ids.insert(item.platform_post_id.clone());
    }

    // 3. Profile timeline pass for non-Premium fallback (ANL-06), historical backfill (ANL-16), and external posts (ANL-17)
    let run_profile_pass = !stopped_early && (non_premium_mode || opts.backfill_days > 0);
    if run_profile_pass {
        if let Some(handle) = detected_handle {
            let cutoff_secs = now_unix - (opts.backfill_days.max(30) as i64) * 86_400;
            let tabs = [
                format!("https://x.com/{handle}"),
                format!("https://x.com/{handle}/with_replies"),
            ];
            let mut followers_recorded = health.account_synced;

            for tab_url in tabs {
                if jobs.stop_requested() {
                    stopped_early = true;
                    break;
                }
                let Ok(cap) = fetcher
                    .fetch_page(AnalyticsPageRequest {
                        url: tab_url,
                        network_url_filters: PROFILE_GQL_FILTERS.iter().map(|s| s.to_string()).collect(),
                        dom_script: PROFILE_TIMELINE_DOM_JS.to_string(),
                        settle_ms: 2000,
                    })
                    .await
                else {
                    continue;
                };
                used_dedicated_tab |= cap.used_dedicated_tab;
                let timeline = parse_profile_timeline(&cap.dom_result);

                let conn = db.lock().unwrap();
                if !followers_recorded && timeline.followers_total.is_some() {
                    store_account_snapshot(
                        &conn,
                        profile_id,
                        None,
                        None,
                        None,
                        timeline.followers_total,
                        &serde_json::json!({ "source": "non_premium_profile_header" }),
                    )?;
                    health.account_synced = true;
                    followers_recorded = true;
                }

                for post in timeline.posts {
                    if synced_post_ids.contains(&post.platform_post_id) {
                        continue;
                    }
                    if let Some(posted_at) = &post.posted_at {
                        if let Some(ts) = super::schedule::parse_rfc3339_unix(posted_at) {
                            if ts < cutoff_secs {
                                continue;
                            }
                        }
                    }

                    let (kind, draft_id) =
                        match_draft_for_post(&conn, profile_id, &post.platform_post_id, post.text.as_deref())?;
                    let age = post.posted_at.as_deref().map(|p| (hours_since(p, now_unix) * 10.0).round() / 10.0);
                    store_post_snapshot(&conn, profile_id, &post, &kind, draft_id.as_deref(), age, &mut health.issues)?;
                    synced_post_ids.insert(post.platform_post_id.clone());
                    health.posts_synced += 1;
                    if kind == "external" {
                        health.external_posts_matched += 1;
                    }
                    if opts.backfill_days > 0 {
                        health.backfill_posts_added += 1;
                    }
                }
            }
        } else if opts.backfill_days > 0 {
            health.issues.push(HealthIssue {
                code: "missing_handle_for_backfill".into(),
                severity: HealthSeverity::Warning,
                message: "Could not detect your X handle for 30-day historical backfill.".into(),
                hint: "Ensure you are logged into X in the browser window or pass your handle.".into(),
            });
        }
    }

    health.recompute_status();
    {
        let conn = db.lock().unwrap();
        let _ = crate::formats::winrate::recompute_personal_win_rates(&conn, profile_id);
        save_last_health(&conn, &health)?;
    }

    Ok(SyncReport {
        health,
        used_dedicated_tab,
        stopped_early,
        duration_ms: started.elapsed().as_millis() as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::CapturedNetworkResponse;
    use crate::storage::{open, repo};
    use crate::util::format_unix;
    use serde_json::json;
    use std::collections::HashMap;
    use tempfile::tempdir;

    struct ScriptedFetcher {
        pages: HashMap<String, AnalyticsPageCapture>,
    }

    #[async_trait::async_trait]
    impl AnalyticsPageFetcher for ScriptedFetcher {
        async fn fetch_page(&self, req: AnalyticsPageRequest) -> Result<AnalyticsPageCapture, BrowserError> {
            self.pages
                .get(&req.url)
                .cloned()
                .ok_or_else(|| BrowserError::Other(format!("unexpected url {}", req.url)))
        }
    }

    fn setup_db_with_draft() -> (tempfile::TempDir, Mutex<Connection>, String) {
        let dir = tempdir().unwrap();
        let (conn, _) = open(&dir.path().join("a.db"), &dir.path().join("b")).unwrap();
        let pid = repo::ensure_default_profile(&conn, "/tmp/p").unwrap();
        let published_at = format_unix(now_unix_secs() - 24 * 3600);
        conn.execute("UPDATE profiles SET handle = 'pavel' WHERE id = ?1", [&pid]).unwrap();
        conn.execute(
            "INSERT INTO observed_posts (id, profile_id, platform_post_id, author_handle, author_followers, text, feed_source, posted_at, first_seen_at, metrics_at_capture_json)
             VALUES ('op1', ?1, 'src1', 'naval', 200000, 'Build leverage', 'for_you', ?2, ?2, '{}')",
            params![pid, published_at],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO reply_drafts (id, profile_id, observed_post_id, initial_text, edited_text, edit_distance, model_used, style_guide_version, draft_kind, actions_json, status, published_at, published_post_id, published_url, created_at)
             VALUES ('rd1', ?1, 'op1', 'hi', 'sharp reply', 5, 'claude-sonnet', 1, 'reply', '[\"like\",\"reply\"]', 'published', ?2, '9001', 'https://x.com/pavel/status/9001', ?2)",
            params![pid, published_at],
        )
        .unwrap();
        (dir, Mutex::new(conn), pid)
    }

    #[tokio::test]
    async fn syncs_account_graphql_tracked_posts_and_backfills_external_posts() {
        let (_d, db, pid) = setup_db_with_draft();
        let recent = format_unix(now_unix_secs() - 3 * 86_400);
        let mut pages = HashMap::new();
        pages.insert(
            ACCOUNT_ANALYTICS_URL.into(),
            AnalyticsPageCapture {
                final_url: ACCOUNT_ANALYTICS_URL.into(),
                used_dedicated_tab: true,
                network_responses: vec![CapturedNetworkResponse {
                    url: "https://x.com/i/api/graphql/hash1/AccountOverviewQuery".into(),
                    status: 200,
                    body: json!({"data": {"impressions": 84200, "engagement_rate": 4.7, "profile_visits": 920, "followers_total": 4510}}).to_string(),
                }],
                dom_result: json!({ "handle": "pavel", "loggedOut": false, "isPremiumUpsell": false }),
            },
        );
        pages.insert(
            "https://x.com/pavel/status/9001/analytics".into(),
            AnalyticsPageCapture {
                final_url: "https://x.com/pavel/status/9001/analytics".into(),
                used_dedicated_tab: true,
                network_responses: vec![CapturedNetworkResponse {
                    url: "https://x.com/i/api/graphql/hash2/TweetActivityQuery".into(),
                    status: 200,
                    body: json!({"data": {"Impressions": 5400, "Likes": 140, "Replies": 12, "Reposts": 8, "Bookmarks": 22}}).to_string(),
                }],
                dom_result: json!({}),
            },
        );
        let too_old = format_unix(now_unix_secs() - 45 * 86_400);
        pages.insert(
            "https://x.com/pavel".into(),
            AnalyticsPageCapture {
                final_url: "https://x.com/pavel".into(),
                used_dedicated_tab: true,
                network_responses: vec![],
                dom_result: json!({
                    "handle": "pavel",
                    "followersText": "4,510 Followers",
                    "posts": [
                        { "id": "9001", "text": "sharp reply", "postedAt": recent, "views": "5.4K Views", "likes": "140 Likes" },
                        { "id": "9999", "text": "Posted manually from phone", "postedAt": recent, "views": "1.1K Views", "likes": "25 Likes", "replies": "3 Replies" },
                        { "id": "7777", "text": "Older than 30d window", "postedAt": too_old, "views": "300 Views", "likes": "5 Likes" }
                    ]
                }),
            },
        );
        pages.insert(
            "https://x.com/pavel/with_replies".into(),
            AnalyticsPageCapture {
                final_url: "https://x.com/pavel/with_replies".into(),
                used_dedicated_tab: true,
                network_responses: vec![],
                dom_result: json!({ "handle": "pavel", "posts": [] }),
            },
        );

        let fetcher = ScriptedFetcher { pages };
        let jobs = JobRegistry::new();
        let report = sync_stats(
            &db,
            &pid,
            &fetcher,
            &jobs,
            SyncOptions { backfill_days: 30, ..Default::default() },
        )
        .await
        .unwrap();

        assert!(report.used_dedicated_tab);
        assert_eq!(report.health.status, HealthSeverity::Ok);
        assert!(report.health.account_synced);
        assert_eq!(report.health.posts_synced, 2);
        assert_eq!(report.health.external_posts_matched, 1);

        let conn = db.lock().unwrap();
        let ext_kind: String = conn
            .query_row("SELECT draft_kind FROM analytics_snapshots WHERE platform_post_id = '9999'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(ext_kind, "external");
        let old_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM analytics_snapshots WHERE platform_post_id = '7777'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(old_count, 0, "posts older than the 30-day backfill window must be skipped");
        assert!(jobs.active().is_empty(), "job guard must be released after sync");
    }

    #[tokio::test]
    async fn handles_non_premium_fallback_gracefully() {
        let (_d, db, pid) = setup_db_with_draft();
        let recent = format_unix(now_unix_secs() - 3600);
        let mut pages = HashMap::new();
        pages.insert(
            ACCOUNT_ANALYTICS_URL.into(),
            AnalyticsPageCapture {
                final_url: ACCOUNT_ANALYTICS_URL.into(),
                used_dedicated_tab: true,
                network_responses: vec![],
                dom_result: json!({ "handle": "pavel", "loggedOut": false, "isPremiumUpsell": true, "metricCards": {} }),
            },
        );
        pages.insert(
            "https://x.com/pavel/status/9001".into(),
            AnalyticsPageCapture {
                final_url: "https://x.com/pavel/status/9001".into(),
                used_dedicated_tab: true,
                network_responses: vec![],
                dom_result: json!({ "impressions": "920 Views", "likes": "22 Likes", "replies": "4 Replies" }),
            },
        );
        pages.insert(
            "https://x.com/pavel".into(),
            AnalyticsPageCapture {
                final_url: "https://x.com/pavel".into(),
                used_dedicated_tab: true,
                network_responses: vec![],
                dom_result: json!({ "handle": "pavel", "followersText": "1.5K Followers", "posts": [{ "id": "8888", "text": "Hi", "postedAt": recent, "views": "400 Views", "likes": "10 Likes" }] }),
            },
        );
        pages.insert(
            "https://x.com/pavel/with_replies".into(),
            AnalyticsPageCapture {
                final_url: "https://x.com/pavel/with_replies".into(),
                used_dedicated_tab: true,
                network_responses: vec![],
                dom_result: json!({ "handle": "pavel", "posts": [] }),
            },
        );

        let report = sync_stats(&db, &pid, &ScriptedFetcher { pages }, &JobRegistry::new(), SyncOptions::default())
            .await
            .unwrap();
        assert_eq!(report.health.tier_mode, "non_premium_fallback");
        assert!(report.health.account_synced);
        assert_eq!(report.health.posts_synced, 2);
        assert!(report.health.issues.iter().any(|i| i.code == "non_premium_fallback"));
    }
}
