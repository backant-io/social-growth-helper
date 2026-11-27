//! The scout loop: scroll the feed, read cards, filter locally, score in batches, fill the queue.

use super::filter::{classify, FilterConfig, FilterCounts, DEFAULT_MAX_AGE_SECS};
use super::model::{parse_metrics_label, parse_rfc3339_unix, velocity, FeedRead, Metrics, ScrapedPost};
use super::outlier::{evaluate_outlier, OutlierSignals};
use super::repo::{self, SessionParams};
use super::score::{apply_outlier_and_preferences, build_scoring_prompt, heuristic_score, parse_scores, rubric_with_preferences, Candidate, Score, BATCH_SIZE};
use crate::drafts::edit::DEFAULT_MAX_CHARS;
use crate::drafts::generate::{self as draft_gen, GenRequest, Instruction};
use crate::drafts::repo as draft_repo;
use crate::browser::health::{report_halt, BrowserState};
use crate::browser::service::{BrowserService, X_FOLLOWING_URL, X_HOME_URL};
use crate::completion::{CompletionProvider, CompletionRequest, TaskSlot};
use crate::control::{EventSink, KillSwitch, StopSignal};
use crate::formats::detect::{PostMetrics, ViralThresholds};
use crate::formats::pipeline::{self, PostInput, PostSource};
use crate::selectors;
use crate::storage::repo::get_preferences;
use rusqlite::Connection;
use serde::Serialize;
use serde_json::json;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use crate::phase3::get_watchlist;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// A feed is only "exhausted" after this many scrolls in a row that revealed nothing new. X loads more posts
/// lazily and sometimes stalls, so the loop waits, retries and reloads before it gives up.
const EXHAUSTED_AFTER_EMPTY_SCROLLS: u32 = 14;
/// After this many empty scrolls the feed page is reloaded once (the timeline is rebuilt from the top; posts
/// already seen are skipped).
const RELOAD_AFTER_EMPTY_SCROLLS: u32 = 7;
/// The scout holds still while the user touched the page less than this long ago (SCT-14).
pub const USER_IDLE_MS: u64 = 5_000;
const MAX_THUMBS_PER_SESSION: usize = 12;

/// Reopens the managed X window when it disappears mid-session, so a crashed or closed Chrome does not end Scout.
/// It refuses (returns an error) when the window was closed on purpose.
#[async_trait::async_trait]
pub trait BrowserRelauncher: Send + Sync {
    async fn relaunch(&self) -> Result<(), String>;
}

/// Consecutive browser errors Scout rides out (with growing waits, reopening the window if needed) before it
/// gives up. Any successful read-and-scroll resets the count.
const MAX_SOFT_ERRORS: u32 = 8;

pub struct ScoutEnv<'a> {
    pub db: &'a Mutex<Connection>,
    pub browser: &'a BrowserService,
    pub completion: Arc<dyn CompletionProvider>,
    pub kill: &'a KillSwitch,
    pub sink: &'a dyn EventSink,
    pub profile_id: &'a str,
    pub stop: &'a StopSignal,
    /// Set by the user (tray, UI) to hold the session without ending it.
    pub pause: &'a AtomicBool,
    /// Where post thumbnails are kept for the retention window; `None` keeps them in memory only.
    pub thumbs_dir: Option<std::path::PathBuf>,
    /// How often to re-check for user activity while paused. Tests use a few milliseconds.
    pub idle_poll_ms: u64,
    /// Random pause between scrolls in milliseconds. Tests use `(0, 0)`.
    pub pause_ms: (u64, u64),
    /// Reopens the X window if it vanishes; `None` only waits for it to come back.
    pub relaunch: Option<&'a dyn BrowserRelauncher>,
}

impl ScoutEnv<'_> {
    fn should_stop(&self) -> bool {
        self.kill.is_engaged() || self.stop.is_stopped()
    }

    async fn cancelled(&self) {
        tokio::select! {
            biased;
            _ = self.stop.cancelled() => {}
            _ = self.kill.engaged() => {}
        }
    }

    /// Runs `fut` unless stop or the kill switch fires first. Returns `None` if cancelled.
    async fn cancellable<F: std::future::Future>(&self, fut: F) -> Option<F::Output> {
        if self.should_stop() {
            return None;
        }
        tokio::select! {
            biased;
            _ = self.cancelled() => None,
            out = fut => Some(out),
        }
    }

    /// Sleeps for `total`, waking immediately if stop or the kill switch fires.
    /// Returns `true` if the full duration elapsed, `false` if cancelled.
    async fn sleep(&self, total: Duration) -> bool {
        if total.is_zero() {
            return !self.should_stop();
        }
        self.cancellable(tokio::time::sleep(total)).await.is_some()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionSummary {
    pub reason: String,
    pub scanned: i64,
    pub scored: i64,
    pub queued: i64,
    pub filtered: FilterCounts,
    pub model: String,
    pub heuristic_scored: i64,
    /// Over-performing posts newly stored as viral-format candidates during this session (OGC-06).
    #[serde(default)]
    pub format_candidates: i64,
}

/// Feeds every organic post the scout reads into continuous viral detection (OGC-06). It runs before the scout's
/// own filters, because a stale or already-scored post can still be a good format source. Never fails the session.
fn consider_for_formats(
    env: &ScoutEnv<'_>,
    thresholds: &ViralThresholds,
    post: &ScrapedPost,
    metrics: &Metrics,
    own_handle: Option<&str>,
    author_followers: Option<i64>,
) -> bool {
    let (Some(id), Some(handle)) = (post.id.as_deref(), post.handle.as_deref()) else { return false };
    if post.promoted || post.text.trim().is_empty() || own_handle.is_some_and(|me| me.eq_ignore_ascii_case(handle)) {
        return false;
    }
    let input = PostInput {
        platform_post_id: id.to_string(),
        author_handle: handle.to_string(),
        author_followers: author_followers.and_then(|f| u64::try_from(f).ok()),
        text: post.text.clone(),
        metrics: PostMetrics { likes: metrics.likes, reposts: metrics.reposts, replies: metrics.replies, bookmarks: metrics.bookmarks, views: (metrics.views > 0).then_some(metrics.views) },
        posted_at: post.time.clone(),
        author_name: post.author.clone(),
        avatar_url: post.avatar_url.clone(),
        media: post.resolved_media(),
    };
    let db = env.db.lock().unwrap();
    pipeline::consider_post(&db, env.profile_id, &input, PostSource::Scout, thresholds, false).map(|d| d.candidate.is_some()).unwrap_or(false)
}

fn now_unix() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn jitter(range: (u64, u64)) -> u64 {
    if range.1 <= range.0 {
        return range.0;
    }
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.subsec_nanos() as u64).unwrap_or(0);
    range.0 + nanos % (range.1 - range.0 + 1)
}

async fn score_batch(env: &ScoutEnv<'_>, rubric: &str, batch: &[Candidate], prefs: &crate::storage::repo::Preferences, heuristic_count: &mut i64, model_name: &mut String) -> Option<Vec<(Candidate, Score, String)>> {
    if env.should_stop() {
        return None;
    }
    let ids: Vec<String> = batch.iter().filter_map(|c| c.post.id.clone()).collect();
    let mut model = String::from("heuristic");
    let mut by_id: Vec<Score> = vec![];
    if env.completion.is_configured(TaskSlot::PostScoring) {
        let (system, user) = build_scoring_prompt(rubric, batch);
        let req = CompletionRequest::new(TaskSlot::PostScoring, system, user, true).with_slots(super::score::scoring_slots(rubric, batch)).with_images(super::score::batch_images(batch));
        let res = env.cancellable(env.completion.complete(req)).await?;
        if let Ok(c) = res {
            by_id = parse_scores(&c.text, &ids);
            model = c.model;
        }
    }
    if env.should_stop() {
        return None;
    }
    let mut out = vec![];
    for c in batch {
        let id = c.post.id.clone().unwrap_or_default();
        let (s, m) = match by_id.iter().find(|s| s.id == id) {
            Some(s) => (s.clone(), model.clone()),
            None => {
                *heuristic_count += 1;
                (heuristic_score(c, prefs), "heuristic".to_string())
            }
        };
        if m != "heuristic" {
            *model_name = m.clone();
        }
        out.push((c.clone(), apply_outlier_and_preferences(s, c, prefs), m));
    }
    Some(out)
}

async fn user_idle_ms(env: &ScoutEnv<'_>) -> u64 {
    let d = env.browser.lock().await;
    if !d.is_running() {
        return u64::MAX;
    }
    d.eval_json(&selectors::user_idle_js()).await.ok().and_then(|v| v.as_u64()).unwrap_or(u64::MAX)
}

