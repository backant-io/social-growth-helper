//! Background analytics worker. Runs for the life of the app, on the invisible publish browser only (never the
//! visible Scout window), and does one small thing per tick: read one post page, or one profile page every few
//! hours. Nothing here is user-triggered and nothing is shown when it fails; the dashboard just updates when
//! numbers arrive (`analytics:updated`).
//!
//! `plan` decides what to do and `execute` does it, so tests drive both without a browser. Budgets, jitter,
//! backoff and the AI self-heal limits live in `pacing`.

use super::extract::{
    account_analytics_range_locator_js, parse_account_overview, parse_post_metrics,
    ACCOUNT_ANALYTICS_DOM_JS, ACCOUNT_ANALYTICS_URL, PROFILE_TIMELINE_DOM_JS,
};
use super::health;
use super::ingest::{self, AnalyticsPageFetcher};
use super::pacing::{self, UnitKind};
use super::sandbox::canonical_period_days;
use super::schedule::{compute_schedule, now_unix_secs, TrackedPostSchedule};
use super::vision_heal::{LiveHealer, PostHealer};
use crate::browser::{AnalyticsPageCapture, AnalyticsPageRequest};
use crate::control::EventSink;
use crate::error::AppResult;
use crate::phase3::{Phase3, TauriSink};
use crate::state::AppState;
use crate::storage::repo;
use rusqlite::Connection;
use serde_json::json;
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Manager};

const STARTUP_GRACE_SECS: u64 = 45;
const BUSY_POLL_SECS: u64 = 120;
const SETTLE_POST_MS: u64 = 3500;
const SETTLE_PROFILE_MS: u64 = 2500;
const SETTLE_ACCOUNT_MS: u64 = 3500;
const SETTLE_ACCOUNT_RANGE_CLICK_MS: u64 = 2500;
const PROFILE_WINDOW_DAYS: i64 = 30;
const PERIOD_LOG_TARGET: &str = "analytics::period";
const LAST_SWEEP_KEY: &str = "analytics_last_sweep_at";
const SWEEP_STAGE_KEY: &str = "analytics_sweep_stage";

#[derive(Debug, Clone, PartialEq)]
pub enum Task {
    AccountPeriod(u32),
    Post(Box<TrackedPostSchedule>),
    Profile,
    Replies,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Plan {
    Idle,
    Paused,
    Throttled,
    Work(Task),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TickOutcome {
    Idle,
    Paused,
    Throttled,
    /// X needs the user (login, challenge, lock, suspension); checked again much later.
    Blocked(String),
    Synced,
    Healed,
    Missing,
    Failed,
}

/// Seconds until the next tick. Work is spread by a random gap so the cadence never looks mechanical.
pub fn next_delay_secs(outcome: &TickOutcome, entropy: u64) -> u64 {
    match outcome {
        TickOutcome::Synced | TickOutcome::Healed | TickOutcome::Missing | TickOutcome::Failed => pacing::jitter_secs(entropy),
        TickOutcome::Idle => pacing::IDLE_POLL_SECS,
        TickOutcome::Throttled => pacing::THROTTLED_POLL_SECS,
        TickOutcome::Paused => pacing::PAUSED_POLL_SECS,
        TickOutcome::Blocked(_) => pacing::BLOCKED_POLL_SECS,
    }
}

fn setting_i64(conn: &Connection, key: &str) -> i64 {
    repo::get_setting(conn, key).ok().flatten().and_then(|v| v.parse().ok()).unwrap_or(0)
}

fn pick_due_post(conn: &Connection, profile_id: &str, now: i64) -> AppResult<Option<TrackedPostSchedule>> {
    let mut items = compute_schedule(conn, profile_id, now)?.items;
    items.sort_by(|a, b| a.age_hours.partial_cmp(&b.age_hours).unwrap_or(std::cmp::Ordering::Equal));
    for item in items {
        if item.age_hours > pacing::MAX_TRACKED_AGE_HOURS {
            continue;
        }
        let state = pacing::load_state(conn, profile_id, &item.platform_post_id)?;
        let already_covered = state.is_none() && item.captured_count > 0 && !item.needs_sync;
        if !already_covered && pacing::is_due(state.as_ref(), now) {
            return Ok(Some(item));
        }
    }
    Ok(None)
}

fn has_account_context(conn: &Connection, profile_id: &str, handle_known: bool) -> bool {
    if handle_known {
        return true;
    }
    let has_activity: i64 = conn
        .query_row(
            "SELECT
               (SELECT COUNT(*) FROM reply_drafts WHERE profile_id = ?1 AND status = 'published') +
               (SELECT COUNT(*) FROM ogc_drafts WHERE profile_id = ?1 AND status = 'published') +
               (SELECT COUNT(*) FROM analytics_snapshots WHERE profile_id = ?1) +
               (SELECT COUNT(*) FROM analytics_period_snapshots WHERE profile_id = ?1)",
            [profile_id],
            |r| r.get(0),
        )
        .unwrap_or(0);
    has_activity > 0
}

/// The window to capture next: a window with no stored row at all goes first (nothing to show yet, so no
/// failure backoff), then any window whose regular refresh is due. The preferred window leads in both cases.
fn pick_due_period(conn: &Connection, profile_id: &str, now: i64) -> Option<u32> {
    let preferred = pacing::preferred_window_days(conn);
    let ordered = if preferred <= 7 { [7u32, 30u32] } else { [30u32, 7u32] };
    ordered
        .into_iter()
        .find(|&days| pacing::period_first_run_due(conn, profile_id, days, now))
        .or_else(|| ordered.into_iter().find(|&days| pacing::period_next_due_at(conn, days) <= now))
}

/// What the next tick should do. Profile sweeps need a known handle, which the first post or account visit teaches us.
/// A due account-period capture goes before every other task and is the only one allowed to spend the units
/// reserved for periods.
pub fn plan(conn: &Connection, profile_id: &str, now: i64) -> AppResult<Plan> {
    if pacing::pause_until(conn).is_some_and(|until| until > now) {
        return Ok(Plan::Paused);
    }
    let handle_known = ingest::profile_handle(conn, profile_id).ok().flatten().is_some();
    let account_context = has_account_context(conn, profile_id, handle_known);
    if account_context {
        if let Some(days) = pick_due_period(conn, profile_id, now) {
            return Ok(if pacing::can_spend(conn, profile_id, now, pacing::DOM_UNITS)? {
                Plan::Work(Task::AccountPeriod(days))
            } else {
                Plan::Throttled
            });
        }
    }
    let task = if handle_known && repo::get_setting(conn, SWEEP_STAGE_KEY)?.as_deref() == Some("replies") {
        Some(Task::Replies)
    } else if handle_known && now - setting_i64(conn, LAST_SWEEP_KEY) >= pacing::SWEEP_INTERVAL_SECS {
        Some(Task::Profile)
    } else {
        pick_due_post(conn, profile_id, now)?.map(|p| Task::Post(Box::new(p)))
    };
    let Some(task) = task else { return Ok(Plan::Idle) };
    if !pacing::can_spend_unreserved(conn, profile_id, now, pacing::DOM_UNITS)? {
        return Ok(Plan::Throttled);
    }
    Ok(Plan::Work(task))
}

pub struct TickEnv<'a> {
    pub db: &'a Mutex<Connection>,
    pub profile_id: &'a str,
    pub fetcher: &'a dyn AnalyticsPageFetcher,
    pub healer: Option<&'a dyn PostHealer>,
    pub sink: &'a dyn EventSink,
}

fn block_reason(cap: &AnalyticsPageCapture) -> Option<String> {
    if let Some(state) = cap.dom_result.get("state").and_then(|v| v.as_str()) {
        if matches!(state, "logged_out" | "challenge" | "locked" | "suspended") {
            return Some(state.to_string());
        }
    }
    let url = cap.final_url.to_lowercase();
    (url.contains("/i/flow/login") || url.contains("/login")).then(|| "logged_out".to_string())
}

fn post_url(handle: Option<&str>, item: &TrackedPostSchedule) -> String {
    match handle {
        Some(h) => format!("https://x.com/{h}/status/{}", item.platform_post_id),
        None => format!("https://x.com/i/web/status/{}", item.platform_post_id),
    }
}

fn updated(env: &TickEnv<'_>, what: &str) {
    env.sink.emit("analytics:updated", json!({ "source": what }));
}

fn tick_result(env: &TickEnv<'_>, ok: bool, now: i64) {
    let db = env.db.lock().unwrap();
    let _ = pacing::note_tick_result(&db, ok, now);
}

fn blocked(env: &TickEnv<'_>, reason: &str) -> TickOutcome {
    let db = env.db.lock().unwrap();
    let _ = health::set_blocked(&db, reason);
    TickOutcome::Blocked(reason.to_string())
}

pub async fn run_tick(env: &TickEnv<'_>, now: i64) -> TickOutcome {
    let planned = {
        let db = env.db.lock().unwrap();
        plan(&db, env.profile_id, now)
    };
    match planned {
        Ok(Plan::Work(task)) => execute(env, task, now).await,
        Ok(Plan::Idle) | Err(_) => TickOutcome::Idle,
        Ok(Plan::Paused) => TickOutcome::Paused,
        Ok(Plan::Throttled) => TickOutcome::Throttled,
    }
}

pub async fn execute(env: &TickEnv<'_>, task: Task, now: i64) -> TickOutcome {
    match task {
        Task::AccountPeriod(days) => execute_account_period(env, days, now).await,
        Task::Post(item) => execute_post(env, &item, now).await,
        Task::Profile => execute_sweep(env, false, now).await,
        Task::Replies => execute_sweep(env, true, now).await,
    }
}

/// One account-period capture, logged step by step to app.log. The steps are `page_open` (load the analytics
/// page), `range_click` (the 7D/30D control took effect), `dom_read` (the numbers were found), `validate`
/// (they are plausible) and `store`. A failure keeps its `step: reason` in `analytics_period_<n>d_last_error`.
struct PeriodTrace {
    days: u32,
}

impl PeriodTrace {
    fn ok(&self, step: &str, detail: &str) {
        log::info!(target: PERIOD_LOG_TARGET, "window={}d step={step} outcome=ok {detail}", self.days);
    }

