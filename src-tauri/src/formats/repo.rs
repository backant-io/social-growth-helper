//! SQLite access for viral formats, viral candidates and the creator watchlist.

use super::blueprint::{FormatBlueprint, Slot};
use super::detect::{PostMetrics, ViralThresholds};
use crate::error::{AppError, AppResult};
use crate::scout::model::MediaItem;
use crate::util::{new_id, now_rfc3339};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

const THRESHOLDS_KEY: &str = "viral_thresholds";
const MAX_EXAMPLES: usize = 5;
const MAX_SOURCES: usize = 20;
const RISING_WINDOW_SECS: i64 = 14 * 86_400;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FormatSource {
    pub post_id: String,
    pub author: String,
    pub url: String,
    pub seen_at: String,
    pub score: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct FormatRecord {
    pub id: String,
    pub name: String,
    pub archetype: String,
    pub hook_formula: String,
    pub structural_pattern: String,
    pub skeleton: String,
    pub slots: Vec<Slot>,
    pub media_hint: String,
    pub observation_count: i64,
    /// Mean over-performance of the observed example posts, 0..1.
    pub observed_win_rate: Option<f64>,
    pub personal_win_rate: Option<f64>,
    pub status: String,
    pub origin: String,
    pub sources: Vec<FormatSource>,
    pub example_texts: Vec<String>,
    /// Sightings in the last 14 days, used for the "Rising" sort.
    pub recent_sightings: i64,
    pub detected_at: String,
    pub updated_at: String,
}

impl FormatRecord {
    pub fn blueprint(&self) -> FormatBlueprint {
        FormatBlueprint {
            name: self.name.clone(),
            archetype: self.archetype.clone(),
            hook_formula: self.hook_formula.clone(),
            structural_pattern: self.structural_pattern.clone(),
            skeleton: self.skeleton.clone(),
            slots: self.slots.clone(),
            media_hint: self.media_hint.clone(),
        }
    }
}

const FORMAT_COLS: &str = "id, name, archetype, hook_formula, structural_pattern, skeleton, slot_schema_json, media_hint, \
observation_count, observed_win_rate, personal_win_rate, status, origin, sources_json, example_texts_json, detected_at, updated_at";

fn parse_secs(ts: &str) -> i64 {
    // `YYYY-MM-DDTHH:MM:SSZ`; only used to compare against "now" for the rising window.
    let p = |a: usize, b: usize| ts.get(a..b).and_then(|s| s.parse::<i64>().ok());
    let (Some(y), Some(mo), Some(d), Some(h), Some(mi), Some(s)) = (p(0, 4), p(5, 7), p(8, 10), p(11, 13), p(14, 16), p(17, 19)) else {
        return 0;
    };
    let (y2, m2) = if mo <= 2 { (y - 1, mo + 9) } else { (y, mo - 3) };
    let era = y2.div_euclid(400);
    let yoe = y2.rem_euclid(400);
    let doy = (153 * m2 + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    days * 86_400 + h * 3600 + mi * 60 + s
}

fn now_secs() -> i64 {
    parse_secs(&now_rfc3339())
}

fn row_to_format(r: &Row) -> rusqlite::Result<FormatRecord> {
    let slots_json: String = r.get(6)?;
    let sources_json: String = r.get(13)?;
    let examples_json: String = r.get(14)?;
    let sources: Vec<FormatSource> = serde_json::from_str(&sources_json).unwrap_or_default();
    let cutoff = now_secs() - RISING_WINDOW_SECS;
    let recent = sources.iter().filter(|s| parse_secs(&s.seen_at) >= cutoff).count() as i64;
    Ok(FormatRecord {
        id: r.get(0)?,
        name: r.get(1)?,
        archetype: r.get(2)?,
        hook_formula: r.get(3)?,
        structural_pattern: r.get(4)?,
        skeleton: r.get(5)?,
        slots: serde_json::from_str(&slots_json).unwrap_or_default(),
        media_hint: r.get(7)?,
        observation_count: r.get(8)?,
        observed_win_rate: r.get(9)?,
        personal_win_rate: r.get(10)?,
        status: r.get(11)?,
        origin: r.get(12)?,
        sources,
        example_texts: serde_json::from_str(&examples_json).unwrap_or_default(),
        recent_sightings: recent,
        detected_at: r.get(15)?,
        updated_at: r.get(16)?,
    })
}

pub fn insert_format(
    conn: &Connection,
    profile_id: &str,
    bp: &FormatBlueprint,
    origin: &str,
    source: Option<FormatSource>,
    example_text: Option<&str>,
) -> AppResult<FormatRecord> {
    let id = new_id();
    let now = now_rfc3339();
    let score = source.as_ref().map(|s| s.score / 100.0);
    let post_ids: Vec<&str> = source.iter().map(|s| s.post_id.as_str()).collect();
    let sources: Vec<&FormatSource> = source.iter().collect();
    let examples: Vec<&str> = example_text.into_iter().collect();
    conn.execute(
        "INSERT INTO viral_formats (id, profile_id, name, archetype, structural_pattern, hook_formula, slot_schema_json, \
         example_post_ids_json, observed_win_rate, personal_win_rate, status, detected_at, updated_at, skeleton, media_hint, \
         observation_count, sources_json, example_texts_json, origin) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,NULL,'active',?10,?10,?11,?12,?13,?14,?15,?16)",
        params![
            id,
            profile_id,
            bp.name,
            bp.archetype,
            bp.structural_pattern,
            bp.hook_formula,
            serde_json::to_string(&bp.slots).unwrap(),
            serde_json::to_string(&post_ids).unwrap(),
            score,
            now,
            bp.skeleton,
            bp.media_hint,
            if source.is_some() { 1 } else { 0 },
            serde_json::to_string(&sources).unwrap(),
            serde_json::to_string(&examples).unwrap(),
            origin,
        ],
    )?;
    get_format(conn, profile_id, &id)?.ok_or_else(|| AppError::Other("format vanished after insert".into()))
}

pub fn get_format(conn: &Connection, profile_id: &str, id: &str) -> AppResult<Option<FormatRecord>> {
    Ok(conn
        .query_row(
            &format!("SELECT {FORMAT_COLS} FROM viral_formats WHERE id=?1 AND profile_id=?2"),
            params![id, profile_id],
            row_to_format,
        )
        .optional()?)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FormatSort {
    /// Formats that worked for you first, then the strongest on X.
    Working,
    /// Most sightings in the last two weeks first.
    Rising,
    Recent,
}

pub fn list_formats(conn: &Connection, profile_id: &str, include_archived: bool, sort: FormatSort) -> AppResult<Vec<FormatRecord>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {FORMAT_COLS} FROM viral_formats WHERE profile_id=?1 AND (?2 OR status='active') ORDER BY updated_at DESC"
    ))?;
    let mut rows: Vec<FormatRecord> = stmt
        .query_map(params![profile_id, include_archived], row_to_format)?
        .collect::<Result<_, _>>()?;
    let cmp = |a: Option<f64>, b: Option<f64>| b.unwrap_or(-1.0).partial_cmp(&a.unwrap_or(-1.0)).unwrap_or(std::cmp::Ordering::Equal);
    match sort {
        FormatSort::Working => rows.sort_by(|a, b| {
            cmp(a.personal_win_rate, b.personal_win_rate)
                .then(cmp(a.observed_win_rate, b.observed_win_rate))
                .then(b.observation_count.cmp(&a.observation_count))
        }),
        FormatSort::Rising => rows.sort_by(|a, b| b.recent_sightings.cmp(&a.recent_sightings).then(b.updated_at.cmp(&a.updated_at))),
        FormatSort::Recent => {}
    }
    Ok(rows)
}