/// Holds while the user is working in the X window or has paused the scout, then resumes (SCT-14).
/// Time spent here does not count against the session duration.
async fn wait_until_clear(env: &ScoutEnv<'_>, session_id: &str, paused_total: &mut Duration) {
    let began = Instant::now();
    let mut announced = false;
    loop {
        if env.should_stop() {
            break;
        }
        let manual = env.pause.load(Ordering::SeqCst);
        if !manual {
            match env.cancellable(user_idle_ms(env)).await {
                None => break,
                Some(ms) if ms >= USER_IDLE_MS => break,
                Some(_) => {}
            }
        }
        if !announced {
            announced = true;
            env.sink.emit("scout:paused", json!({ "session_id": session_id, "reason": if manual { "manual" } else { "user_activity" } }));
        }
        if !env.sleep(Duration::from_millis(env.idle_poll_ms)).await {
            break;
        }
    }
    if announced {
        *paused_total += began.elapsed();
        if !env.should_stop() {
            env.sink.emit("scout:resumed", json!({ "session_id": session_id }));
        }
    }
}

/// Feed pages to visit, in order: the For You timeline, the Following timeline and/or each watchlist profile
/// (SCT-03, SCT-16). `both` stays For You + watchlist; `all` is every source.
pub fn targets(feed_source: &str, watchlist: &[String]) -> Vec<String> {
    let mut t = vec![];
    if matches!(feed_source, "for_you" | "both" | "all") {
        t.push(X_HOME_URL.to_string());
    }
    if matches!(feed_source, "following" | "all") {
        t.push(X_FOLLOWING_URL.to_string());
    }
    if matches!(feed_source, "watchlist" | "both" | "all") {
        t.extend(watchlist.iter().map(|h| format!("https://x.com/{h}")));
    }
    t
}

/// Whether the session reads the Following timeline, so the home page's tab must be set explicitly.
fn uses_following(feed_source: &str) -> bool {
    matches!(feed_source, "following" | "all")
}

/// Opens a feed target. The home page remembers the last selected tab, so For You and Following are chosen
/// by clicking the tab (best effort; a missing tab leaves whatever X shows).
async fn visit(d: &dyn crate::browser::BrowserDriver, url: &str, following_in_play: bool) -> Result<(), crate::browser::BrowserError> {
    d.navigate(url).await?;
    let tab = if url == X_FOLLOWING_URL {
        "following"
    } else if url == X_HOME_URL && following_in_play {
        "for you"
    } else {
        return Ok(());
    };
    tokio::time::sleep(Duration::from_millis(800)).await;
    if let Ok(v) = d.eval_json(&selectors::rect_js(None, &selectors::sel::FEED_TAB, Some(tab))).await {
        if let (Some(x), Some(y)) = (v["x"].as_f64(), v["y"].as_f64()) {
            let _ = d.click_point(x, y).await;
        }
    }
    Ok(())
}

/// Fetches a small thumbnail for up to a few image posts per session and keeps a copy on disk (SCT-19).
async fn attach_thumbnails(env: &ScoutEnv<'_>, candidates: &mut [Candidate], budget: &mut usize) {
    if !env.completion.is_configured(TaskSlot::PostScoring) {
        return;
    }
    for c in candidates.iter_mut() {
        if *budget == 0 || env.should_stop() {
            return;
        }
        let Some(url) = c.post.image_urls.first().filter(|u| selectors::is_image_url(u)).cloned() else { continue };
        *budget -= 1;
        let Some(b64) = env
            .cancellable(async {
                let d = env.browser.lock().await;
                d.eval_json(&selectors::thumbnail_js(&url)).await.ok().and_then(|v| v.as_str().map(str::to_string)).filter(|s| !s.is_empty())
            })
            .await
        else {
            return;
        };
        let Some(b64) = b64 else { continue };
        if let (Some(dir), Some(id)) = (env.thumbs_dir.as_ref(), c.post.id.as_deref()) {
            use base64::Engine;
            if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(&b64) {
                let _ = std::fs::create_dir_all(dir);
                let _ = std::fs::write(dir.join(format!("{id}.jpg")), bytes);
            }
        }
        c.image = Some(b64);
    }
}

fn record_activity(env: &ScoutEnv<'_>, kind: &str, title: &str, detail: Option<&str>, ok: Option<bool>) {
    let db = env.db.lock().unwrap();
    crate::activity::record(&db, env.profile_id, kind, title, detail, ok, None);
}

/// CDP `Input.dispatchMouseEvent` timeouts happen when the managed Chrome window loses focus
/// or is occluded (macOS pauses compositor ACKs). Never end Scout over these: keep backing off
/// and retrying until focus returns, the user stops Scout, or the window is closed on purpose.
fn is_transient_input_timeout(err: &str) -> bool {
    let lower = err.to_ascii_lowercase();
    lower.contains("input.dispatchmouseevent") && lower.contains("timed out")
}

/// Handles a transient browser error: logs it, waits with growing delays, reopens the window if it is gone, reloads
/// the feed page, and tells the caller to carry on. Returns `false` only when the error is not recoverable
/// (retries used up, or the window cannot be reopened). The session is never finished by a single error.
async fn soft_fail(env: &ScoutEnv<'_>, session_id: &str, strikes: &mut u32, what: &str, err: &str, target: &str, following_in_play: bool) -> bool {
    if env.should_stop() {
        return true;
    }
    let transient_timeout = is_transient_input_timeout(err);
    *strikes = strikes.saturating_add(1);
    let attempt = *strikes;
    if transient_timeout {
        log::warn!("scout {session_id}: transient focus/input timeout while trying to {what} ({err}); backing off (attempt {attempt})");
    } else {
        log::warn!("scout {session_id}: could not {what} ({err}); attempt {attempt} of {MAX_SOFT_ERRORS}");
    }
    if attempt == 1 {
        record_activity(env, "scout", "Scout hit a browser hiccup and is retrying", Some(&format!("Could not {what}: {err}")), None);
    }
    env.sink.emit("scout:warning", json!({ "session_id": session_id, "message": format!("Could not {what}; retrying"), "attempt": attempt, "max": MAX_SOFT_ERRORS }));

    let give_up = |why: &str| {
        env.sink.emit("scout:error", json!({ "message": format!("Scout stopped: {why}") }));
        record_activity(env, "error", "Scout could not recover from a browser error", Some(why), Some(false));
    };
    if !transient_timeout && attempt > MAX_SOFT_ERRORS {
        give_up(&format!("could not {what} after {MAX_SOFT_ERRORS} retries ({err})"));
        if !env.browser.is_running().await {
            report_halt(env.sink, BrowserState::Crashed, "scout");
        }
        return false;
    }

    let base_ms: u64 = if env.pause_ms.1 == 0 { 1 } else { 1500 };
    let delay = (base_ms << (attempt - 1).min(5)).min(20_000);
    if !env.sleep(Duration::from_millis(delay)).await {
        return true;
    }
    let Some(running) = env.cancellable(env.browser.is_running()).await else {
        return true;
    };
    let mut relaunched = false;
    if !running {
        let Some(relaunch) = env.relaunch else { return true };
        let Some(res) = env.cancellable(relaunch.relaunch()).await else {
            return true;
        };
        if let Err(why) = res {
            give_up(&why);
            report_halt(env.sink, BrowserState::Crashed, "scout");
            return false;
        }
        relaunched = true;
    }
    if !transient_timeout || relaunched {
        let _ = env
            .cancellable(async {
                let d = env.browser.lock().await;
                if d.is_running() {
                    let _ = visit(&*d, target, following_in_play).await;
                }
            })
            .await;
    }
    true
}

