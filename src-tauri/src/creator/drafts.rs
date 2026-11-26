//! OGC drafts: storage, editing, media validation, and the claim step the publish gate uses after a user click.
//! Publishing itself only happens through `publish::gate::approve_top_level` and the Phase 3 executor (DRF-04, OGC-09).

use crate::error::{AppError, AppResult};
use crate::util::{new_id, now_rfc3339};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;
use std::path::{Path, PathBuf};

pub const MAX_IMAGES: usize = 4;
const IMAGE_EXT: &[&str] = &["png", "jpg", "jpeg", "webp"];
const GIF_EXT: &str = "gif";
const VIDEO_EXT: &[&str] = &["mp4", "mov"];
const MAX_IMAGE_BYTES: u64 = 5 * 1024 * 1024;
const MAX_GIF_BYTES: u64 = 15 * 1024 * 1024;
const MAX_VIDEO_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct OgcDraft {
    pub id: String,
    pub viral_format_id: Option<String>,
    pub batch_id: Option<String>,
    pub topic_seed: String,
    pub initial_text: String,
    pub edited_text: String,
    pub edit_distance: i64,
    pub media_paths: Vec<String>,
    pub model_used: String,
    /// variant | draft | publishing | published | failed | unconfirmed | discarded
    pub status: String,
    pub similarity: Option<f64>,
    pub published_url: Option<String>,
    pub error_message: Option<String>,
    pub user_approved_at: Option<String>,
    pub published_at: Option<String>,
    pub created_at: String,
}

const COLS: &str = "id, viral_format_id, batch_id, topic_seed, initial_text, edited_text, edit_distance, media_paths_json, \
model_used, status, similarity, published_url, error_message, user_approved_at, published_at, created_at";

fn row(r: &Row) -> rusqlite::Result<OgcDraft> {
    let media: String = r.get(7)?;
    Ok(OgcDraft {
        id: r.get(0)?,
        viral_format_id: r.get(1)?,
        batch_id: r.get(2)?,
        topic_seed: r.get(3)?,
        initial_text: r.get(4)?,
        edited_text: r.get(5)?,
        edit_distance: r.get(6)?,
        media_paths: serde_json::from_str(&media).unwrap_or_default(),
        model_used: r.get(8)?,
        status: r.get(9)?,
        similarity: r.get(10)?,
        published_url: r.get(11)?,
        error_message: r.get(12)?,
        user_approved_at: r.get(13)?,
        published_at: r.get(14)?,
        created_at: r.get(15)?,
    })
}

/// Character-level Levenshtein distance, stored for the learning loop (what the user changed).
pub fn edit_distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            cur.push((prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    prev[b.len()]
}

pub struct NewDraft<'a> {
    pub format_id: Option<&'a str>,
    pub batch_id: Option<&'a str>,
    pub topic_seed: &'a str,
    pub text: &'a str,
    pub model_used: &'a str,
    pub status: &'a str,
    pub similarity: Option<f64>,
}

pub fn insert(conn: &Connection, profile_id: &str, d: &NewDraft) -> AppResult<OgcDraft> {
    let id = new_id();
    conn.execute(
        "INSERT INTO ogc_drafts (id, profile_id, viral_format_id, topic_seed, initial_text, edited_text, edit_distance, \
         media_paths_json, model_used, status, created_at, batch_id, similarity) VALUES (?1,?2,?3,?4,?5,?5,0,'[]',?6,?7,?8,?9,?10)",
        params![id, profile_id, d.format_id, d.topic_seed, d.text, d.model_used, d.status, now_rfc3339(), d.batch_id, d.similarity],
    )?;
    get(conn, profile_id, &id)?.ok_or_else(|| AppError::Other("draft vanished after insert".into()))
}

pub fn get(conn: &Connection, profile_id: &str, id: &str) -> AppResult<Option<OgcDraft>> {
    Ok(conn
        .query_row(&format!("SELECT {COLS} FROM ogc_drafts WHERE id=?1 AND profile_id=?2"), params![id, profile_id], row)
        .optional()?)
}