/// Records another sighting of an existing format (OGC-04, OGC-14).
pub fn merge_sighting(conn: &Connection, profile_id: &str, id: &str, source: FormatSource, example_text: &str) -> AppResult<FormatRecord> {
    let f = get_format(conn, profile_id, id)?.ok_or_else(|| AppError::Validation("format not found".into()))?;
    let mut sources = f.sources.clone();
    if !sources.iter().any(|s| s.post_id == source.post_id) {
        sources.push(source.clone());
        sources.sort_by(|a, b| b.seen_at.cmp(&a.seen_at));
        sources.truncate(MAX_SOURCES);
    }
    let mut examples = f.example_texts.clone();
    if examples.len() < MAX_EXAMPLES && !examples.iter().any(|e| e == example_text) {
        examples.push(example_text.to_string());
    }
    let n = f.observation_count as f64;
    let rate = match f.observed_win_rate {
        Some(old) => (old * n + source.score / 100.0) / (n + 1.0),
        None => source.score / 100.0,
    };
    let post_ids: Vec<&str> = sources.iter().map(|s| s.post_id.as_str()).collect();
    conn.execute(
        "UPDATE viral_formats SET observation_count=observation_count+1, observed_win_rate=?1, sources_json=?2, \
         example_texts_json=?3, example_post_ids_json=?4, updated_at=?5 WHERE id=?6 AND profile_id=?7",
        params![
            rate,
            serde_json::to_string(&sources).unwrap(),
            serde_json::to_string(&examples).unwrap(),
            serde_json::to_string(&post_ids).unwrap(),
            now_rfc3339(),
            id,
            profile_id
        ],
    )?;
    get_format(conn, profile_id, id)?.ok_or_else(|| AppError::Other("format vanished after merge".into()))
}