pub async fn run_session(env: &ScoutEnv<'_>, session_id: &str, params: &SessionParams) -> SessionSummary {
    let started = Instant::now();
    let max_duration = Duration::from_secs(params.max_duration_sec.max(0) as u64);
    let mut seen_in_session: HashSet<String> = HashSet::new();
    let mut filtered = FilterCounts::default();
    let (mut scored, mut queued, mut heuristic_scored) = (0i64, 0i64, 0i64);
    let mut model_name = String::from("heuristic");
    let mut empty_scrolls = 0u32;
    let mut soft_errors = 0u32;
    let mut format_candidates = 0i64;
    let viral_thresholds = {
        let db = env.db.lock().unwrap();
        crate::formats::repo::get_thresholds(&db).unwrap_or_default()
    };

    let (prefs, rubric, rubric_version) = {
        let db = env.db.lock().unwrap();
        let prefs = get_preferences(&db, env.profile_id).ok().flatten().unwrap_or_default();
        let (v, text) = repo::ensure_default_rubric(&db, env.profile_id).unwrap_or_else(|_| (0, super::score::DEFAULT_RUBRIC.to_string()));
        let rubric = rubric_with_preferences(&text, &prefs);
        (prefs, rubric, v)
    };

    let watchlist = get_watchlist(&env.db.lock().unwrap());
    let targets = targets(&params.feed_source, &watchlist);
    let following_in_play = uses_following(&params.feed_source);
    let per_target = ((params.max_posts as usize).div_ceil(targets.len().max(1))).max(5);
    let queued_quota = params.stop_after_queued.filter(|_| targets.len() > 1).map(|n| ((n + targets.len() as i64 - 1) / targets.len() as i64).max(1));
    let (mut target_idx, mut target_new) = (0usize, 0usize);
    let mut target_queued_start = 0i64;
    let mut reloaded_this_target = false;
    let mut paused_total = Duration::ZERO;
    let mut thumb_budget = MAX_THUMBS_PER_SESSION;
    let initial_url = targets.first().map(String::as_str).unwrap_or(X_HOME_URL);
    if env
        .cancellable(async {
            let d = env.browser.lock().await;
            if d.is_running() {
                let _ = visit(&*d, initial_url, following_in_play).await;
            }
        })
        .await
        .is_some()
        && env.pause_ms.1 > 0
    {
        env.sleep(Duration::from_millis(2500)).await;
    }

    let reason: String = 'main: loop {
        wait_until_clear(env, session_id, &mut paused_total).await;
        if env.kill.is_engaged() {
            break "killed".into();
        }
        if env.stop.is_stopped() {
            break "stopped".into();
        }
        if started.elapsed().saturating_sub(paused_total) >= max_duration {
            break "duration".into();
        }
        if seen_in_session.len() as i64 >= params.max_posts {
            break "post_cap".into();
        }
        if params.stop_after_queued.is_some_and(|n| queued >= n) {
            break "queue_goal".into();
        }

        let Some(read) = env
            .cancellable(async {
                let d = env.browser.lock().await;
                if !d.is_running() {
                    Err("the browser window is not open".into())
                } else {
                    match d.eval_json(&selectors::extract_feed_js()).await {
                        Ok(v) => serde_json::from_value::<FeedRead>(v).map_err(|e| format!("unexpected page data: {e}")),
                        Err(e) => Err(e.to_string()),
                    }
                }
            })
            .await
        else {
            continue 'main;
        };
        let read = match read {
            Ok(r) => r,
            Err(e) => {
                if soft_fail(env, session_id, &mut soft_errors, "read the page", &e, &targets[target_idx], following_in_play).await {
                    continue;
                }
                break "browser_error".into();
            }
        };
        match read.state.as_str() {
            other => {
                if let Some(halt) = BrowserState::from_page_state(other) {
                    record_activity(env, "challenge", &format!("Scout stopped: {}", halt.message()), Some("Fix it in the X window, then press Resume."), Some(false));
                    report_halt(env.sink, halt, "scout");
                    break halt.as_str().into();
                }
            }
        }

        let cfg = FilterConfig {
            now_unix: now_unix(),
            max_age_secs: DEFAULT_MAX_AGE_SECS,
            own_handle: read.me.clone(),
            avoid_accounts: prefs.avoid_accounts.clone(),
            languages: prefs.languages.clone(),
        };
        let mut new_ids = 0;
        let mut candidates: Vec<Candidate> = vec![];
        for post in read.posts {
            let Some(id) = post.id.clone() else {
                filtered.invalid += 1;
                continue;
            };
            if seen_in_session.contains(&id) {
                continue;
            }
            if seen_in_session.len() as i64 >= params.max_posts {
                break;
            }
            seen_in_session.insert(id.clone());
            new_ids += 1;
            target_new += 1;
            let metrics = post.metrics_label.as_deref().map(parse_metrics_label).unwrap_or_default();
            let author_followers = post.handle.as_deref().and_then(|h| {
                let db = env.db.lock().unwrap();
                repo::lookup_author_followers(&db, env.profile_id, h)
            });
            if consider_for_formats(env, &viral_thresholds, &post, &metrics, read.me.as_deref(), author_followers) {
                format_candidates += 1;
            }
            let drop = {
                let db = env.db.lock().unwrap();
                classify(&post, &cfg, &|pid| repo::has_evaluation(&db, env.profile_id, pid) || repo::already_engaged(&db, env.profile_id, pid))
            };
            if let Some(d) = drop {
                filtered.record(d);
                continue;
            }
            let age_secs = post.time.as_deref().and_then(parse_rfc3339_unix).map(|t| (cfg.now_unix - t).max(0)).unwrap_or(3600);
            let v = velocity(&metrics, age_secs);
            let outlier = evaluate_outlier(&OutlierSignals {
                metrics: &metrics,
                age_minutes: age_secs / 60,
                author_followers,
                author_verified: post.author_verified,
                is_repost: post.is_repost,
                has_media: post.has_media || !post.resolved_media().is_empty(),
                text: &post.text,
                has_context: post.context_text().is_some(),
            });
            candidates.push(Candidate { post, metrics, age_minutes: age_secs / 60, velocity: v, author_followers, outlier, image: None });
        }
        attach_thumbnails(env, &mut candidates, &mut thumb_budget).await;
        if env.should_stop() {
            continue 'main;
        }

        for batch in candidates.chunks(BATCH_SIZE) {
            if env.should_stop() {
                continue 'main;
            }
            let Some(results) = score_batch(env, &rubric, batch, &prefs, &mut heuristic_scored, &mut model_name).await else {
                continue 'main;
            };
            if env.should_stop() {
                continue 'main;
            }
            let mut to_draft: Vec<(String, String, String)> = vec![];
            {
                let db = env.db.lock().unwrap();
                for (c, s, model) in results {
                    let Ok(obs) = repo::upsert_observed(&db, env.profile_id, &c.post, &c.metrics, c.velocity, &params.feed_source, c.author_followers) else { continue };
                    if let Ok(eval_id) = repo::insert_evaluation_with_outlier(
                        &db,
                        &obs,
                        session_id,
                        rubric_version,
                        &model,
                        s.score,
                        &s.angle,
                        &s.reasoning,
                        Some(c.outlier.outlier_score),
                        &c.outlier.breakout_flags,
                    ) {
                        scored += 1;
                        if s.score >= params.threshold {
                            queued += 1;
                            to_draft.push((eval_id, s.angle, s.reasoning));
                        }
                    }
                }
            }
            if env.completion.is_configured(TaskSlot::ReplyDrafting) && !to_draft.is_empty() {
                for (eval_id, angle, reasoning) in to_draft {
                    if env.should_stop() {
                        continue 'main;
                    }
                    let prep = {
                        let db = env.db.lock().unwrap();
                        let Ok(draft_id) = draft_repo::create_for_evaluation(&db, env.profile_id, &eval_id) else { continue };
                        let Ok(view) = draft_repo::get_view(&db, &draft_id) else { continue };
                        if !view.edited_text.trim().is_empty() || !view.variants.is_empty() {
                            continue;
                        }
                        let Ok(style) = draft_repo::load_style_for_generation(&db, env.profile_id, &view.post.text) else { continue };
                        let context = draft_repo::post_context(&db, &view.post.observed_id).ok().flatten();
                        Some((draft_id, view, style, context))
                    };
                    let Some((draft_id, view, style, context)) = prep else { continue };
                    let req = GenRequest {
                        quote: view.kind == "quote",
                        handle: &view.post.handle,
                        post_text: &view.post.text,
                        max_chars: DEFAULT_MAX_CHARS,
                        style: &style,
                        instruction: Instruction::Fresh,
                        current: None,
                        angle: (!angle.trim().is_empty()).then_some(angle.as_str()),
                        model_hint: None,
                        context: context.as_deref(),
                        why_working: (!reasoning.trim().is_empty()).then_some(reasoning.as_str()),
                    };
                    let Some(res) = env.cancellable(draft_gen::generate(&*env.completion, &req)).await else {
                        continue 'main;
                    };
                    if let Ok(gen) = res {
                        let db = env.db.lock().unwrap();
                        let _ = draft_repo::add_variants(&db, &draft_id, &gen.variants, &gen.model);
                    }
                }
            }
        }
        if env.should_stop() {
            continue 'main;
        }

        let scanned = seen_in_session.len() as i64;
        {
            let db = env.db.lock().unwrap();
            let _ = repo::update_progress(&db, session_id, scanned, queued);
        }
        env.sink.emit(
            "scout:progress",
            json!({ "session_id": session_id, "scanned": scanned, "queued": queued, "target": params.stop_after_queued, "scored": scored, "elapsed_sec": started.elapsed().as_secs(), "filtered": filtered }),
        );

        if new_ids == 0 {
            // Only count an empty scroll when the previous iteration actually scrolled.
            if soft_errors == 0 {
                empty_scrolls += 1;
            }
        } else {
            empty_scrolls = 0;
        }
        if empty_scrolls == RELOAD_AFTER_EMPTY_SCROLLS && !reloaded_this_target {
            reloaded_this_target = true;
            let Some(visited) = env
                .cancellable(async {
                    let d = env.browser.lock().await;
                    visit(&*d, &targets[target_idx], following_in_play).await
                })
                .await
            else {
                continue 'main;
            };
            if let Err(e) = visited {
                if soft_fail(env, session_id, &mut soft_errors, "reload the feed", &e.to_string(), &targets[target_idx], following_in_play).await {
                    continue;
                }
                break 'main "browser_error".into();
            }
            if !env.sleep(Duration::from_millis(jitter(env.pause_ms).max(env.pause_ms.0))).await {
                continue 'main;
            }
            continue;
        }
        let quota_met = queued_quota.is_some_and(|q| queued - target_queued_start >= q);
        let target_done = empty_scrolls >= EXHAUSTED_AFTER_EMPTY_SCROLLS || (targets.len() > 1 && (target_new >= per_target || quota_met));
        if target_done {
            if target_idx + 1 >= targets.len() {
                break "feed_exhausted".into();
            }
            target_idx += 1;
            (target_new, empty_scrolls) = (0, 0);
            target_queued_start = queued;
            reloaded_this_target = false;
            let Some(visited) = env
                .cancellable(async {
                    let d = env.browser.lock().await;
                    visit(&*d, &targets[target_idx], following_in_play).await
                })
                .await
            else {
                continue 'main;
            };
            if let Err(e) = visited {
                if soft_fail(env, session_id, &mut soft_errors, "open the next feed", &e.to_string(), &targets[target_idx], following_in_play).await {
                    continue;
                }
                break 'main "browser_error".into();
            }
            if !env.sleep(Duration::from_millis(jitter(env.pause_ms).max(env.pause_ms.0))).await {
                continue 'main;
            }
            continue;
        }

        if env.should_stop() {
            continue 'main;
        }
        let Some(scrolled) = env
            .cancellable(async {
                let d = env.browser.lock().await;
                let stalled = empty_scrolls.min(6) as f64;
                let delta = 700.0 + jitter((0, 400)) as f64 + stalled * 350.0;
                if empty_scrolls > 0 {
                    let _ = d.eval_json(&selectors::retry_feed_js()).await;
                }
                let _ = d.eval_json(&selectors::set_busy_js(true)).await;
                let scrolled = d.wheel_scroll(640.0, 500.0, delta).await;
                let _ = d.eval_json(&selectors::set_busy_js(false)).await;
                scrolled
            })
            .await
        else {
            continue 'main;
        };
        if let Err(e) = scrolled {
            if soft_fail(env, session_id, &mut soft_errors, "scroll the feed", &e.to_string(), &targets[target_idx], following_in_play).await {
                continue;
            }
            break 'main "browser_error".into();
        }
        if soft_errors > 0 {
            log::info!("scout {session_id}: recovered from a browser error and kept scrolling");
            soft_errors = 0;
        }
        if !env.sleep(Duration::from_millis(jitter(env.pause_ms))).await {
            continue;
        }
        if empty_scrolls > 0 && env.pause_ms.1 > 0 && !env.sleep(Duration::from_millis(1200 * u64::from(empty_scrolls.min(4)))).await {
            continue;
        }
    };

    let scanned = seen_in_session.len() as i64;
    let summary = SessionSummary { reason: reason.clone(), scanned, scored, queued, filtered, model: model_name, heuristic_scored, format_candidates };
    {
        let db = env.db.lock().unwrap();
        let _ = repo::update_progress(&db, session_id, scanned, queued);
        let _ = repo::finish_session(&db, session_id, &reason, &serde_json::to_string(&summary).unwrap_or_default());
    }
    record_activity(
        env,
        "scout",
        &format!("Scout finished ({reason})"),
        Some(&format!("{scanned} scanned, {scored} scored, {queued} queued, {format_candidates} viral format candidates")),
        Some(!matches!(reason.as_str(), "browser_error" | "logged_out" | "challenge" | "suspended" | "locked")),
    );
    env.sink.emit("scout:finished", json!({ "session_id": session_id, "summary": summary }));
    summary
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::mock::MockDriver;
    use crate::browser::ChromeLaunchConfig;
    use crate::completion::{ScriptedProvider, UnconfiguredProvider};
    use crate::control::CollectSink;
    use crate::drafts::repo::testutil::db;
    use serde_json::{json, Value};

    fn feed(n: usize, start: usize) -> Value {
        let posts: Vec<Value> = (start..start + n)
            .map(|i| {
                json!({
                    "id": format!("{}", 1000 + i), "handle": format!("user{i}"), "author": "U", "text": format!("post number {i}?"),
                    "lang": "en", "promoted": false, "reply_restricted": false, "has_media": false,
                    "time": crate::util::now_rfc3339(), "metrics_label": "2 replies, 3 reposts, 40 likes, 1 bookmarks, 900 views"
                })
            })
            .collect();
        json!({ "state": "ok", "me": "me", "url": "https://x.com/home", "posts": posts })
    }

    async fn browser(mock: &MockDriver) -> BrowserService {
        let svc = BrowserService::new(Box::new(mock.clone()));
        svc.launch(ChromeLaunchConfig { binary_path: "/x".into(), profile_dir: "/p".into(), start_url: X_HOME_URL.into(), width: 1, height: 1, invisible: false }).await.unwrap();
        svc
    }

    fn params(max_posts: i64, stop_after: Option<i64>) -> SessionParams {
        SessionParams { feed_source: "for_you".into(), max_duration_sec: 60, max_posts, stop_after_queued: stop_after, threshold: 0.0 }
    }

    struct Fixture {
        db: Mutex<Connection>,
        profile: String,
        kill: KillSwitch,
        sink: CollectSink,
        stop: StopSignal,
        pause: AtomicBool,
    }

    fn fixture() -> Fixture {
        let (conn, profile) = db();
        Fixture { db: Mutex::new(conn), profile, kill: KillSwitch::default(), sink: CollectSink::default(), stop: StopSignal::new(), pause: AtomicBool::new(false) }
    }

    fn env<'a>(f: &'a Fixture, b: &'a BrowserService, p: Arc<dyn CompletionProvider>) -> ScoutEnv<'a> {
        ScoutEnv { db: &f.db, browser: b, completion: p, kill: &f.kill, sink: &f.sink, profile_id: &f.profile, stop: &f.stop, pause: &f.pause, thumbs_dir: None, idle_poll_ms: 2, pause_ms: (0, 0), relaunch: None }
    }

    #[tokio::test]
    async fn stops_at_the_post_cap_and_scores_with_heuristic_when_no_model() {
        let mock = MockDriver::new().with_eval_fn(|_| feed(12, 0));
        let b = browser(&mock).await;
        let f = fixture();
        let p = params(8, None);
        let sid = repo::create_session(&f.db.lock().unwrap(), &f.profile, &p).unwrap();
        let s = run_session(&env(&f, &b, Arc::new(UnconfiguredProvider)), &sid, &p).await;
        assert_eq!((s.reason.as_str(), s.scanned, s.scored), ("post_cap", 8, 8));
        assert_eq!(s.heuristic_scored, 8);
        let n: i64 = f.db.lock().unwrap().query_row("SELECT COUNT(*) FROM scout_evaluations WHERE model_used = 'heuristic'", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 8);
        let events = f.sink.0.lock().unwrap();
        assert!(events.iter().any(|(e, _)| e == "scout:progress") && events.last().unwrap().0 == "scout:finished");
    }

    #[tokio::test]
    async fn over_performing_feed_posts_become_format_candidates_during_a_normal_run() {
        let viral = |id: &str, handle: &str, promoted: bool| {
            json!({
                "id": id, "handle": handle, "author": "V", "text": format!("viral post {id}"), "lang": "en", "promoted": promoted,
                "reply_restricted": false, "has_media": false, "time": crate::util::now_rfc3339(),
                "metrics_label": "60 replies, 120 reposts, 900 likes, 80 bookmarks, 90000 views"
            })
        };
        let mock = MockDriver::new().with_eval_fn(move |_| {
            let mut f = feed(4, 0);
            let posts = f["posts"].as_array_mut().unwrap();
            posts.push(viral("5000", "star", false));
            posts.push(viral("5001", "me", false));
            posts.push(viral("5002", "paid", true));
            f
        });
        let b = browser(&mock).await;
        let f = fixture();
        let p = params(7, None);
        let sid = repo::create_session(&f.db.lock().unwrap(), &f.profile, &p).unwrap();
        let s = run_session(&env(&f, &b, Arc::new(UnconfiguredProvider)), &sid, &p).await;
        assert_eq!(s.format_candidates, 1, "ordinary, own and promoted posts are not candidates");
        let db = f.db.lock().unwrap();
        let cands = crate::formats::repo::list_candidates(&db, &f.profile, "pending", 10).unwrap();
        assert_eq!(cands.len(), 1);
        assert_eq!((cands[0].platform_post_id.as_str(), cands[0].source.as_str()), ("5000", "scout"));
    }

    #[tokio::test]
    async fn stops_when_the_queue_goal_is_reached() {
        let mock = MockDriver::new().with_eval_fn(|_| feed(10, 0));
        let b = browser(&mock).await;
        let f = fixture();
        let p = params(100, Some(3));
        let sid = repo::create_session(&f.db.lock().unwrap(), &f.profile, &p).unwrap();
        let s = run_session(&env(&f, &b, Arc::new(UnconfiguredProvider)), &sid, &p).await;
        assert_eq!(s.reason, "queue_goal");
        assert!(s.queued >= 3);
    }

    #[tokio::test]
    async fn repeated_empty_scrolls_end_the_session() {
        let mock = MockDriver::new().with_eval_fn(|_| feed(3, 0));
        let b = browser(&mock).await;
        let f = fixture();
        let p = params(100, None);
        let sid = repo::create_session(&f.db.lock().unwrap(), &f.profile, &p).unwrap();
        let s = run_session(&env(&f, &b, Arc::new(UnconfiguredProvider)), &sid, &p).await;
        assert_eq!((s.reason.as_str(), s.scanned), ("feed_exhausted", 3));
    }

    #[tokio::test]
    async fn keeps_scrolling_through_a_stall_until_the_target_is_found() {
        use std::sync::atomic::AtomicUsize;
        let reads = Arc::new(AtomicUsize::new(0));
        let rc = reads.clone();
        let mock = MockDriver::new().with_eval_fn(move |js| {
            if js.contains("posts.push") {
                let n = rc.fetch_add(1, Ordering::SeqCst);
                return if n < 9 { feed(3, 0) } else { feed(12, 0) };
            }
            feed(3, 0)
        });
        let b = browser(&mock).await;
        let f = fixture();
        let p = params(500, Some(8));
        let sid = repo::create_session(&f.db.lock().unwrap(), &f.profile, &p).unwrap();
        let s = run_session(&env(&f, &b, Arc::new(UnconfiguredProvider)), &sid, &p).await;
        assert_eq!(s.reason, "queue_goal", "a feed that stalls for several scrolls is not exhausted");
        assert!(s.queued >= 8);
    }

    #[tokio::test]
    async fn model_scores_are_used_and_malformed_batches_fall_back() {
        let mock = MockDriver::new().with_eval_fn(|_| feed(2, 0));
        let b = browser(&mock).await;
        let f = fixture();
        let p = params(100, None);
        let sid = repo::create_session(&f.db.lock().unwrap(), &f.profile, &p).unwrap();
        let provider = Arc::new(ScriptedProvider::new(vec![r#"{"scores":[{"id":"1000","score":91,"angle":"ask back","reasoning":"early"}]}"#]));
        let s = run_session(&env(&f, &b, provider.clone()), &sid, &p).await;
        assert_eq!(s.scored, 2);
        assert_eq!(s.heuristic_scored, 1, "the post the model skipped is scored locally");
        let top: (f64, String) = f.db.lock().unwrap().query_row(
            "SELECT potential_score, model_used FROM scout_evaluations e JOIN observed_posts o ON o.id = e.observed_post_id WHERE o.platform_post_id = '1000'", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!((top.0, top.1.as_str()), (91.0, "scripted"));
        assert!(provider.requests.lock().unwrap()[0].user.starts_with(crate::guard::FENCE_OPEN));
    }

    #[tokio::test]
    async fn kill_switch_ends_the_session_and_skips_remaining_work() {
        let mock = MockDriver::new().with_eval_fn(|_| feed(5, 0));
        let b = browser(&mock).await;
        let f = fixture();
        f.kill.engage();
        let p = params(100, None);
        let sid = repo::create_session(&f.db.lock().unwrap(), &f.profile, &p).unwrap();
        let t = Instant::now();
        let s = run_session(&env(&f, &b, Arc::new(UnconfiguredProvider)), &sid, &p).await;
        assert_eq!((s.reason.as_str(), s.scanned), ("killed", 0));
        assert!(t.elapsed() < Duration::from_secs(2));
    }

    #[tokio::test]
    async fn logged_out_page_stops_cleanly() {
        let mock = MockDriver::new().with_eval_fn(|_| json!({ "state": "logged_out", "me": null, "posts": [] }));
        let b = browser(&mock).await;
        let f = fixture();
        let p = params(100, None);
        let sid = repo::create_session(&f.db.lock().unwrap(), &f.profile, &p).unwrap();
        let s = run_session(&env(&f, &b, Arc::new(UnconfiguredProvider)), &sid, &p).await;
        assert_eq!(s.reason, "logged_out");
    }

    #[tokio::test]
    async fn own_and_seen_posts_are_filtered() {
        let mock = MockDriver::new().with_eval_fn(|_| {
            let mut f = feed(3, 0);
            f["posts"][0]["handle"] = json!("me");
            f
        });
        let b = browser(&mock).await;
        let f = fixture();
        let p = params(100, None);
        let sid = repo::create_session(&f.db.lock().unwrap(), &f.profile, &p).unwrap();
        let s = run_session(&env(&f, &b, Arc::new(UnconfiguredProvider)), &sid, &p).await;
        assert_eq!((s.filtered.own, s.scored), (1, 2));
        let sid2 = repo::create_session(&f.db.lock().unwrap(), &f.profile, &p).unwrap();
        let s2 = run_session(&env(&f, &b, Arc::new(UnconfiguredProvider)), &sid2, &p).await;
        assert_eq!(s2.scored, 0, "already evaluated posts are not scored twice");
    }

    #[tokio::test]
    async fn posts_we_already_replied_to_are_not_scored_or_queued() {
        let mock = MockDriver::new().with_eval_fn(|_| feed(3, 0));
        let b = browser(&mock).await;
        let f = fixture();
        crate::publish::repo::record_event(&f.db.lock().unwrap(), &f.profile, "1001", "reply", "publish", true).unwrap();
        let p = params(100, None);
        let sid = repo::create_session(&f.db.lock().unwrap(), &f.profile, &p).unwrap();
        let s = run_session(&env(&f, &b, Arc::new(UnconfiguredProvider)), &sid, &p).await;
        assert_eq!((s.filtered.seen, s.scored, s.queued), (1, 2, 2));
        let db = f.db.lock().unwrap();
        let mut ids: Vec<String> = repo::queue(&db, &f.profile, 0.0, crate::scout::model::parse_rfc3339_unix(&crate::util::now_rfc3339()).unwrap(), 50)
            .unwrap()
            .into_iter()
            .map(|q| q.post.platform_post_id)
            .collect();
        ids.sort();
        assert_eq!(ids, vec!["1000", "1002"]);
    }

    #[tokio::test]
    async fn holds_while_the_user_is_active_and_resumes_when_idle() {
        use std::sync::atomic::AtomicUsize;
        let polls = Arc::new(AtomicUsize::new(0));
        let pc = polls.clone();
        let mock = MockDriver::new().with_eval_fn(move |js| {
            if js.contains("__sghActivity") && js.contains("return window[k].last") {
                let n = pc.fetch_add(1, Ordering::SeqCst);
                return json!(if n < 3 { 200 } else { 1_000_000 });
            }
            feed(10, 0)
        });
        let b = browser(&mock).await;
        let f = fixture();
        let p = params(5, None);
        let sid = repo::create_session(&f.db.lock().unwrap(), &f.profile, &p).unwrap();
        let s = run_session(&env(&f, &b, Arc::new(UnconfiguredProvider)), &sid, &p).await;
        assert_eq!(s.reason, "post_cap");
        assert!(polls.load(Ordering::SeqCst) >= 4);
        let ev = f.sink.0.lock().unwrap();
        let names: Vec<&str> = ev.iter().map(|(e, _)| e.as_str()).collect();
        let paused = names.iter().position(|e| *e == "scout:paused").expect("paused event");
        let resumed = names.iter().position(|e| *e == "scout:resumed").expect("resumed event");
        assert!(paused < resumed);
        assert_eq!(ev[paused].1["reason"], "user_activity");
        let reads = mock.calls().iter().filter(|c| c.contains("eval") && c.contains("posts.push")).count();
        assert!(reads >= 1);
    }

    #[tokio::test]
    async fn manual_pause_holds_until_released() {
        let mock = MockDriver::new().with_eval_fn(|_| feed(10, 0));
        let b = browser(&mock).await;
        let f = fixture();
        f.pause.store(true, Ordering::SeqCst);
        let p = params(5, None);
        let sid = repo::create_session(&f.db.lock().unwrap(), &f.profile, &p).unwrap();
        let e = env(&f, &b, Arc::new(UnconfiguredProvider));
        let run = run_session(&e, &sid, &p);
        let release = async {
            tokio::time::sleep(Duration::from_millis(40)).await;
            f.pause.store(false, Ordering::SeqCst);
        };
        let (s, _) = tokio::join!(run, release);
        assert_eq!(s.reason, "post_cap");
        let ev = f.sink.0.lock().unwrap();
        assert_eq!(ev.iter().find(|(e, _)| e == "scout:paused").unwrap().1["reason"], "manual");
        assert!(ev.iter().any(|(e, _)| e == "scout:resumed"));
    }

    #[test]
    fn feed_targets_cover_home_and_watchlist() {
        let w = vec!["naval".to_string(), "paulg".to_string()];
        assert_eq!(targets("for_you", &w), vec![X_HOME_URL.to_string()]);
        assert_eq!(targets("watchlist", &w), vec!["https://x.com/naval".to_string(), "https://x.com/paulg".to_string()]);
        assert_eq!(targets("both", &w).len(), 3);
    }

    #[test]
    fn following_is_its_own_feed_target() {
        let w = vec!["naval".to_string()];
        assert_eq!(targets("following", &w), vec![X_FOLLOWING_URL.to_string()]);
        assert_eq!(targets("all", &w), vec![X_HOME_URL.to_string(), X_FOLLOWING_URL.to_string(), "https://x.com/naval".to_string()]);
        assert_eq!(targets("both", &w), vec![X_HOME_URL.to_string(), "https://x.com/naval".to_string()]);
    }

    #[tokio::test]
    async fn following_session_selects_the_following_tab_and_labels_the_source() {
        let mock = MockDriver::new().with_eval_fn(|js| if js.contains("[role=\\\"tab\\\"]") || js.contains("role=\\\"tab") { json!({ "x": 120.0, "y": 48.0 }) } else { feed(12, 0) });
        let b = browser(&mock).await;
        let f = fixture();
        let p = SessionParams { feed_source: "following".into(), ..params(100, None) };
        let sid = repo::create_session(&f.db.lock().unwrap(), &f.profile, &p).unwrap();
        run_session(&env(&f, &b, Arc::new(UnconfiguredProvider)), &sid, &p).await;
        let calls = mock.calls();
        assert!(calls.iter().any(|c| *c == format!("navigate {X_FOLLOWING_URL}")));
        assert!(!calls.iter().any(|c| *c == format!("navigate {X_HOME_URL}")), "following-only sessions skip the For You target");
        assert!(calls.iter().any(|c| c == "click 120,48"), "the Following tab is clicked");
        let src: String = f.db.lock().unwrap().query_row("SELECT feed_source FROM observed_posts LIMIT 1", [], |r| r.get(0)).unwrap();
        assert_eq!(src, "following");
    }

    #[tokio::test]
    async fn missing_following_tab_does_not_abort_the_session() {
        let mock = MockDriver::new().with_eval_fn(|js| if js.contains("role=\\\"tab") { json!(null) } else { feed(12, 0) });
        let b = browser(&mock).await;
        let f = fixture();
        let p = SessionParams { feed_source: "following".into(), ..params(100, None) };
        let sid = repo::create_session(&f.db.lock().unwrap(), &f.profile, &p).unwrap();
        let s = run_session(&env(&f, &b, Arc::new(UnconfiguredProvider)), &sid, &p).await;
        assert!(!mock.calls().iter().any(|c| c.starts_with("click ")));
        assert_ne!(s.reason, "browser_error");
    }

    #[tokio::test]
    async fn watchlist_session_visits_each_profile_and_labels_the_source() {
        let mock = MockDriver::new().with_eval_fn(|_| feed(12, 0));
        let b = browser(&mock).await;
        let f = fixture();
        crate::storage::repo::set_setting(&f.db.lock().unwrap(), "scout_watchlist", r#"["naval","paulg"]"#).unwrap();
        let p = SessionParams { feed_source: "watchlist".into(), ..params(100, None) };
        let sid = repo::create_session(&f.db.lock().unwrap(), &f.profile, &p).unwrap();
        run_session(&env(&f, &b, Arc::new(UnconfiguredProvider)), &sid, &p).await;
        let calls = mock.calls();
        let nav = |u: &str| calls.iter().any(|c| *c == format!("navigate {u}"));
        assert!(nav("https://x.com/naval") && nav("https://x.com/paulg"));
        assert!(!nav("https://x.com/home"), "watchlist-only sessions skip the home feed");
        let src: String = f.db.lock().unwrap().query_row("SELECT feed_source FROM observed_posts LIMIT 1", [], |r| r.get(0)).unwrap();
        assert_eq!(src, "watchlist");
    }

    #[tokio::test]
    async fn thumbnails_reach_the_scoring_request_and_sensitive_posts_are_dropped() {
        let mock = MockDriver::new().with_eval_fn(|js| {
            if js.contains("createImageBitmap") {
                return json!("QUJD");
            }
            let mut f = feed(3, 0);
            f["posts"][0]["image_urls"] = json!(["https://pbs.twimg.com/media/a.jpg"]);
            f["posts"][1]["sensitive"] = json!(true);
            f["posts"][2]["quoted"] = json!("the quoted take");
            f
        });
        let b = browser(&mock).await;
        let f = fixture();
        let dir = tempfile::tempdir().unwrap();
        let p = params(100, None);
        let sid = repo::create_session(&f.db.lock().unwrap(), &f.profile, &p).unwrap();
        let provider = Arc::new(ScriptedProvider::new(vec![r#"{"scores":[]}"#]));
        let mut e = env(&f, &b, provider.clone());
        e.thumbs_dir = Some(dir.path().to_path_buf());
        let s = run_session(&e, &sid, &p).await;
        assert_eq!(s.filtered.sensitive, 1);
        let req = &provider.requests.lock().unwrap()[0];
        assert_eq!(req.images, vec!["QUJD".to_string()]);
        assert!(req.user.contains("the quoted take"));
        assert!(dir.path().join("1000.jpg").exists());
        let ctx: Option<String> = f.db.lock().unwrap().query_row("SELECT context_json FROM observed_posts WHERE platform_post_id = '1002'", [], |r| r.get(0)).unwrap();
        assert_eq!(ctx.as_deref(), Some("Quoted post: the quoted take"));
    }

    #[tokio::test]
    async fn challenge_and_finish_are_written_to_the_activity_log() {
        let mock = MockDriver::new().with_eval_fn(|_| json!({ "state": "challenge", "me": null, "posts": [] }));
        let b = browser(&mock).await;
        let f = fixture();
        let p = params(100, None);
        let sid = repo::create_session(&f.db.lock().unwrap(), &f.profile, &p).unwrap();
        run_session(&env(&f, &b, Arc::new(UnconfiguredProvider)), &sid, &p).await;
        let l = crate::activity::list(&f.db.lock().unwrap(), &f.profile, 10).unwrap();
        assert!(l.iter().any(|e| e.kind == "challenge") && l.iter().any(|e| e.kind == "scout" && e.ok == Some(false)));
        assert!(f.sink.0.lock().unwrap().iter().any(|(e, p)| e == "notify" && p["kind"] == "attention"));
    }

    #[tokio::test]
    async fn every_blocking_page_state_halts_scout_and_reports_the_halt() {
        for page in ["logged_out", "challenge", "suspended", "locked"] {
            let mock = MockDriver::new().with_eval_fn(move |_| json!({ "state": page, "me": null, "posts": [] }));
            let b = browser(&mock).await;
            let f = fixture();
            let p = params(100, None);
            let sid = repo::create_session(&f.db.lock().unwrap(), &f.profile, &p).unwrap();
            let s = run_session(&env(&f, &b, Arc::new(UnconfiguredProvider)), &sid, &p).await;
            assert_eq!(s.reason, page);
            let ev = f.sink.0.lock().unwrap();
            assert!(ev.iter().any(|(e, v)| e == "browser:halt" && v["state"] == page && v["source"] == "scout"), "{page}");
            assert!(ev.iter().any(|(e, v)| e == "notify" && v["kind"] == "attention"), "{page}");
        }
    }

    #[tokio::test]
    async fn a_closed_window_reports_a_crash_halt() {
        let mock = MockDriver::new();
        let b = BrowserService::new(Box::new(mock));
        let f = fixture();
        let p = params(100, None);
        let sid = repo::create_session(&f.db.lock().unwrap(), &f.profile, &p).unwrap();
        let s = run_session(&env(&f, &b, Arc::new(UnconfiguredProvider)), &sid, &p).await;
        assert_eq!(s.reason, "browser_error");
        assert!(f.sink.0.lock().unwrap().iter().any(|(e, v)| e == "browser:halt" && v["state"] == "crashed"));
    }

    fn reads_fail_first(n: u32) -> impl Fn(&str) -> bool + Send + Sync + 'static {
        let seen = Arc::new(std::sync::atomic::AtomicU32::new(0));
        move |js: &str| js.contains("const REPLYING") && seen.fetch_add(1, Ordering::SeqCst) < n
    }

    fn activity_titles(f: &Fixture) -> Vec<(String, Option<bool>)> {
        let db = f.db.lock().unwrap();
        crate::activity::list(&db, &f.profile, 50).unwrap().into_iter().map(|a| (a.title, a.ok)).collect()
    }

    #[tokio::test]
    async fn transient_read_errors_do_not_end_the_session() {
        let mock = MockDriver::new().with_eval_fn(|_| feed(12, 0)).with_eval_failure(reads_fail_first(3));
        let b = browser(&mock).await;
        let f = fixture();
        let p = params(8, None);
        let sid = repo::create_session(&f.db.lock().unwrap(), &f.profile, &p).unwrap();
        let s = run_session(&env(&f, &b, Arc::new(UnconfiguredProvider)), &sid, &p).await;
        assert_eq!((s.reason.as_str(), s.scanned), ("post_cap", 8));
        let ev = f.sink.0.lock().unwrap();
        assert_eq!(ev.iter().filter(|(e, _)| e == "scout:warning").count(), 3);
        assert!(!ev.iter().any(|(e, _)| e == "scout:error"));
        drop(ev);
        let titles = activity_titles(&f);
        assert_eq!(titles.iter().filter(|(t, _)| t.contains("hiccup")).count(), 1, "one log line per streak: {titles:?}");
        assert!(titles.iter().all(|(t, ok)| !t.contains("hiccup") || ok.is_none()), "a soft error is neither success nor failure");
        assert!(titles.iter().any(|(t, ok)| t == "Scout finished (post_cap)" && *ok == Some(true)));
    }

    #[tokio::test]
    async fn transient_scroll_errors_are_retried() {
        let n = std::sync::atomic::AtomicU32::new(0);
        let mock = MockDriver::new().with_eval_fn(move |_| feed(3, (n.fetch_add(1, Ordering::SeqCst) as usize) * 3));
        mock.fail_next_wheels(3);
        let b = browser(&mock).await;
        let f = fixture();
        let p = params(100, Some(9));
        let sid = repo::create_session(&f.db.lock().unwrap(), &f.profile, &p).unwrap();
        let s = run_session(&env(&f, &b, Arc::new(UnconfiguredProvider)), &sid, &p).await;
        assert_eq!(s.reason, "queue_goal");
        assert_eq!(f.sink.0.lock().unwrap().iter().filter(|(e, _)| e == "scout:warning").count(), 3);
    }

    struct Reopen {
        svc: Arc<BrowserService>,
        times: Arc<std::sync::atomic::AtomicU32>,
        on_purpose: bool,
    }

    #[async_trait::async_trait]
    impl BrowserRelauncher for Reopen {
        async fn relaunch(&self) -> Result<(), String> {
            self.times.fetch_add(1, Ordering::SeqCst);
            if self.on_purpose {
                return Err("the X window was closed on purpose".into());
            }
            let cfg = ChromeLaunchConfig { binary_path: "/x".into(), profile_dir: "/p".into(), start_url: X_HOME_URL.into(), width: 1, height: 1, invisible: false };
            self.svc.launch(cfg).await.map(|_| ()).map_err(|e| e.to_string())
        }
    }

    #[tokio::test]
    async fn a_crashed_window_is_reopened_and_the_session_continues() {
        let mock = MockDriver::new();
        let m = mock.clone();
        let reads = std::sync::atomic::AtomicU32::new(0);
        let mock = mock.with_eval_fn(move |js| {
            if js.contains("const REPLYING") && reads.fetch_add(1, Ordering::SeqCst) == 1 {
                m.crash();
            }
            feed(3, (reads.load(Ordering::SeqCst) as usize) * 3)
        });
        let svc = Arc::new(browser(&mock).await);
        let times = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let reopen = Reopen { svc: svc.clone(), times: times.clone(), on_purpose: false };
        let f = fixture();
        let p = params(100, Some(12));
        let sid = repo::create_session(&f.db.lock().unwrap(), &f.profile, &p).unwrap();
        let e = ScoutEnv { relaunch: Some(&reopen), ..env(&f, &svc, Arc::new(UnconfiguredProvider)) };
        let s = run_session(&e, &sid, &p).await;
        assert_eq!(s.reason, "queue_goal");
        assert_eq!(times.load(Ordering::SeqCst), 1);
        assert!(mock.calls().iter().filter(|c| c.starts_with("launch")).count() >= 2);
    }

    #[tokio::test]
    async fn a_window_closed_on_purpose_ends_the_session_as_a_hard_failure() {
        let mock = MockDriver::new();
        let svc = Arc::new(BrowserService::new(Box::new(mock)));
        let times = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let reopen = Reopen { svc: svc.clone(), times: times.clone(), on_purpose: true };
        let f = fixture();
        let p = params(100, None);
        let sid = repo::create_session(&f.db.lock().unwrap(), &f.profile, &p).unwrap();
        let e = ScoutEnv { relaunch: Some(&reopen), ..env(&f, &svc, Arc::new(UnconfiguredProvider)) };
        let s = run_session(&e, &sid, &p).await;
        assert_eq!(s.reason, "browser_error");
        assert_eq!(times.load(Ordering::SeqCst), 1, "no retry loop once the window is known to be closed on purpose");
        assert!(f.sink.0.lock().unwrap().iter().any(|(e, _)| e == "scout:error"));
    }

    #[tokio::test]
    async fn persistent_errors_stop_only_after_all_retries() {
        let mock = MockDriver::new().with_eval_fn(|_| feed(12, 0)).with_eval_failure(|js| js.contains("const REPLYING"));
        let b = browser(&mock).await;
        let f = fixture();
        let p = params(100, None);
        let sid = repo::create_session(&f.db.lock().unwrap(), &f.profile, &p).unwrap();
        let s = run_session(&env(&f, &b, Arc::new(UnconfiguredProvider)), &sid, &p).await;
        assert_eq!(s.reason, "browser_error");
        let ev = f.sink.0.lock().unwrap();
        assert_eq!(ev.iter().filter(|(e, _)| e == "scout:warning").count() as u32, MAX_SOFT_ERRORS + 1);
        assert!(ev.iter().any(|(e, v)| e == "scout:error" && v["message"].as_str().unwrap().contains("retries")));
        drop(ev);
        assert!(activity_titles(&f).iter().any(|(t, ok)| t == "Scout finished (browser_error)" && *ok == Some(false)));
    }

    struct BlockingProvider {
        block_slot: TaskSlot,
        entered: Arc<tokio::sync::Notify>,
        scoring_response: Option<String>,
        draft_calls: Arc<std::sync::atomic::AtomicU32>,
    }

    #[async_trait::async_trait]
    impl CompletionProvider for BlockingProvider {
        fn is_configured(&self, _task: TaskSlot) -> bool {
            true
        }
        async fn complete(&self, req: CompletionRequest) -> Result<crate::completion::Completion, crate::completion::CompletionError> {
            if req.task == TaskSlot::ReplyDrafting {
                self.draft_calls.fetch_add(1, Ordering::SeqCst);
            }
            if req.task == self.block_slot {
                self.entered.notify_one();
                tokio::time::sleep(Duration::from_secs(30)).await;
                return Ok(crate::completion::Completion { text: "{}".into(), model: "slow".into() });
            }
            Ok(crate::completion::Completion {
                text: self.scoring_response.clone().unwrap_or_else(|| "{}".into()),
                model: "scripted".into(),
            })
        }
    }

    #[tokio::test]
    async fn stop_cancels_in_flight_scoring_immediately_without_scrolling() {
        let mock = MockDriver::new().with_eval_fn(|_| feed(3, 0));
        let b = browser(&mock).await;
        let f = fixture();
        let p = params(100, Some(20));
        let sid = repo::create_session(&f.db.lock().unwrap(), &f.profile, &p).unwrap();
        let entered = Arc::new(tokio::sync::Notify::new());
        let provider = Arc::new(BlockingProvider {
            block_slot: TaskSlot::PostScoring,
            entered: entered.clone(),
            scoring_response: None,
            draft_calls: Arc::default(),
        });
        let e = env(&f, &b, provider);
        let t = Instant::now();
        let run = run_session(&e, &sid, &p);
        let trigger = async {
            entered.notified().await;
            f.stop.stop();
        };
        let (s, _) = tokio::join!(run, trigger);
        assert!(t.elapsed() < Duration::from_secs(1), "Stop must interrupt in-flight scoring immediately");
        assert_eq!((s.reason.as_str(), s.scored, s.queued), ("stopped", 0, 0));
        assert!(!mock.calls().iter().any(|c| c.starts_with("wheel ")), "must not scroll after Stop");
        assert_eq!(f.sink.0.lock().unwrap().last().unwrap().0, "scout:finished");
    }

    #[tokio::test]
    async fn stop_cancels_in_flight_reply_drafting_immediately_without_scrolling() {
        let mock = MockDriver::new().with_eval_fn(|_| feed(3, 0));
        let b = browser(&mock).await;
        let f = fixture();
        let p = params(100, Some(20));
        let sid = repo::create_session(&f.db.lock().unwrap(), &f.profile, &p).unwrap();
        let entered = Arc::new(tokio::sync::Notify::new());
        let draft_calls = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let provider = Arc::new(BlockingProvider {
            block_slot: TaskSlot::ReplyDrafting,
            entered: entered.clone(),
            scoring_response: Some(
                r#"{"scores":[{"id":"1000","score":92,"angle":"a","reasoning":"r"},{"id":"1001","score":91,"angle":"a","reasoning":"r"}]}"#
                    .into(),
            ),
            draft_calls: draft_calls.clone(),
        });
        let e = env(&f, &b, provider);
        let t = Instant::now();
        let run = run_session(&e, &sid, &p);
        let trigger = async {
            entered.notified().await;
            f.stop.stop();
        };
        let (s, _) = tokio::join!(run, trigger);
        assert!(t.elapsed() < Duration::from_secs(1), "Stop must interrupt in-flight drafting immediately");
        assert_eq!(s.reason, "stopped");
        assert_eq!(draft_calls.load(Ordering::SeqCst), 1, "must not start drafting remaining posts after Stop");
        assert!(!mock.calls().iter().any(|c| c.starts_with("wheel ")), "must not scroll after Stop during drafting");
        let ev = f.sink.0.lock().unwrap();
        assert!(!ev.iter().any(|(name, _)| name == "scout:progress"), "must not emit progress after Stop");
        assert_eq!(ev.last().unwrap().0, "scout:finished");
    }

    #[tokio::test]
    async fn stop_cancels_while_paused_without_resuming_or_scrolling() {
        let mock = MockDriver::new().with_eval_fn(|_| feed(5, 0));
        let b = browser(&mock).await;
        let f = fixture();
        f.pause.store(true, Ordering::SeqCst);
        let p = params(100, Some(20));
        let sid = repo::create_session(&f.db.lock().unwrap(), &f.profile, &p).unwrap();
        let mut e = env(&f, &b, Arc::new(UnconfiguredProvider));
        e.idle_poll_ms = 30_000;
        let t = Instant::now();
        let run = run_session(&e, &sid, &p);
        let trigger = async {
            tokio::time::sleep(Duration::from_millis(30)).await;
            f.stop.stop();
        };
        let (s, _) = tokio::join!(run, trigger);
        assert!(t.elapsed() < Duration::from_secs(1));
        assert_eq!((s.reason.as_str(), s.scanned), ("stopped", 0));
        assert!(!mock.calls().iter().any(|c| c.starts_with("wheel ")));
        let ev = f.sink.0.lock().unwrap();
        assert!(!ev.iter().any(|(name, _)| name == "scout:resumed"));
        assert_eq!(ev.last().unwrap().0, "scout:finished");
    }

    #[tokio::test]
    async fn stop_cancels_during_initial_settle_before_reading_feed() {
        let mock = MockDriver::new().with_eval_fn(|_| feed(5, 0));
        let b = browser(&mock).await;
        let f = fixture();
        let p = params(100, Some(20));
        let sid = repo::create_session(&f.db.lock().unwrap(), &f.profile, &p).unwrap();
        let mut e = env(&f, &b, Arc::new(UnconfiguredProvider));
        e.pause_ms = (20_000, 20_000);
        let t = Instant::now();
        let run = run_session(&e, &sid, &p);
        let trigger = async {
            tokio::time::sleep(Duration::from_millis(30)).await;
            f.stop.stop();
        };
        let (s, _) = tokio::join!(run, trigger);
        assert!(t.elapsed() < Duration::from_secs(1));
        assert_eq!((s.reason.as_str(), s.scanned), ("stopped", 0));
        assert!(!mock.calls().iter().any(|c| c.contains("posts.push") || c.starts_with("wheel ")));
    }

    #[tokio::test]
    async fn input_dispatch_mouse_event_timeouts_do_not_end_scout_session() {
        let reads = std::sync::atomic::AtomicU32::new(0);
        // Exceed both MAX_SOFT_ERRORS (8) and EXHAUSTED_AFTER_EMPTY_SCROLLS (14) to prove
        // focus-loss scroll timeouts never terminate Scout as browser_error or feed_exhausted.
        let timeouts = EXHAUSTED_AFTER_EMPTY_SCROLLS + 2;
        let mock = MockDriver::new().with_eval_fn(move |js| {
            if js.contains("posts.push") {
                let n = reads.fetch_add(1, Ordering::SeqCst);
                // While scrolling times out, the same 3 posts remain on screen; once scrolling
                // recovers after `timeouts` attempts, new posts scroll into view.
                let page = if n <= timeouts { 0 } else { (n - timeouts) as usize };
                return feed(3, page * 3);
            }
            json!(null)
        });
        mock.fail_next_wheels_with_timeout(timeouts);
        let b = browser(&mock).await;
        let f = fixture();
        let p = params(100, Some(9));
        let sid = repo::create_session(&f.db.lock().unwrap(), &f.profile, &p).unwrap();
        let s = run_session(&env(&f, &b, Arc::new(UnconfiguredProvider)), &sid, &p).await;
        assert_eq!(s.reason, "queue_goal", "repeated CDP Input.dispatchMouseEvent timeouts must not end Scout");
        let ev = f.sink.0.lock().unwrap();
        assert_eq!(ev.iter().filter(|(e, _)| e == "scout:warning").count() as u32, timeouts);
        assert!(!ev.iter().any(|(e, _)| e == "scout:error"));
        // Only the initial navigation happened; focus timeouts must not reload the feed to the top.
        assert_eq!(mock.calls().iter().filter(|c| c.starts_with("navigate ")).count(), 1);
    }

    #[tokio::test]
    async fn input_dispatch_mouse_event_timeouts_still_stop_immediately_on_user_stop() {
        let mock = MockDriver::new().with_eval_fn(|_| feed(3, 0));
        mock.fail_next_wheels_with_timeout(100);
        let b = browser(&mock).await;
        let f = fixture();
        let p = params(100, Some(20));
        let sid = repo::create_session(&f.db.lock().unwrap(), &f.profile, &p).unwrap();
        let e = env(&f, &b, Arc::new(UnconfiguredProvider));
        let t = Instant::now();
        let run = run_session(&e, &sid, &p);
        let trigger = async {
            tokio::time::sleep(Duration::from_millis(25)).await;
            f.stop.stop();
        };
        let (s, _) = tokio::join!(run, trigger);
        assert!(t.elapsed() < Duration::from_secs(1));
        assert_eq!(s.reason, "stopped");
    }

    #[tokio::test]
    async fn input_dispatch_mouse_event_timeouts_stop_when_window_closed_on_purpose() {
        let mock = MockDriver::new();
        let m = mock.clone();
        let reads = std::sync::atomic::AtomicU32::new(0);
        let mock = mock.with_eval_fn(move |js| {
            if js.contains(".busy = true") && reads.load(Ordering::SeqCst) >= 1 {
                m.crash();
            } else if js.contains("posts.push") {
                reads.fetch_add(1, Ordering::SeqCst);
            }
            feed(3, 0)
        });
        mock.fail_next_wheels_with_timeout(5);
        let svc = Arc::new(browser(&mock).await);
        let times = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let reopen = Reopen { svc: svc.clone(), times: times.clone(), on_purpose: true };
        let f = fixture();
        let p = params(100, Some(20));
        let sid = repo::create_session(&f.db.lock().unwrap(), &f.profile, &p).unwrap();
        let e = ScoutEnv { relaunch: Some(&reopen), ..env(&f, &svc, Arc::new(UnconfiguredProvider)) };
        let s = run_session(&e, &sid, &p).await;
        assert_eq!(s.reason, "browser_error");
        assert_eq!(times.load(Ordering::SeqCst), 1);
    }
}