pub fn list_recent(conn: &Connection, profile_id: &str, limit: usize) -> AppResult<Vec<OgcDraft>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM ogc_drafts WHERE profile_id=?1 AND status != 'discarded' ORDER BY created_at DESC, rowid DESC LIMIT ?2"
    ))?;
    let rows = stmt.query_map(params![profile_id, limit as i64], row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

fn editable(status: &str) -> bool {
    matches!(status, "variant" | "draft" | "failed")
}

fn must_get(conn: &Connection, profile_id: &str, id: &str) -> AppResult<OgcDraft> {
    get(conn, profile_id, id)?.ok_or_else(|| AppError::Validation("draft not found".into()))
}

pub fn update_text(conn: &Connection, profile_id: &str, id: &str, text: &str, similarity: Option<f64>) -> AppResult<OgcDraft> {
    let d = must_get(conn, profile_id, id)?;
    if !editable(&d.status) {
        return Err(AppError::Validation(format!("a {} draft cannot be edited", d.status)));
    }
    let distance = edit_distance(&d.initial_text, text) as i64;
    conn.execute(
        "UPDATE ogc_drafts SET edited_text=?1, edit_distance=?2, similarity=?3, status='draft' WHERE id=?4 AND profile_id=?5",
        params![text, distance, similarity, id, profile_id],
    )?;
    must_get(conn, profile_id, id)
}

pub fn discard(conn: &Connection, profile_id: &str, id: &str) -> AppResult<()> {
    let d = must_get(conn, profile_id, id)?;
    if matches!(d.status.as_str(), "publishing" | "published") {
        return Err(AppError::Validation(format!("a {} draft cannot be discarded", d.status)));
    }
    conn.execute("UPDATE ogc_drafts SET status='discarded' WHERE id=?1 AND profile_id=?2", params![id, profile_id])?;
    Ok(())
}

/// For a post the user has confirmed is not on X: makes a failed or unconfirmed draft publishable again.
pub fn reopen(conn: &Connection, profile_id: &str, id: &str) -> AppResult<OgcDraft> {
    let d = must_get(conn, profile_id, id)?;
    if !matches!(d.status.as_str(), "failed" | "unconfirmed") {
        return Err(AppError::Validation("only failed or unconfirmed drafts can be reopened".into()));
    }
    conn.execute(
        "UPDATE ogc_drafts SET status='draft', error_message=NULL, user_approved_at=NULL WHERE id=?1 AND profile_id=?2",
        params![id, profile_id],
    )?;
    must_get(conn, profile_id, id)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MediaKind {
    Images,
    Gif,
    Video,
}

fn classify(path: &Path) -> Result<MediaKind, AppError> {
    let ext = path.extension().and_then(|e| e.to_str()).map(|e| e.to_lowercase()).unwrap_or_default();
    if IMAGE_EXT.contains(&ext.as_str()) {
        Ok(MediaKind::Images)
    } else if ext == GIF_EXT {
        Ok(MediaKind::Gif)
    } else if VIDEO_EXT.contains(&ext.as_str()) {
        Ok(MediaKind::Video)
    } else {
        Err(AppError::Validation(format!("\"{}\" is not a supported media type (png, jpg, webp, gif, mp4, mov)", path.display())))
    }
}

/// Checks type, size and combination limits for the files that would be attached. Attachments are local
/// files only; nothing is generated or uploaded anywhere but X.
pub fn validate_media(paths: &[PathBuf]) -> AppResult<()> {
    let mut kinds = Vec::new();
    for p in paths {
        if !p.is_absolute() {
            return Err(AppError::Validation("media paths must be absolute".into()));
        }
        let kind = classify(p)?;
        let meta = std::fs::metadata(p).map_err(|_| AppError::Validation(format!("\"{}\" does not exist", p.display())))?;
        if !meta.is_file() {
            return Err(AppError::Validation(format!("\"{}\" is not a file", p.display())));
        }
        let (limit, label) = match kind {
            MediaKind::Images => (MAX_IMAGE_BYTES, "5 MB for images"),
            MediaKind::Gif => (MAX_GIF_BYTES, "15 MB for GIFs"),
            MediaKind::Video => (MAX_VIDEO_BYTES, "512 MB for videos"),
        };
        if meta.len() > limit {
            return Err(AppError::Validation(format!("\"{}\" is too large (limit {label})", p.display())));
        }
        kinds.push(kind);
    }
    let images = kinds.iter().filter(|k| **k == MediaKind::Images).count();
    if images > MAX_IMAGES {
        return Err(AppError::Validation(format!("a post can hold at most {MAX_IMAGES} images")));
    }
    if images < kinds.len() && kinds.len() > 1 {
        return Err(AppError::Validation("a GIF or video cannot be combined with other media".into()));
    }
    Ok(())
}

pub fn set_media(conn: &Connection, profile_id: &str, id: &str, paths: &[PathBuf]) -> AppResult<OgcDraft> {
    let d = must_get(conn, profile_id, id)?;
    if !editable(&d.status) {
        return Err(AppError::Validation(format!("a {} draft cannot be edited", d.status)));
    }
    validate_media(paths)?;
    let strings: Vec<String> = paths.iter().map(|p| p.display().to_string()).collect();
    conn.execute(
        "UPDATE ogc_drafts SET media_paths_json=?1 WHERE id=?2 AND profile_id=?3",
        params![serde_json::to_string(&strings).unwrap(), id, profile_id],
    )?;
    must_get(conn, profile_id, id)
}

/// A draft claimed for publishing. Built only by [`claim`], which the publish gate calls after a user click.
#[derive(Debug)]
pub struct ClaimedOgc {
    draft_id: String,
    text: String,
    media: Vec<PathBuf>,
}

impl ClaimedOgc {
    pub fn draft_id(&self) -> &str {
        &self.draft_id
    }
    pub fn text(&self) -> &str {
        &self.text
    }
    pub fn media(&self) -> &[PathBuf] {
        &self.media
    }
}

/// Records the user's approval and claims the draft for publishing in one atomic update, so two clicks can never
/// publish twice. `expected_text` is what the user was looking at; if it differs from what is stored the
/// approval is refused rather than publishing something they did not see.
pub(crate) fn claim(conn: &Connection, profile_id: &str, id: &str, expected_text: &str, max_chars: usize) -> AppResult<ClaimedOgc> {
    let d = must_get(conn, profile_id, id)?;
    if d.edited_text != expected_text {
        return Err(AppError::Validation("the draft changed since you last looked at it; review it and publish again".into()));
    }
    let text = d.edited_text.trim().to_string();
    if text.is_empty() {
        return Err(AppError::Validation("the post is empty".into()));
    }
    if text.chars().count() > max_chars {
        return Err(AppError::Validation(format!("the post is {} characters; the limit is {max_chars}", text.chars().count())));
    }
    let media: Vec<PathBuf> = d.media_paths.iter().map(PathBuf::from).collect();
    validate_media(&media)?;

    let claimed = conn.execute(
        "UPDATE ogc_drafts SET status='publishing', user_approved_at=?1, error_message=NULL \
         WHERE id=?2 AND profile_id=?3 AND status IN ('variant','draft','failed')",
        params![now_rfc3339(), id, profile_id],
    )?;
    if claimed == 0 {
        return Err(AppError::Validation(format!("a {} draft cannot be published", d.status)));
    }
    Ok(ClaimedOgc { draft_id: id.to_string(), text, media })
}

/// Texts of the most recent published original posts, newest first, for the duplicate check.
pub fn recent_published_texts(conn: &Connection, profile_id: &str, limit: i64) -> AppResult<Vec<String>> {
    let mut st = conn.prepare(
        "SELECT edited_text FROM ogc_drafts WHERE profile_id = ?1 AND status = 'published' AND edited_text <> '' \
         ORDER BY published_at DESC LIMIT ?2",
    )?;
    let rows = st.query_map(params![profile_id, limit], |r| r.get::<_, String>(0))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn mark_published(conn: &Connection, profile_id: &str, id: &str, url: Option<&str>) -> AppResult<()> {
    conn.execute(
        "UPDATE ogc_drafts SET status='published', published_at=?1, published_url=?2, error_message=NULL WHERE id=?3 AND profile_id=?4",
        params![now_rfc3339(), url, id, profile_id],
    )?;
    Ok(())
}

/// `unconfirmed` means the post button was clicked but success was not seen: it may be live, so it is never retried automatically.
pub fn mark_failed(conn: &Connection, profile_id: &str, id: &str, message: &str, unconfirmed: bool) -> AppResult<()> {
    conn.execute(
        "UPDATE ogc_drafts SET status=?1, error_message=?2 WHERE id=?3 AND profile_id=?4",
        params![if unconfirmed { "unconfirmed" } else { "failed" }, message, id, profile_id],
    )?;
    Ok(())
}

pub fn write_audit(conn: &Connection, profile_id: &str, draft_id: &str, approval_ref: &str, success: bool, before: Option<&str>, after: Option<&str>, error: Option<&str>) -> AppResult<()> {
    conn.execute(
        "INSERT INTO publish_audit_log (id, profile_id, target_kind, target_draft_id, approval_ref, actions_executed_json, \
         before_screenshot_path, after_screenshot_path, success, error_message, created_at) VALUES (?1,?2,'ogc',?3,?4,?5,?6,?7,?8,?9,?10)",
        params![
            new_id(),
            profile_id,
            draft_id,
            approval_ref,
            if success { "[\"post\"]" } else { "[]" },
            before,
            after,
            success,
            error,
            now_rfc3339()
        ],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formats::repo::testing::db;

    fn draft(conn: &Connection, pid: &str, text: &str) -> OgcDraft {
        insert(conn, pid, &NewDraft { format_id: None, batch_id: Some("b1"), topic_seed: "t", text, model_used: "m", status: "variant", similarity: None }).unwrap()
    }

    fn file(dir: &Path, name: &str, bytes: usize) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, vec![0u8; bytes]).unwrap();
        p
    }

    #[test]
    fn edit_distance_is_character_level() {
        assert_eq!(edit_distance("kitten", "sitting"), 3);
        assert_eq!(edit_distance("", "abc"), 3);
        assert_eq!(edit_distance("same", "same"), 0);
    }

    #[test]
    fn editing_stores_both_texts_and_the_distance_and_promotes_a_variant() {
        let (conn, pid, _d) = db();
        let d = draft(&conn, &pid, "hello world");
        let e = update_text(&conn, &pid, &d.id, "hello there", Some(0.1)).unwrap();
        assert_eq!((e.initial_text.as_str(), e.edited_text.as_str(), e.edit_distance, e.status.as_str()), ("hello world", "hello there", 5, "draft"));
        assert_eq!(e.similarity, Some(0.1));
    }

    #[test]
    fn approval_refuses_stale_empty_overlong_and_duplicate_publishes() {
        let (conn, pid, _d) = db();
        let d = draft(&conn, &pid, "hello");
        assert!(claim(&conn, &pid, &d.id, "something else", 280).unwrap_err().to_string().contains("changed"));
        let empty = update_text(&conn, &pid, &d.id, "   ", None).unwrap();
        assert!(claim(&conn, &pid, &empty.id, "   ", 280).unwrap_err().to_string().contains("empty"));
        let long = update_text(&conn, &pid, &d.id, &"x".repeat(281), None).unwrap();
        assert!(claim(&conn, &pid, &long.id, &long.edited_text, 280).unwrap_err().to_string().contains("281"));
        update_text(&conn, &pid, &d.id, "ok", None).unwrap();
        let a = claim(&conn, &pid, &d.id, "ok", 280).unwrap();
        assert_eq!(a.text(), "ok");
        assert_eq!(get(&conn, &pid, &d.id).unwrap().unwrap().status, "publishing");
        assert!(claim(&conn, &pid, &d.id, "ok", 280).is_err(), "a second approval must not claim it again");
        assert!(update_text(&conn, &pid, &d.id, "edit while publishing", None).is_err());
        assert!(discard(&conn, &pid, &d.id).is_err());
    }

    #[test]
    fn unconfirmed_is_never_retried_until_reopened() {
        let (conn, pid, _d) = db();
        let d = draft(&conn, &pid, "hello");
        let a = claim(&conn, &pid, &d.id, "hello", 280).unwrap();
        mark_failed(&conn, &pid, a.draft_id(), "unsure", true).unwrap();
        assert!(claim(&conn, &pid, &d.id, "hello", 280).is_err());
        reopen(&conn, &pid, &d.id).unwrap();
        assert!(claim(&conn, &pid, &d.id, "hello", 280).is_ok());
    }

    #[test]
    fn failed_drafts_can_be_retried_and_published_ones_cannot() {
        let (conn, pid, _d) = db();
        let d = draft(&conn, &pid, "hello");
        let a = claim(&conn, &pid, &d.id, "hello", 280).unwrap();
        mark_failed(&conn, &pid, a.draft_id(), "composer never appeared", false).unwrap();
        let a2 = claim(&conn, &pid, &d.id, "hello", 280).unwrap();
        mark_published(&conn, &pid, a2.draft_id(), Some("https://x.com/a/status/1")).unwrap();
        assert!(claim(&conn, &pid, &d.id, "hello", 280).is_err());
        assert!(discard(&conn, &pid, &d.id).is_err());
    }

    #[test]
    fn audit_row_carries_the_approval_reference() {
        let (conn, pid, _d) = db();
        let d = draft(&conn, &pid, "hello");
        let a = claim(&conn, &pid, &d.id, "hello", 280).unwrap();
        write_audit(&conn, &pid, a.draft_id(), "ref-1", true, None, Some("/a.png"), None).unwrap();
        let (kind, appr, ok): (String, String, i64) = conn
            .query_row("SELECT target_kind, approval_ref, success FROM publish_audit_log", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap();
        assert_eq!((kind.as_str(), appr.as_str(), ok), ("ogc", "ref-1", 1));
    }

    #[test]
    fn media_rules() {
        let dir = tempfile::tempdir().unwrap();
        let png = |n: &str| file(dir.path(), n, 10);
        assert!(validate_media(&[png("a.png"), png("b.JPG"), png("c.webp"), png("d.png")]).is_ok());
        assert!(validate_media(&[png("a1.png"), png("b1.png"), png("c1.png"), png("d1.png"), png("e1.png")]).is_err());
        assert!(validate_media(&[png("p.png"), file(dir.path(), "v.mp4", 10)]).is_err(), "no mixing video with images");
        assert!(validate_media(&[file(dir.path(), "big.png", 5 * 1024 * 1024 + 1)]).unwrap_err().to_string().contains("too large"));
        assert!(validate_media(&[file(dir.path(), "x.exe", 1)]).unwrap_err().to_string().contains("not a supported"));
        assert!(validate_media(&[dir.path().join("missing.png")]).unwrap_err().to_string().contains("does not exist"));
        assert!(validate_media(&[PathBuf::from("relative.png")]).is_err());
        assert!(validate_media(&[]).is_ok());
    }

    #[test]
    fn media_is_stored_on_the_draft_and_checked_again_at_approval() {
        let (conn, pid, _d) = db();
        let dir = tempfile::tempdir().unwrap();
        let img = file(dir.path(), "a.png", 10);
        let d = draft(&conn, &pid, "hello");
        let d = set_media(&conn, &pid, &d.id, &[img.clone()]).unwrap();
        assert_eq!(d.media_paths, vec![img.display().to_string()]);
        std::fs::remove_file(&img).unwrap();
        assert!(claim(&conn, &pid, &d.id, "hello", 280).unwrap_err().to_string().contains("does not exist"));
        assert_eq!(get(&conn, &pid, &d.id).unwrap().unwrap().status, "variant", "a refused approval claims nothing");
    }
}