pub fn update_format(conn: &Connection, profile_id: &str, id: &str, bp: &FormatBlueprint) -> AppResult<FormatRecord> {
    let changed = conn.execute(
        "UPDATE viral_formats SET name=?1, archetype=?2, hook_formula=?3, structural_pattern=?4, skeleton=?5, \
         slot_schema_json=?6, media_hint=?7, updated_at=?8 WHERE id=?9 AND profile_id=?10",
        params![
            bp.name,
            bp.archetype,
            bp.hook_formula,
            bp.structural_pattern,
            bp.skeleton,
            serde_json::to_string(&bp.slots).unwrap(),
            bp.media_hint,
            now_rfc3339(),
            id,
            profile_id
        ],
    )?;
    if changed == 0 {
        return Err(AppError::Validation("format not found".into()));
    }
    get_format(conn, profile_id, id)?.ok_or_else(|| AppError::Other("format vanished after update".into()))
}

pub fn set_format_status(conn: &Connection, profile_id: &str, id: &str, status: &str) -> AppResult<()> {
    if !matches!(status, "active" | "archived") {
        return Err(AppError::Validation("status must be active or archived".into()));
    }
    let n = conn.execute(
        "UPDATE viral_formats SET status=?1, updated_at=?2 WHERE id=?3 AND profile_id=?4",
        params![status, now_rfc3339(), id, profile_id],
    )?;
    if n == 0 {
        return Err(AppError::Validation("format not found".into()));
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct Candidate {
    pub id: String,
    pub platform_post_id: String,
    pub author_handle: String,
    pub author_followers: Option<u64>,
    pub text: String,
    pub metrics: PostMetrics,
    pub posted_at: Option<String>,
    pub source: String,
    pub virality_score: f64,
    pub reasons: Vec<String>,
    pub status: String,
    pub format_id: Option<String>,
    pub error_message: Option<String>,
    pub created_at: String,
    pub author_name: String,
    pub avatar_url: Option<String>,
    pub media: Vec<MediaItem>,
}

impl Candidate {
    pub fn url(&self) -> String {
        format!("https://x.com/{}/status/{}", self.author_handle, self.platform_post_id)
    }
}

#[derive(Debug, Clone)]
pub struct NewCandidate {
    pub platform_post_id: String,
    pub author_handle: String,
    pub author_followers: Option<u64>,
    pub text: String,
    pub metrics: PostMetrics,
    pub posted_at: Option<String>,
    pub source: &'static str,
    pub virality_score: f64,
    pub reasons: Vec<String>,
    pub author_name: String,
    pub avatar_url: Option<String>,
    pub media: Vec<MediaItem>,
}

const CAND_COLS: &str = "id, platform_post_id, author_handle, author_followers, text, metrics_json, posted_at, source, \
virality_score, reasons_json, status, format_id, error_message, created_at, author_name, author_avatar_url, media_json";

fn row_to_candidate(r: &Row) -> rusqlite::Result<Candidate> {
    let metrics: String = r.get(5)?;
    let reasons: String = r.get(9)?;
    Ok(Candidate {
        id: r.get(0)?,
        platform_post_id: r.get(1)?,
        author_handle: r.get(2)?,
        author_followers: r.get::<_, Option<i64>>(3)?.map(|v| v.max(0) as u64),
        text: r.get(4)?,
        metrics: serde_json::from_str(&metrics).unwrap_or_default(),
        posted_at: r.get(6)?,
        source: r.get(7)?,
        virality_score: r.get(8)?,
        reasons: serde_json::from_str(&reasons).unwrap_or_default(),
        status: r.get(10)?,
        format_id: r.get(11)?,
        error_message: r.get(12)?,
        created_at: r.get(13)?,
        author_name: r.get::<_, Option<String>>(14)?.unwrap_or_default(),
        avatar_url: r.get(15)?,
        media: serde_json::from_str(&r.get::<_, Option<String>>(16)?.unwrap_or_else(|| "[]".into())).unwrap_or_default(),
    })
}

/// Stores a candidate; returns `None` when the same post was already seen.
pub fn insert_candidate(conn: &Connection, profile_id: &str, c: &NewCandidate) -> AppResult<Option<Candidate>> {
    let id = new_id();
    let n = conn.execute(
        "INSERT OR IGNORE INTO format_candidates (id, profile_id, platform_post_id, author_handle, author_followers, text, \
         metrics_json, posted_at, source, virality_score, reasons_json, created_at, author_name, author_avatar_url, media_json) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
        params![
            id,
            profile_id,
            c.platform_post_id,
            c.author_handle,
            c.author_followers.map(|v| v as i64),
            c.text,
            serde_json::to_string(&c.metrics).unwrap(),
            c.posted_at,
            c.source,
            c.virality_score,
            serde_json::to_string(&c.reasons).unwrap(),
            now_rfc3339(),
            c.author_name,
            c.avatar_url,
            serde_json::to_string(&c.media).unwrap_or_else(|_| "[]".into())
        ],
    )?;
    if n == 0 {
        return Ok(None);
    }
    get_candidate(conn, profile_id, &id)
}

pub fn get_candidate(conn: &Connection, profile_id: &str, id: &str) -> AppResult<Option<Candidate>> {
    Ok(conn
        .query_row(
            &format!("SELECT {CAND_COLS} FROM format_candidates WHERE id=?1 AND profile_id=?2"),
            params![id, profile_id],
            row_to_candidate,
        )
        .optional()?)
}

pub fn list_candidates(conn: &Connection, profile_id: &str, status: &str, limit: usize) -> AppResult<Vec<Candidate>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {CAND_COLS} FROM format_candidates WHERE profile_id=?1 AND status=?2 ORDER BY virality_score DESC, created_at DESC LIMIT ?3"
    ))?;
    let rows = stmt.query_map(params![profile_id, status, limit as i64], row_to_candidate)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn set_candidate_status(
    conn: &Connection,
    profile_id: &str,
    id: &str,
    status: &str,
    format_id: Option<&str>,
    error: Option<&str>,
) -> AppResult<()> {
    let n = conn.execute(
        "UPDATE format_candidates SET status=?1, format_id=?2, error_message=?3 WHERE id=?4 AND profile_id=?5",
        params![status, format_id, error, id, profile_id],
    )?;
    if n == 0 {
        return Err(AppError::Validation("candidate not found".into()));
    }
    Ok(())
}