    fn skipped(&self, step: &str, reason: &str) {
        log::info!(target: PERIOD_LOG_TARGET, "window={}d step={step} outcome=skipped reason={reason}", self.days);
    }

    fn fail(&self, step: &str, reason: &str) -> String {
        log::warn!(target: PERIOD_LOG_TARGET, "window={}d step={step} outcome=fail reason={reason}", self.days);
        format!("{step}: {reason}")
    }
}

fn single_line(text: &str) -> String {
    let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    flat.chars().take(200).collect()
}

async fn execute_account_period(env: &TickEnv<'_>, period_days: u32, now: i64) -> TickOutcome {
    let pid = env.profile_id;
    let target_canonical = canonical_period_days(period_days);
    let trace = PeriodTrace { days: target_canonical as u32 };
    {
        let db = env.db.lock().unwrap();
        let _ = pacing::record_period_units(&db, pid, now, period_days);
    }
    let fail_period = |error: &str| {
        let db = env.db.lock().unwrap();
        let _ = pacing::mark_period_failed(&db, period_days, now);
        let _ = pacing::set_period_error(&db, period_days, Some(error));
        drop(db);
        tick_result(env, false, now);
        TickOutcome::Failed
    };

    let req = AnalyticsPageRequest {
        url: ACCOUNT_ANALYTICS_URL.to_string(),
        network_url_filters: vec![],
        dom_script: ACCOUNT_ANALYTICS_DOM_JS.to_string(),
        settle_ms: SETTLE_ACCOUNT_MS,
        pre_click_locator_js: Some(account_analytics_range_locator_js(period_days)),
        post_click_settle_ms: SETTLE_ACCOUNT_RANGE_CLICK_MS,
    };

    let cap = match env.fetcher.fetch_page(req).await {
        Ok(c) => c,
        Err(e) => {
            let error = trace.fail("page_open", &single_line(&e.to_string()));
            return fail_period(&error);
        }
    };
    if let Some(reason) = block_reason(&cap) {
        let error = trace.fail("page_open", &format!("blocked by X ({reason}), waiting for the user"));
        {
            let db = env.db.lock().unwrap();
            let _ = pacing::set_period_error(&db, period_days, Some(&error));
        }
        return blocked(env, &reason);
    }
    trace.ok("page_open", &format!("final_url={}", single_line(&cap.final_url)));

    let metrics = parse_account_overview(&cap.network_responses, &cap.dom_result);
    if let Some(h) = &metrics.handle {
        let db = env.db.lock().unwrap();
        ingest::update_profile_handle_if_empty(&db, pid, h);
    }
    if metrics.is_logged_out {
        let error = trace.fail("page_open", "blocked by X (logged_out), waiting for the user");
        {
            let db = env.db.lock().unwrap();
            let _ = pacing::set_period_error(&db, period_days, Some(&error));
        }
        return blocked(env, "logged_out");
    }
    if metrics.is_premium_upsell {
        trace.skipped("dom_read", "premium_upsell: account analytics needs X Premium, retrying in 24h");
        let db = env.db.lock().unwrap();
        let _ = pacing::mark_period_upsell(&db, now);
        let _ = pacing::set_period_error(&db, period_days, Some("dom_read: premium_upsell"));
        drop(db);
        tick_result(env, true, now);
        return TickOutcome::Synced;
    }

    let range_matches = metrics
        .period_days
        .map(|d| canonical_period_days(d) == target_canonical)
        .unwrap_or(true);
    let mut root_error: Option<String> = None;
    match metrics.period_days {
        Some(d) if range_matches => trace.ok("range_click", &format!("page_range={d}d")),
        Some(d) => {
            root_error = Some(trace.fail("range_click", &format!("page still shows {d}d, wanted {target_canonical}d")));
        }
        None => trace.ok("range_click", "page_range=unknown, assuming the control took effect"),
    }

    if root_error.is_none() {
        if metrics.source == "empty" {
            root_error = Some(trace.fail("dom_read", "no analytics numbers found on the page (selectors matched nothing)"));
        } else {
            trace.ok("dom_read", &format!("source={}", metrics.source));
            if !ingest::account_metrics_are_sane(&metrics) {
                root_error = Some(trace.fail("validate", "metrics failed the sanity check (negative, out of range or no usable number)"));
            } else {
                trace.ok("validate", "");
                let stored = {
                    let db = env.db.lock().unwrap();
                    ingest::ingest_account_period_metrics(&db, pid, period_days, &metrics, now)
                };
                match stored {
                    Ok(true) => {
                        trace.ok("store", "");
                        let db = env.db.lock().unwrap();
                        let _ = pacing::mark_period_synced(&db, period_days, now);
                        drop(db);
                        tick_result(env, true, now);
                        updated(env, "account_period");
                        return TickOutcome::Synced;
                    }
                    Ok(false) => root_error = Some(trace.fail("store", "ingest refused the metrics")),
                    Err(e) => root_error = Some(trace.fail("store", &single_line(&e.to_string()))),
                }
            }
        }
    }

    // The subscription analytics page loaded, wasn't an upsell, and DOM extraction came back empty
    // or couldn't verify the selected range: try one vision heal under the shared AI heal budget.
    let mut heal_note = String::new();
    if let Some(healer) = env.healer.filter(|h| h.available()) {
        let may_heal = {
            let db = env.db.lock().unwrap();
            let last = pacing::period_last_heal_at(&db, period_days);
            pacing::can_ai_heal(&db, pid, now, last).unwrap_or(false)
        };
        if may_heal {
            {
                let db = env.db.lock().unwrap();
                let _ = pacing::mark_period_healed(&db, period_days, now);
            }
            let healed = healer.heal_account_period(ACCOUNT_ANALYTICS_URL, period_days).await;
            let kind = if healed.is_ok() { UnitKind::AiOk } else { UnitKind::AiFail };
            {
                let db = env.db.lock().unwrap();
                let _ = pacing::record_units(&db, pid, now, pacing::AI_HEAL_UNITS, kind);
            }
            match healed {
                Ok(m) => {
                    let stored = {
                        let db = env.db.lock().unwrap();
                        let ok = ingest::ingest_account_period_metrics(&db, pid, period_days, &m, now).unwrap_or(false);
                        if ok {
                            let _ = pacing::mark_period_synced(&db, period_days, now);
                            crate::activity::record(
                                &db,
                                pid,
                                "action",
                                "Analytics read account totals from a screenshot",
                                Some("The analytics page layout changed, so the period totals were read visually."),
                                Some(true),
                                None,
                            );
                        }
                        ok
                    };
                    if stored {
                        trace.ok("store", "source=vision_heal");
                        tick_result(env, true, now);
                        updated(env, "account_period");
                        return TickOutcome::Healed;
                    }
                    heal_note = format!("; vision heal: {}", trace.fail("validate", "vision heal result was refused"));
                }
                Err(e) => heal_note = format!("; vision heal: {}", trace.fail("dom_read", &format!("vision heal failed: {}", single_line(&e)))),
            }
        } else {
            trace.skipped("dom_read", "vision heal not allowed right now (AI heal limits or budget)");
        }
    }

    let error = root_error.unwrap_or_else(|| "store: nothing was stored".to_string());
    fail_period(&format!("{error}{heal_note}"))
}

async fn execute_post(env: &TickEnv<'_>, item: &TrackedPostSchedule, now: i64) -> TickOutcome {
    let pid = env.profile_id;
    let post_id = item.platform_post_id.as_str();
    let handle = {
        let db = env.db.lock().unwrap();
        let _ = pacing::record_units(&db, pid, now, pacing::DOM_UNITS, UnitKind::Dom);
        ingest::profile_handle(&db, pid).ok().flatten()
    };
    let url = post_url(handle.as_deref(), item);
    let req = AnalyticsPageRequest {
        url: url.clone(),
        network_url_filters: vec![],
        dom_script: crate::selectors::post_page_js(post_id),
        settle_ms: SETTLE_POST_MS,
        ..Default::default()
    };

    let cap = match env.fetcher.fetch_page(req).await {
        Ok(c) => c,
        Err(_) => {
            let db = env.db.lock().unwrap();
            let _ = pacing::mark_failed(&db, pid, post_id, now);
            drop(db);
            tick_result(env, false, now);
            return TickOutcome::Failed;
        }
    };
    if let Some(reason) = block_reason(&cap) {
        return blocked(env, &reason);
    }
    let dom = &cap.dom_result;
    if dom.get("gone").and_then(|v| v.as_bool()) == Some(true) {
        let db = env.db.lock().unwrap();
        let _ = pacing::mark_missing(&db, pid, post_id, now);
        return TickOutcome::Missing;
    }
    if let Some(h) = dom.get("handle").and_then(|v| v.as_str()) {
        let db = env.db.lock().unwrap();
        ingest::update_profile_handle_if_empty(&db, pid, h);
    }

    let metrics = parse_post_metrics(post_id, &[], dom);
    if metrics.source != "empty" {
        let stored = {
            let db = env.db.lock().unwrap();
            ingest::ingest_post_metrics(&db, pid, item, &metrics, now).unwrap_or(false)
        };
        if stored {
            tick_result(env, true, now);
            updated(env, "post");
            return TickOutcome::Synced;
        }
        let db = env.db.lock().unwrap();
        let _ = pacing::mark_failed(&db, pid, post_id, now);
        drop(db);
        tick_result(env, false, now);
        return TickOutcome::Failed;
    }

    // The page loaded, the post is there, and the selectors found nothing: the DOM probably changed.
    if let Some(healer) = env.healer.filter(|h| h.available()) {
        let may_heal = {
            let db = env.db.lock().unwrap();
            let last = pacing::load_state(&db, pid, post_id).ok().flatten().and_then(|s| s.last_heal_at);
            pacing::can_post_ai_heal(&db, pid, now, last).unwrap_or(false)
        };
        if may_heal {
            {
                let db = env.db.lock().unwrap();
                let _ = pacing::mark_healed(&db, pid, post_id, now);
            }
            let healed = healer.heal(&url, post_id).await;
            let kind = if healed.is_ok() { UnitKind::AiOk } else { UnitKind::AiFail };
            {
                let db = env.db.lock().unwrap();
                let _ = pacing::record_units(&db, pid, now, pacing::AI_HEAL_UNITS, kind);
            }
            if let Ok(m) = healed {
                let stored = {
                    let db = env.db.lock().unwrap();
                    let ok = ingest::ingest_post_metrics(&db, pid, item, &m, now).unwrap_or(false);
                    if ok {
                        crate::activity::record(&db, pid, "action", "Analytics read a post from a screenshot", Some("The page layout changed, so the numbers were read visually."), Some(true), None);
                    }
                    ok
                };
                if stored {
                    tick_result(env, true, now);
                    updated(env, "post");
                    return TickOutcome::Healed;
                }
            }
        }
    }

    let db = env.db.lock().unwrap();
    let _ = pacing::mark_failed(&db, pid, post_id, now);
    drop(db);
    tick_result(env, false, now);
    TickOutcome::Failed
}

async fn execute_sweep(env: &TickEnv<'_>, replies: bool, now: i64) -> TickOutcome {
    let pid = env.profile_id;
    let handle = {
        let db = env.db.lock().unwrap();
        let _ = pacing::record_units(&db, pid, now, pacing::DOM_UNITS, UnitKind::Dom);
        ingest::profile_handle(&db, pid).ok().flatten()
    };
    let Some(handle) = handle else { return TickOutcome::Idle };
    let url = if replies { format!("https://x.com/{handle}/with_replies") } else { format!("https://x.com/{handle}") };
    let req = AnalyticsPageRequest {
        url,
        network_url_filters: vec![],
        dom_script: PROFILE_TIMELINE_DOM_JS.to_string(),
        settle_ms: SETTLE_PROFILE_MS,
        ..Default::default()
    };

    let finish = |ok: bool| {
        let db = env.db.lock().unwrap();
        let _ = repo::set_setting(&db, SWEEP_STAGE_KEY, if ok && !replies { "replies" } else { "" });
        if !ok || replies {
            let at = if ok { now } else { now - pacing::SWEEP_INTERVAL_SECS + 3600 };
            let _ = repo::set_setting(&db, LAST_SWEEP_KEY, &at.to_string());
        }
    };

    let cap = match env.fetcher.fetch_page(req).await {
        Ok(c) => c,
        Err(_) => {
            finish(false);
            tick_result(env, false, now);
            return TickOutcome::Failed;
        }
    };
    if let Some(reason) = block_reason(&cap) {
        return blocked(env, &reason);
    }
    let timeline = super::extract::parse_profile_timeline(&cap.dom_result);
    if timeline.posts.is_empty() && timeline.followers_total.is_none() {
        finish(false);
        tick_result(env, false, now);
        return TickOutcome::Failed;
    }
    let stored = {
        let db = env.db.lock().unwrap();
        ingest::ingest_profile_page(&db, pid, &cap.dom_result, now, PROFILE_WINDOW_DAYS).unwrap_or(0)
    };
    finish(true);
    tick_result(env, true, now);
    if stored > 0 {
        updated(env, "profile");
    }
    TickOutcome::Synced
}

/// Starts the loop. Idle ticks cost nothing; the browser is only started when there is work.
pub fn spawn(app: &AppHandle) {
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(STARTUP_GRACE_SECS)).await;
        loop {
            let delay = tick_once(&handle).await;
            tokio::time::sleep(Duration::from_secs(delay)).await;
        }
    });
}