pub fn get_thresholds(conn: &Connection) -> AppResult<ViralThresholds> {
    let raw = crate::storage::repo::get_setting(conn, THRESHOLDS_KEY)?;
    Ok(raw.and_then(|r| serde_json::from_str(&r).ok()).unwrap_or_default())
}

pub fn set_thresholds(conn: &Connection, t: &ViralThresholds) -> AppResult<()> {
    t.validate().map_err(AppError::Validation)?;
    crate::storage::repo::set_setting(conn, THRESHOLDS_KEY, &serde_json::to_string(t).unwrap())
}

#[derive(Debug, Clone, Serialize)]
pub struct WatchEntry {
    pub id: String,
    pub handle: String,
    pub followers: Option<u64>,
    pub added_at: String,
    pub last_scanned_at: Option<String>,
}

pub fn normalize_handle(raw: &str) -> AppResult<String> {
    let h = raw.trim().trim_start_matches('@').trim().to_string();
    let ok = !h.is_empty() && h.len() <= 15 && h.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !ok {
        return Err(AppError::Validation(format!("\"{raw}\" is not a valid X handle (1-15 letters, digits or underscores)")));
    }
    Ok(h)
}

pub fn add_watch(conn: &Connection, profile_id: &str, handle: &str, followers: Option<u64>) -> AppResult<WatchEntry> {
    let handle = normalize_handle(handle)?;
    let existing: Option<String> = conn
        .query_row(
            "SELECT id FROM creator_watchlist WHERE profile_id=?1 AND handle=?2 COLLATE NOCASE",
            params![profile_id, handle],
            |r| r.get(0),
        )
        .optional()?;
    if existing.is_some() {
        return Err(AppError::Validation(format!("@{handle} is already on the watchlist")));
    }
    let count: i64 = conn.query_row("SELECT count(*) FROM creator_watchlist WHERE profile_id=?1", [profile_id], |r| r.get(0))?;
    if count >= 50 {
        return Err(AppError::Validation("the watchlist is limited to 50 creators".into()));
    }
    let id = new_id();
    conn.execute(
        "INSERT INTO creator_watchlist (id, profile_id, handle, followers, added_at) VALUES (?1,?2,?3,?4,?5)",
        params![id, profile_id, handle, followers.map(|v| v as i64), now_rfc3339()],
    )?;
    list_watch(conn, profile_id)?
        .into_iter()
        .find(|w| w.id == id)
        .ok_or_else(|| AppError::Other("watch entry vanished".into()))
}

pub fn list_watch(conn: &Connection, profile_id: &str) -> AppResult<Vec<WatchEntry>> {
    let mut stmt = conn.prepare(
        "SELECT id, handle, followers, added_at, last_scanned_at FROM creator_watchlist WHERE profile_id=?1 ORDER BY added_at",
    )?;
    let rows = stmt.query_map([profile_id], |r| {
        Ok(WatchEntry {
            id: r.get(0)?,
            handle: r.get(1)?,
            followers: r.get::<_, Option<i64>>(2)?.map(|v| v.max(0) as u64),
            added_at: r.get(3)?,
            last_scanned_at: r.get(4)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn remove_watch(conn: &Connection, profile_id: &str, id: &str) -> AppResult<()> {
    conn.execute("DELETE FROM creator_watchlist WHERE id=?1 AND profile_id=?2", params![id, profile_id])?;
    Ok(())
}

pub fn touch_watch_scanned(conn: &Connection, profile_id: &str, id: &str) -> AppResult<()> {
    conn.execute(
        "UPDATE creator_watchlist SET last_scanned_at=?1 WHERE id=?2 AND profile_id=?3",
        params![now_rfc3339(), id, profile_id],
    )?;
    Ok(())
}

#[cfg(test)]
pub mod testing {
    use crate::storage;
    use rusqlite::Connection;

    pub fn db() -> (Connection, String, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let (conn, _) = storage::open(&dir.path().join("app.db"), &dir.path().join("b")).unwrap();
        let pid = storage::repo::ensure_default_profile(&conn, "/tmp/p").unwrap();
        (conn, pid, dir)
    }
}

#[cfg(test)]
mod tests {
    use super::testing::db;
    use super::*;
    use crate::formats::blueprint::Slot;

    fn bp(name: &str) -> FormatBlueprint {
        FormatBlueprint {
            name: name.into(),
            archetype: "contrarian_take".into(),
            hook_formula: "hook".into(),
            structural_pattern: "pattern".into(),
            skeleton: "Opinion: {belief} is wrong. {twist}".into(),
            slots: vec![
                Slot { key: "belief".into(), label: "Belief".into(), hint: "".into() },
                Slot { key: "twist".into(), label: "Twist".into(), hint: "".into() },
            ],
            media_hint: "text_only".into(),
        }
    }

    fn src(id: &str, score: f64, seen_at: &str) -> FormatSource {
        FormatSource { post_id: id.into(), author: "alice".into(), url: format!("https://x.com/alice/status/{id}"), seen_at: seen_at.into(), score }
    }

    fn cand(id: &str) -> NewCandidate {
        NewCandidate {
            platform_post_id: id.into(),
            author_handle: "alice".into(),
            author_followers: Some(1000),
            text: "text".into(),
            metrics: PostMetrics { likes: 500, ..Default::default() },
            posted_at: None,
            source: "manual",
            virality_score: 70.0,
            reasons: vec!["r".into()],
            author_name: "Alice".into(),
            avatar_url: Some("https://pbs.twimg.com/profile_images/1/a_bigger.jpg".into()),
            media: vec![],
        }
    }

    #[test]
    fn parse_secs_matches_now_format() {
        assert_eq!(parse_secs("1970-01-01T00:00:00Z"), 0);
        assert_eq!(parse_secs("2023-11-14T22:13:20Z"), 1_700_000_000);
        assert!(now_secs() > 1_700_000_000);
    }

    #[test]
    fn candidates_are_deduplicated_per_post() {
        let (conn, pid, _d) = db();
        assert!(insert_candidate(&conn, &pid, &cand("1")).unwrap().is_some());
        assert!(insert_candidate(&conn, &pid, &cand("1")).unwrap().is_none());
        assert_eq!(list_candidates(&conn, &pid, "pending", 10).unwrap().len(), 1);
    }

    #[test]
    fn merging_counts_sightings_keeps_sources_and_averages_the_rate() {
        let (conn, pid, _d) = db();
        let f = insert_format(&conn, &pid, &bp("A"), "detected", Some(src("1", 60.0, "2026-01-01T00:00:00Z")), Some("first")).unwrap();
        assert_eq!(f.observation_count, 1);
        let f = merge_sighting(&conn, &pid, &f.id, src("2", 80.0, &now_rfc3339()), "second").unwrap();
        assert_eq!(f.observation_count, 2);
        assert!((f.observed_win_rate.unwrap() - 0.7).abs() < 1e-9);
        assert_eq!(f.sources.len(), 2);
        assert_eq!(f.example_texts, vec!["first", "second"]);
        assert_eq!(f.recent_sightings, 1, "only the sighting from today counts as rising");
        let again = merge_sighting(&conn, &pid, &f.id, src("2", 80.0, &now_rfc3339()), "second").unwrap();
        assert_eq!(again.sources.len(), 2, "the same post is not listed twice");
    }

    #[test]
    fn sort_working_prefers_personal_then_observed_and_rising_prefers_recent() {
        let (conn, pid, _d) = db();
        let old = insert_format(&conn, &pid, &bp("Old strong"), "detected", Some(src("1", 95.0, "2026-01-01T00:00:00Z")), None).unwrap();
        let fresh = insert_format(&conn, &pid, &bp("Fresh"), "detected", Some(src("2", 40.0, &now_rfc3339())), None).unwrap();
        let mine = insert_format(&conn, &pid, &bp("Mine"), "manual", None, None).unwrap();
        conn.execute("UPDATE viral_formats SET personal_win_rate=0.2 WHERE id=?1", [&mine.id]).unwrap();

        let working: Vec<_> = list_formats(&conn, &pid, false, FormatSort::Working).unwrap().into_iter().map(|f| f.name).collect();
        assert_eq!(working, vec!["Mine", "Old strong", "Fresh"]);
        let rising = list_formats(&conn, &pid, false, FormatSort::Rising).unwrap();
        assert_eq!(rising[0].id, fresh.id);
        assert_ne!(rising[0].id, old.id);
    }

    #[test]
    fn archived_formats_are_hidden_until_asked_for() {
        let (conn, pid, _d) = db();
        let f = insert_format(&conn, &pid, &bp("A"), "manual", None, None).unwrap();
        set_format_status(&conn, &pid, &f.id, "archived").unwrap();
        assert!(list_formats(&conn, &pid, false, FormatSort::Recent).unwrap().is_empty());
        assert_eq!(list_formats(&conn, &pid, true, FormatSort::Recent).unwrap().len(), 1);
        assert!(set_format_status(&conn, &pid, &f.id, "weird").is_err());
    }

    #[test]
    fn edits_persist() {
        let (conn, pid, _d) = db();
        let f = insert_format(&conn, &pid, &bp("A"), "manual", None, None).unwrap();
        let mut edited = f.blueprint();
        edited.name = "Renamed".into();
        assert_eq!(update_format(&conn, &pid, &f.id, &edited).unwrap().name, "Renamed");
        assert!(update_format(&conn, &pid, "nope", &edited).is_err());
    }

    #[test]
    fn thresholds_round_trip_and_reject_nonsense() {
        let (conn, _pid, _d) = db();
        assert_eq!(get_thresholds(&conn).unwrap(), ViralThresholds::default());
        let t = ViralThresholds { reach_multiple: 0.5, ..Default::default() };
        set_thresholds(&conn, &t).unwrap();
        assert_eq!(get_thresholds(&conn).unwrap().reach_multiple, 0.5);
        assert!(set_thresholds(&conn, &ViralThresholds { min_engagement: 0.0, ..Default::default() }).is_err());
    }

    #[test]
    fn watchlist_normalises_dedupes_and_removes() {
        let (conn, pid, _d) = db();
        let w = add_watch(&conn, &pid, " @Naval ", Some(2_000_000)).unwrap();
        assert_eq!(w.handle, "Naval");
        assert!(add_watch(&conn, &pid, "naval", None).is_err());
        assert!(add_watch(&conn, &pid, "not valid!", None).is_err());
        remove_watch(&conn, &pid, &w.id).unwrap();
        assert!(list_watch(&conn, &pid).unwrap().is_empty());
    }
}