async fn tick_once(app: &AppHandle) -> u64 {
    let state = app.state::<AppState>();
    let p3 = app.state::<Phase3>();
    if p3.kill.is_engaged() {
        return pacing::PAUSED_POLL_SECS;
    }
    if p3.publish.pending() > 0 {
        return BUSY_POLL_SECS;
    }
    let now = now_unix_secs();
    let planned = {
        let db = state.db.lock().unwrap();
        plan(&db, &state.profile_id, now)
    };
    let entropy = crate::util::new_id().bytes().fold(0u64, |a, b| a.wrapping_mul(31).wrapping_add(b as u64));
    let outcome = match planned {
        Ok(Plan::Work(task)) => {
            if crate::commands::ensure_publish_browser_ready(&state).await.is_err() {
                return pacing::PAUSED_POLL_SECS;
            }
            let sink = TauriSink(app.clone());
            let healer = LiveHealer { browser: state.publish_browser.clone(), provider: p3.completion(), kill: p3.kill.clone() };
            let env = TickEnv { db: &state.db, profile_id: &state.profile_id, fetcher: &*state.publish_browser, healer: Some(&healer), sink: &sink };
            execute(&env, task, now).await
        }
        Ok(Plan::Paused) => TickOutcome::Paused,
        Ok(Plan::Throttled) => TickOutcome::Throttled,
        Ok(Plan::Idle) | Err(_) => TickOutcome::Idle,
    };
    next_delay_secs(&outcome, entropy)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::BrowserError;
    use crate::control::CollectSink;
    use crate::storage::{open, repo as srepo};
    use crate::util::format_unix;
    use rusqlite::params;
    use std::collections::{HashMap, VecDeque};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tempfile::tempdir;

    struct Fake {
        by_url: Mutex<HashMap<String, VecDeque<Result<serde_json::Value, String>>>>,
        urls: Mutex<Vec<String>>,
        final_url: Mutex<Option<String>>,
    }

    impl Fake {
        fn new() -> Self {
            Self { by_url: Mutex::default(), urls: Mutex::default(), final_url: Mutex::default() }
        }
        fn on(&self, url: &str, r: Result<serde_json::Value, String>) {
            self.by_url.lock().unwrap().entry(url.to_string()).or_default().push_back(r);
        }
        fn calls(&self) -> Vec<String> {
            self.urls.lock().unwrap().clone()
        }
    }

    #[async_trait::async_trait]
    impl AnalyticsPageFetcher for Fake {
        async fn fetch_page(&self, req: AnalyticsPageRequest) -> Result<AnalyticsPageCapture, BrowserError> {
            self.urls.lock().unwrap().push(req.url.clone());
            assert!(req.network_url_filters.is_empty(), "no GraphQL capture is requested");
            let next = self.by_url.lock().unwrap().get_mut(&req.url).and_then(|q| q.pop_front());
            match next {
                Some(Ok(dom)) => Ok(AnalyticsPageCapture {
                    final_url: self.final_url.lock().unwrap().clone().unwrap_or(req.url),
                    used_dedicated_tab: true,
                    network_responses: vec![],
                    dom_result: dom,
                }),
                Some(Err(e)) => Err(BrowserError::Other(e)),
                None => Err(BrowserError::Other("no scripted page".into())),
            }
        }
    }

    struct Healer {
        calls: AtomicUsize,
        result: Mutex<Result<(i64, i64), String>>,
    }

    #[async_trait::async_trait]
    impl PostHealer for Healer {
        fn available(&self) -> bool {
            true
        }
        async fn heal(&self, _url: &str, post_id: &str) -> Result<super::super::extract::PostMetrics, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.result.lock().unwrap().clone().map(|(imp, likes)| super::super::extract::PostMetrics {
                platform_post_id: post_id.into(),
                impressions: Some(imp),
                likes: Some(likes),
                source: "vision_heal".into(),
                raw_json: json!({}),
                ..Default::default()
            })
        }
    }

    struct World {
        _dir: tempfile::TempDir,
        db: Mutex<Connection>,
        pid: String,
    }

    fn world(post_ages_hours: &[(&str, i64)]) -> World {
        let dir = tempdir().unwrap();
        let (conn, _) = open(&dir.path().join("a.db"), &dir.path().join("b")).unwrap();
        let pid = srepo::ensure_default_profile(&conn, "/tmp/p").unwrap();
        let now = now_unix_secs();
        conn.execute(
            "INSERT INTO observed_posts (id, profile_id, platform_post_id, author_handle, author_followers, text, feed_source, posted_at, first_seen_at, metrics_at_capture_json)
             VALUES ('op1', ?1, 'src1', 'naval', 200000, 'x', 'for_you', ?2, ?2, '{}')",
            params![pid, format_unix(now)],
        )
        .unwrap();
        for (post, age) in post_ages_hours {
            let at = format_unix(now - age * 3600);
            conn.execute(
                "INSERT INTO reply_drafts (id, profile_id, observed_post_id, initial_text, edited_text, edit_distance, model_used, style_guide_version, draft_kind, actions_json, status, published_at, published_post_id, published_url, created_at)
                 VALUES (?3, ?1, 'op1', 'hi', 'text', 5, 'm', 1, 'reply', '[]', 'published', ?2, ?3, NULL, ?2)",
                params![pid, at, post],
            )
            .unwrap();
        }
        World { _dir: dir, db: Mutex::new(conn), pid }
    }

    fn set_handle(w: &World, handle: &str) {
        w.db.lock().unwrap().execute("UPDATE profiles SET handle = ?1 WHERE id = ?2", params![handle, w.pid]).unwrap();
    }

    fn seed_period_row(w: &World, days: i64) {
        w.db.lock().unwrap().execute(
            "INSERT OR IGNORE INTO analytics_period_snapshots (id, profile_id, period_days, period_label, captured_at, impressions, source, raw_json)
             VALUES (?1, ?2, ?3, ?4, '2026-01-01T00:00:00Z', 1000, 'dom', '{}')",
            params![format!("seed{days}"), w.pid, days, format!("{days}D")],
        )
        .unwrap();
    }

    fn silence_sweeps_and_periods(w: &World, now: i64) {
        seed_period_row(w, 7);
        seed_period_row(w, 30);
        let db = w.db.lock().unwrap();
        repo::set_setting(&db, LAST_SWEEP_KEY, &now.to_string()).unwrap();
        pacing::mark_period_synced(&db, 7, now).unwrap();
        pacing::mark_period_synced(&db, 30, now).unwrap();
    }

    fn page(likes: &str, views: &str) -> serde_json::Value {
        json!({ "state": "ok", "found": true, "gone": false, "handle": "pavel", "impressions": views, "likes": likes, "replies": "1", "bodySnippet": "" })
    }

    fn empty_page() -> serde_json::Value {
        json!({ "state": "ok", "found": true, "gone": false, "handle": "pavel", "bodySnippet": "" })
    }

    fn env<'a>(w: &'a World, f: &'a Fake, h: Option<&'a dyn PostHealer>, sink: &'a CollectSink) -> TickEnv<'a> {
        TickEnv { db: &w.db, profile_id: &w.pid, fetcher: f, healer: h, sink }
    }

    fn url(post: &str) -> String {
        format!("https://x.com/pavel/status/{post}")
    }

    fn snapshots(w: &World) -> i64 {
        w.db.lock().unwrap().query_row("SELECT COUNT(*) FROM analytics_snapshots WHERE scope = 'post'", [], |r| r.get(0)).unwrap()
    }

    #[test]
    fn delays_are_jittered_for_work_and_long_for_everything_else() {
        for e in [0, 7, 1_000_003, u64::MAX] {
            for o in [TickOutcome::Synced, TickOutcome::Healed, TickOutcome::Failed, TickOutcome::Missing] {
                let d = next_delay_secs(&o, e);
                assert!((pacing::JITTER_MIN_SECS..=pacing::JITTER_MAX_SECS).contains(&d), "{o:?} {d}");
            }
        }
        assert_eq!(next_delay_secs(&TickOutcome::Idle, 1), 300);
        assert_eq!(next_delay_secs(&TickOutcome::Throttled, 1), 600);
        assert_eq!(next_delay_secs(&TickOutcome::Paused, 1), 900);
        assert_eq!(next_delay_secs(&TickOutcome::Blocked("logged_out".into()), 1), 1800);
    }

    #[tokio::test]
    async fn one_post_per_tick_newest_first_and_the_ui_is_told() {
        let w = world(&[("111", 30), ("222", 2)]);
        set_handle(&w, "pavel");
        let f = Fake::new();
        f.on(&url("222"), Ok(page("12", "900")));
        f.on(&url("111"), Ok(page("30", "2000")));
        let sink = CollectSink::default();
        let now = now_unix_secs();
        silence_sweeps_and_periods(&w, now);

        assert_eq!(run_tick(&env(&w, &f, None, &sink), now).await, TickOutcome::Synced);
        assert_eq!(f.calls(), vec![url("222")], "the newest post first, and only one page per tick");
        assert_eq!(snapshots(&w), 1);
        assert_eq!(sink.0.lock().unwrap().iter().filter(|(n, _)| n == "analytics:updated").count(), 1);

        assert_eq!(run_tick(&env(&w, &f, None, &sink), now + 120).await, TickOutcome::Synced);
        assert_eq!(f.calls().len(), 2);
        assert_eq!(snapshots(&w), 2);

        assert_eq!(run_tick(&env(&w, &f, None, &sink), now + 240).await, TickOutcome::Idle, "both are scheduled for later checkpoints");
        assert_eq!(f.calls().len(), 2);
        assert!(health::load_status(&w.db.lock().unwrap()).last_update_at.is_some());
    }

    #[tokio::test]
    async fn the_hourly_budget_stops_work_and_the_next_hour_resumes_it() {
        let w = world(&[("111", 2)]);
        set_handle(&w, "pavel");
        let f = Fake::new();
        f.on(&url("111"), Ok(page("5", "500")));
        let sink = CollectSink::default();
        let now = now_unix_secs();
        silence_sweeps_and_periods(&w, now);
        {
            let db = w.db.lock().unwrap();
            pacing::record_units(&db, &w.pid, now - 60, pacing::HOURLY_UNIT_CAP, UnitKind::Dom).unwrap();
        }
        assert_eq!(run_tick(&env(&w, &f, None, &sink), now).await, TickOutcome::Throttled);
        assert!(f.calls().is_empty(), "no page is loaded while throttled");
        assert_eq!(run_tick(&env(&w, &f, None, &sink), now + 3700).await, TickOutcome::Synced);
    }

    #[tokio::test]
    async fn nothing_to_do_is_idle_and_loads_nothing() {
        let w = world(&[]);
        let f = Fake::new();
        let sink = CollectSink::default();
        assert_eq!(run_tick(&env(&w, &f, None, &sink), now_unix_secs()).await, TickOutcome::Idle);
        assert!(f.calls().is_empty());
    }

    #[tokio::test]
    async fn failures_are_quiet_back_off_and_three_in_a_row_pause_the_worker() {
        let w = world(&[("111", 2), ("222", 3), ("333", 4)]);
        set_handle(&w, "pavel");
        let f = Fake::new();
        let sink = CollectSink::default();
        let now = now_unix_secs();
        silence_sweeps_and_periods(&w, now);
        for i in 0..3 {
            assert_eq!(run_tick(&env(&w, &f, None, &sink), now + i * 120).await, TickOutcome::Failed);
        }
        assert!(sink.0.lock().unwrap().is_empty(), "nothing is announced");
        assert_eq!(snapshots(&w), 0);
        assert_eq!(run_tick(&env(&w, &f, None, &sink), now + 400).await, TickOutcome::Paused);
        assert_eq!(f.calls().len(), 3, "paused: no further loads");
        let st = pacing::load_state(&w.db.lock().unwrap(), &w.pid, "111").unwrap().unwrap();
        assert!(st.next_due_at >= now + pacing::BACKOFF_SECS[0], "the failed post waits before its next try");
        assert_eq!(health::load_status(&w.db.lock().unwrap()), health::SyncStatus::default(), "no scary status either");
    }

    #[tokio::test]
    async fn a_logged_out_page_blocks_calmly_without_counting_as_a_failure() {
        let w = world(&[("111", 2)]);
        set_handle(&w, "pavel");
        let f = Fake::new();
        f.on(&url("111"), Ok(json!({ "state": "logged_out", "found": false, "bodySnippet": "" })));
        let sink = CollectSink::default();
        let now = now_unix_secs();
        silence_sweeps_and_periods(&w, now);
        let out = run_tick(&env(&w, &f, None, &sink), now).await;
        assert_eq!(out, TickOutcome::Blocked("logged_out".into()));
        let db = w.db.lock().unwrap();
        assert_eq!(health::load_status(&db).blocked.as_deref(), Some("logged_out"));
        assert!(pacing::load_state(&db, &w.pid, "111").unwrap().is_none(), "the post is not penalised");
        assert!(pacing::pause_until(&db).is_none());
    }

    #[tokio::test]
    async fn a_deleted_post_is_retired_after_three_confirmations() {
        let w = world(&[("111", 2)]);
        set_handle(&w, "pavel");
        let f = Fake::new();
        for _ in 0..3 {
            f.on(&url("111"), Ok(json!({ "state": "ok", "found": false, "gone": true, "bodySnippet": "" })));
        }
        let sink = CollectSink::default();
        let now = now_unix_secs();
        silence_sweeps_and_periods(&w, now);
        let mut t = now;
        for _ in 0..3 {
            assert_eq!(run_tick(&env(&w, &f, None, &sink), t).await, TickOutcome::Missing);
            t += 2 * 86_400;
            silence_sweeps_and_periods(&w, t);
        }
        assert!(pacing::load_state(&w.db.lock().unwrap(), &w.pid, "111").unwrap().unwrap().retired);
        assert_eq!(run_tick(&env(&w, &f, None, &sink), t).await, TickOutcome::Idle);
    }

    #[tokio::test]
    async fn an_unreadable_page_is_healed_from_a_screenshot_once_then_the_limits_hold() {
        let w = world(&[("111", 2), ("222", 3)]);
        set_handle(&w, "pavel");
        let f = Fake::new();
        f.on(&url("111"), Ok(empty_page()));
        f.on(&url("222"), Ok(empty_page()));
        let healer = Healer { calls: AtomicUsize::new(0), result: Mutex::new(Ok((800, 20))) };
        let sink = CollectSink::default();
        let now = now_unix_secs();
        silence_sweeps_and_periods(&w, now);

        assert_eq!(run_tick(&env(&w, &f, Some(&healer), &sink), now).await, TickOutcome::Healed);
        assert_eq!(healer.calls.load(Ordering::SeqCst), 1);
        let src: String = w.db.lock().unwrap().query_row("SELECT json_extract(raw_json, '$.extraction_source') FROM analytics_snapshots WHERE platform_post_id = '111'", [], |r| r.get(0)).unwrap();
        assert_eq!(src, "vision_heal");

        assert_eq!(run_tick(&env(&w, &f, Some(&healer), &sink), now + 120).await, TickOutcome::Failed, "a second heal within the hour is refused");
        assert_eq!(healer.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_failed_heal_opens_the_breaker_and_is_never_shown() {
        let w = world(&[("111", 2), ("222", 3), ("333", 4)]);
        set_handle(&w, "pavel");
        let f = Fake::new();
        for p in ["111", "222", "333"] {
            f.on(&url(p), Ok(empty_page()));
        }
        let healer = Healer { calls: AtomicUsize::new(0), result: Mutex::new(Err("give up".into())) };
        let sink = CollectSink::default();
        let now = now_unix_secs();
        silence_sweeps_and_periods(&w, now);
        assert_eq!(run_tick(&env(&w, &f, Some(&healer), &sink), now).await, TickOutcome::Failed);
        pacing::retire(&w.db.lock().unwrap(), &w.pid, "111", now).unwrap();
        assert_eq!(run_tick(&env(&w, &f, Some(&healer), &sink), now + 3700).await, TickOutcome::Failed);
        pacing::retire(&w.db.lock().unwrap(), &w.pid, "222", now).unwrap();
        assert_eq!(healer.calls.load(Ordering::SeqCst), 2);
        let _ = run_tick(&env(&w, &f, Some(&healer), &sink), now + 7400).await;
        assert_eq!(healer.calls.load(Ordering::SeqCst), 2, "two failures in a row stop healing for hours");
        assert!(sink.0.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn profile_sweeps_run_every_few_hours_and_never_back_to_back_with_post_reads() {
        let w = world(&[("111", 2)]);
        set_handle(&w, "pavel");
        let f = Fake::new();
        let now = now_unix_secs();
        let recent = format_unix(now - 3600);
        let dom = json!({ "handle": "pavel", "followersText": "1,200 Followers", "posts": [
            { "id": "111", "text": "text", "postedAt": recent, "views": "700 Views", "likes": "20 Likes" }
        ]});
        f.on("https://x.com/pavel", Ok(dom.clone()));
        f.on("https://x.com/pavel/with_replies", Ok(json!({ "handle": "pavel", "followersText": "1,200 Followers", "posts": [] })));
        f.on(&url("111"), Ok(page("20", "700")));
        let sink = CollectSink::default();
        seed_period_row(&w, 7);
        seed_period_row(&w, 30);
        {
            let db = w.db.lock().unwrap();
            pacing::mark_period_synced(&db, 7, now).unwrap();
            pacing::mark_period_synced(&db, 30, now).unwrap();
        }

        assert_eq!(run_tick(&env(&w, &f, None, &sink), now).await, TickOutcome::Synced);
        assert_eq!(run_tick(&env(&w, &f, None, &sink), now + 120).await, TickOutcome::Synced);
        assert_eq!(f.calls(), vec!["https://x.com/pavel".to_string(), "https://x.com/pavel/with_replies".to_string()]);
        assert_eq!(snapshots(&w), 1, "the profile pass stored the tracked post");

        assert_eq!(run_tick(&env(&w, &f, None, &sink), now + 240).await, TickOutcome::Idle, "the post was just covered by the profile pass");
        {
            let db = w.db.lock().unwrap();
            pacing::mark_period_synced(&db, 7, now + 120 + pacing::SWEEP_INTERVAL_SECS).unwrap();
            pacing::mark_period_synced(&db, 30, now + 120 + pacing::SWEEP_INTERVAL_SECS).unwrap();
        }
        assert_eq!(run_tick(&env(&w, &f, None, &sink), now + 120 + pacing::SWEEP_INTERVAL_SECS + 10).await, TickOutcome::Failed, "next sweep after the interval (no scripted page left)");
        assert_eq!(f.calls().len(), 3);
    }

    #[tokio::test]
    async fn a_pause_in_the_settings_is_respected() {
        let w = world(&[("111", 2)]);
        set_handle(&w, "pavel");
        let f = Fake::new();
        let sink = CollectSink::default();
        let now = now_unix_secs();
        pacing::set_pause(&w.db.lock().unwrap(), now + 600).unwrap();
        assert_eq!(run_tick(&env(&w, &f, None, &sink), now).await, TickOutcome::Paused);
        assert!(f.calls().is_empty());
    }

    #[tokio::test]
    async fn syncs_creator_analytics_7d_and_30d_periods_then_continues_per_post_sync() {
        let w = world(&[("111", 2)]);
        set_handle(&w, "pavel");
        let now = now_unix_secs();
        {
            let db = w.db.lock().unwrap();
            repo::set_setting(&db, LAST_SWEEP_KEY, &now.to_string()).unwrap();
            pacing::set_preferred_window_days(&db, 7).unwrap();
        }

        let f = Fake::new();
        f.on(
            ACCOUNT_ANALYTICS_URL,
            Ok(json!({
                "state": "ok",
                "handle": "pavel",
                "selectedRangeLabel": "7D",
                "selectedRangeDays": 7,
                "labeledMetrics": {
                    "impressions": "31.5K",
                    "engagement rate": "4.9%",
                    "engagements": "1.5K",
                    "profile visits": "420",
                    "new follows": "29",
                    "unfollows": "4"
                },
                "metricCards": {},
                "bodySnippet": ""
            })),
        );
        f.on(
            ACCOUNT_ANALYTICS_URL,
            Ok(json!({
                "state": "ok",
                "handle": "pavel",
                "selectedRangeLabel": "4W",
                "selectedRangeDays": 28,
                "labeledMetrics": {
                    "impressions": "142.3K",
                    "engagement rate": "4.2%",
                    "engagements": "5.9K",
                    "profile visits": "1.8K",
                    "new follows": "160",
                    "unfollows": "33"
                },
                "metricCards": {},
                "bodySnippet": ""
            })),
        );
        f.on(&url("111"), Ok(page("15", "850")));

        let sink = CollectSink::default();

        // Tick 1: preferred 7d period snapshot
        assert_eq!(run_tick(&env(&w, &f, None, &sink), now).await, TickOutcome::Synced);
        // Tick 2: 30d period snapshot
        assert_eq!(run_tick(&env(&w, &f, None, &sink), now + 120).await, TickOutcome::Synced);
        // Tick 3: gradual per-post link sync continues seamlessly
        assert_eq!(run_tick(&env(&w, &f, None, &sink), now + 240).await, TickOutcome::Synced);
        // Tick 4: idle until next checkpoint / 4h period refresh
        assert_eq!(run_tick(&env(&w, &f, None, &sink), now + 360).await, TickOutcome::Idle);

        assert_eq!(
            f.calls(),
            vec![ACCOUNT_ANALYTICS_URL.to_string(), ACCOUNT_ANALYTICS_URL.to_string(), url("111")]
        );
        assert_eq!(snapshots(&w), 1);

        let db = w.db.lock().unwrap();
        let row7: (i64, f64, i64) = db
            .query_row(
                "SELECT impressions, engagement_rate, follower_delta FROM v_analytics_account_overview WHERE profile_id = ?1 AND period_days = 7",
                [&w.pid],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(row7, (31_500, 4.9, 25));

        let row30: (i64, f64, i64) = db
            .query_row(
                "SELECT impressions, engagement_rate, follower_delta FROM v_analytics_account_overview WHERE profile_id = ?1 AND period_days = 30",
                [&w.pid],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(row30, (142_300, 4.2, 127));
    }

    #[tokio::test]
    async fn period_range_mismatch_is_not_stored_and_upsell_cools_down_calmly() {
        let w = world(&[]);
        set_handle(&w, "pavel");
        let now = now_unix_secs();
        {
            let db = w.db.lock().unwrap();
            repo::set_setting(&db, LAST_SWEEP_KEY, &now.to_string()).unwrap();
            pacing::set_preferred_window_days(&db, 7).unwrap();
        }

        // Page stayed on 4W (28d) even though worker requested 7d -> must not overwrite 7d snapshot!
        let f = Fake::new();
        f.on(
            ACCOUNT_ANALYTICS_URL,
            Ok(json!({
                "state": "ok",
                "selectedRangeLabel": "4W",
                "selectedRangeDays": 28,
                "labeledMetrics": { "impressions": "142.3K" },
                "metricCards": {},
                "bodySnippet": ""
            })),
        );
        // Next tick (30d) sees Premium upsell -> calm 24h cooldown for both periods, no failure streak
        f.on(
            ACCOUNT_ANALYTICS_URL,
            Ok(json!({
                "state": "ok",
                "isPremiumUpsell": true,
                "metricCards": {},
                "labeledMetrics": {},
                "bodySnippet": "Subscribe to X Premium to unlock analytics"
            })),
        );

        let sink = CollectSink::default();
        assert_eq!(run_tick(&env(&w, &f, None, &sink), now).await, TickOutcome::Failed);
        let stored_7d: i64 = w
            .db
            .lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM analytics_period_snapshots WHERE period_days = 7", [], |r| r.get(0))
            .unwrap();
        assert_eq!(stored_7d, 0, "mismatched 28d capture must not be stored under 7d");

        assert_eq!(run_tick(&env(&w, &f, None, &sink), now + 120).await, TickOutcome::Synced);
        assert_eq!(run_tick(&env(&w, &f, None, &sink), now + 240).await, TickOutcome::Idle, "upsell cooled down both 7d and 30d");
        assert!(pacing::pause_until(&w.db.lock().unwrap()).is_none());
    }

    #[tokio::test]
    async fn empty_account_period_scrape_falls_back_to_vision_heal() {
        struct PeriodHealer;
        #[async_trait::async_trait]
        impl PostHealer for PeriodHealer {
            fn available(&self) -> bool {
                true
            }
            async fn heal(&self, _url: &str, _post_id: &str) -> Result<super::super::extract::PostMetrics, String> {
                Err("unused".into())
            }
            async fn heal_account_period(&self, _url: &str, period_days: u32) -> Result<super::super::extract::AccountMetrics, String> {
                Ok(super::super::extract::AccountMetrics {
                    period_days: Some(period_days),
                    period_label: Some("30D".into()),
                    impressions: Some(88_000),
                    engagements: Some(3_520),
                    engagement_rate: Some(4.0),
                    profile_visits: Some(950),
                    follower_delta: Some(64),
                    source: "vision_heal".into(),
                    raw_json: json!({ "capture_source": "vision_heal" }),
                    ..Default::default()
                })
            }
        }

        let w = world(&[]);
        set_handle(&w, "pavel");
        let now = now_unix_secs();
        seed_period_row(&w, 7);
        {
            let db = w.db.lock().unwrap();
            repo::set_setting(&db, LAST_SWEEP_KEY, &now.to_string()).unwrap();
            pacing::mark_period_synced(&db, 7, now).unwrap();
        }

        let f = Fake::new();
        f.on(
            ACCOUNT_ANALYTICS_URL,
            Ok(json!({ "state": "ok", "metricCards": {}, "labeledMetrics": {}, "bodySnippet": "" })),
        );
        let sink = CollectSink::default();
        let healer = PeriodHealer;

        assert_eq!(run_tick(&env(&w, &f, Some(&healer), &sink), now).await, TickOutcome::Healed);
        let (imp, src): (i64, String) = w
            .db
            .lock()
            .unwrap()
            .query_row(
                "SELECT impressions, source FROM v_analytics_account_overview WHERE profile_id = ?1 AND period_days = 30",
                [&w.pid],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((imp, src.as_str()), (88_000, "vision_heal"));
    }

    fn period_page(range_days: i64) -> serde_json::Value {
        json!({
            "state": "ok", "handle": "pavel", "selectedRangeDays": range_days,
            "labeledMetrics": { "impressions": "31.5K", "engagements": "1.5K", "profile visits": "420" },
            "metricCards": {}, "bodySnippet": ""
        })
    }

    #[tokio::test]
    async fn a_full_post_budget_cannot_starve_period_captures() {
        let w = world(&[("111", 2)]);
        set_handle(&w, "pavel");
        let f = Fake::new();
        f.on(ACCOUNT_ANALYTICS_URL, Ok(period_page(7)));
        f.on(&url("111"), Ok(page("5", "500")));
        let sink = CollectSink::default();
        let now = now_unix_secs();
        silence_sweeps_and_periods(&w, now);
        {
            let db = w.db.lock().unwrap();
            for i in 0..36 {
                pacing::record_units(&db, &w.pid, now - 7200 - i * 60, pacing::DOM_UNITS, UnitKind::Dom).unwrap();
            }
        }
        assert_eq!(run_tick(&env(&w, &f, None, &sink), now).await, TickOutcome::Throttled, "posts stop at 36 of 40 units");
        assert!(f.calls().is_empty());

        pacing::set_preferred_window_days(&w.db.lock().unwrap(), 7).unwrap();
        w.db.lock().unwrap().execute("DELETE FROM app_settings WHERE key = 'analytics_period_7d_next_due_at'", []).unwrap();
        assert_eq!(run_tick(&env(&w, &f, None, &sink), now + 60).await, TickOutcome::Synced);
        assert_eq!(f.calls(), vec![ACCOUNT_ANALYTICS_URL.to_string()], "the reserved units pay for the period capture");
        assert_eq!(pacing::period_units_spent(&w.db.lock().unwrap(), 7, now + 60), 1);
    }

    #[tokio::test]
    async fn a_window_with_no_row_is_captured_at_once_even_after_a_failure_and_before_posts() {
        let w = world(&[("111", 2)]);
        set_handle(&w, "pavel");
        let f = Fake::new();
        f.on(ACCOUNT_ANALYTICS_URL, Err("first attempt failed".into()));
        f.on(ACCOUNT_ANALYTICS_URL, Ok(period_page(7)));
        f.on(ACCOUNT_ANALYTICS_URL, Ok(period_page(30)));
        f.on(&url("111"), Ok(page("5", "500")));
        let sink = CollectSink::default();
        let now = now_unix_secs();
        repo::set_setting(&w.db.lock().unwrap(), LAST_SWEEP_KEY, &now.to_string()).unwrap();
        pacing::set_preferred_window_days(&w.db.lock().unwrap(), 7).unwrap();

        assert_eq!(run_tick(&env(&w, &f, None, &sink), now).await, TickOutcome::Failed);
        assert!(pacing::period_next_due_at(&w.db.lock().unwrap(), 7) >= now + pacing::BACKOFF_SECS[0], "the 15 minute backoff is set");
        assert_eq!(
            run_tick(&env(&w, &f, None, &sink), now + 120).await,
            TickOutcome::Synced,
            "7d has no data, so it is retried right away"
        );
        assert_eq!(run_tick(&env(&w, &f, None, &sink), now + 240).await, TickOutcome::Synced, "30d has no data either");
        assert_eq!(run_tick(&env(&w, &f, None, &sink), now + 360).await, TickOutcome::Synced, "only then the post");
        assert_eq!(f.calls().last(), Some(&url("111")));
        assert_eq!(f.calls().iter().filter(|u| u.as_str() == ACCOUNT_ANALYTICS_URL).count(), 3);
        assert_eq!(pacing::period_last_error(&w.db.lock().unwrap(), 7), None, "a success clears the stored error");
    }

    #[tokio::test]
    async fn the_premium_upsell_is_not_retried_every_tick_for_a_window_with_no_row() {
        let w = world(&[]);
        set_handle(&w, "pavel");
        let f = Fake::new();
        f.on(
            ACCOUNT_ANALYTICS_URL,
            Ok(json!({ "state": "ok", "isPremiumUpsell": true, "metricCards": {}, "labeledMetrics": {}, "bodySnippet": "" })),
        );
        let sink = CollectSink::default();
        let now = now_unix_secs();
        repo::set_setting(&w.db.lock().unwrap(), LAST_SWEEP_KEY, &now.to_string()).unwrap();
        assert_eq!(run_tick(&env(&w, &f, None, &sink), now).await, TickOutcome::Synced);
        assert_eq!(run_tick(&env(&w, &f, None, &sink), now + 120).await, TickOutcome::Idle);
        assert_eq!(f.calls().len(), 1);
    }

    struct CaptureLogger;
    static CAPTURED: Mutex<Vec<String>> = Mutex::new(Vec::new());

    impl log::Log for CaptureLogger {
        fn enabled(&self, m: &log::Metadata) -> bool {
            m.level() <= log::Level::Info
        }
        fn log(&self, r: &log::Record) {
            CAPTURED.lock().unwrap().push(format!("{} {} {}", r.level(), r.target(), r.args()));
        }
        fn flush(&self) {}
    }

    fn captured_with(marker: &str) -> Vec<String> {
        CAPTURED.lock().unwrap().iter().filter(|l| l.contains(marker)).cloned().collect()
    }

    fn capture_logs() {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            let _ = log::set_logger(&CaptureLogger);
            log::set_max_level(log::LevelFilter::Info);
        });
    }

    #[tokio::test]
    async fn a_failed_period_capture_logs_its_step_and_reason_and_keeps_it_in_settings() {
        capture_logs();
        let w = world(&[]);
        set_handle(&w, "pavel");
        let f = Fake::new();
        f.on(ACCOUNT_ANALYTICS_URL, Err("boom-reason-7d-open".into()));
        let sink = CollectSink::default();
        let now = now_unix_secs();
        pacing::set_preferred_window_days(&w.db.lock().unwrap(), 7).unwrap();

        assert_eq!(run_tick(&env(&w, &f, None, &sink), now).await, TickOutcome::Failed);
        let lines = captured_with("boom-reason-7d-open");
        assert_eq!(lines.len(), 1, "{lines:?}");
        for part in ["WARN", "analytics::period", "window=7d", "step=page_open", "outcome=fail", "reason=", "boom-reason-7d-open"] {
            assert!(lines[0].contains(part), "{part} missing from {}", lines[0]);
        }
        let stored = repo::get_setting(&w.db.lock().unwrap(), "analytics_period_7d_last_error").unwrap().unwrap();
        assert!(stored.starts_with("page_open: ") && stored.contains("boom-reason-7d-open"), "{stored}");
        assert_eq!(pacing::period_last_error(&w.db.lock().unwrap(), 30), None);
    }

    #[tokio::test]
    async fn every_period_step_is_logged_and_each_failure_names_its_step() {
        capture_logs();
        let w = world(&[]);
        set_handle(&w, "pavel");
        let f = Fake::new();
        f.on(ACCOUNT_ANALYTICS_URL, Ok(period_page(28)));
        f.on(ACCOUNT_ANALYTICS_URL, Ok(json!({ "state": "ok", "selectedRangeDays": 7, "metricCards": {}, "labeledMetrics": {}, "bodySnippet": "step-marker-empty" })));
        f.on(ACCOUNT_ANALYTICS_URL, Ok(period_page(7)));
        let sink = CollectSink::default();
        let now = now_unix_secs();
        repo::set_setting(&w.db.lock().unwrap(), LAST_SWEEP_KEY, &now.to_string()).unwrap();
        pacing::set_preferred_window_days(&w.db.lock().unwrap(), 7).unwrap();
        let db_error = |days: u32| pacing::period_last_error(&w.db.lock().unwrap(), days).unwrap_or_default();

        assert_eq!(run_tick(&env(&w, &f, None, &sink), now).await, TickOutcome::Failed);
        assert!(db_error(7).starts_with("range_click: page still shows 28d, wanted 7d"), "{}", db_error(7));

        assert_eq!(run_tick(&env(&w, &f, None, &sink), now + 120).await, TickOutcome::Failed);
        assert!(db_error(7).starts_with("dom_read: no analytics numbers"), "{}", db_error(7));

        assert_eq!(run_tick(&env(&w, &f, None, &sink), now + 240).await, TickOutcome::Synced);
        assert_eq!(db_error(7), "");
        let ok_lines: Vec<String> = CAPTURED
            .lock()
            .unwrap()
            .iter()
            .filter(|l| l.starts_with("INFO analytics::period") && l.contains("window=7d") && l.contains("outcome=ok"))
            .cloned()
            .collect();
        for step in ["page_open", "range_click", "dom_read", "validate", "store"] {
            assert!(ok_lines.iter().any(|l| l.contains(&format!("step={step} "))), "no ok line for {step}");
        }
    }
}
