//! Browsable history (SHL-21): scout runs, publishes, single actions, challenges and errors.

use crate::error::AppResult;
use crate::util::{new_id, now_rfc3339};
use rusqlite::{params, Connection};
use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ActivityEntry {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub detail: Option<String>,
    pub ok: Option<bool>,
    pub at: String,
    pub screenshot: Option<String>,
}

pub const KINDS: &[&str] = &["scout", "publish", "action", "challenge", "error"];

/// Best effort: a failure to write history never breaks the job that produced it.
pub fn record(conn: &Connection, profile_id: &str, kind: &str, title: &str, detail: Option<&str>, ok: Option<bool>, screenshot: Option<&str>) {
    if !KINDS.contains(&kind) {
        return;
    }
    let _ = conn.execute(
        "INSERT INTO activity_log (id, profile_id, kind, title, detail, ok, screenshot, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![new_id(), profile_id, kind, title, detail, ok.map(|b| b as i64), screenshot, now_rfc3339()],
    );
}

pub fn list(conn: &Connection, profile_id: &str, limit: i64) -> AppResult<Vec<ActivityEntry>> {
    let mut st = conn.prepare(
        "SELECT id, kind, title, detail, ok, created_at, screenshot FROM activity_log WHERE profile_id = ?1 ORDER BY created_at DESC, rowid DESC LIMIT ?2",
    )?;
    let rows = st.query_map(params![profile_id, limit.clamp(1, 1000)], |r| {
        Ok(ActivityEntry { id: r.get(0)?, kind: r.get(1)?, title: r.get(2)?, detail: r.get(3)?, ok: r.get::<_, Option<i64>>(4)?.map(|v| v != 0), at: r.get(5)?, screenshot: r.get(6)? })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_and_lists_newest_first_and_ignores_unknown_kinds() {
        let (conn, p) = crate::drafts::repo::testutil::db();
        record(&conn, &p, "scout", "Scout finished", Some("12 scanned"), Some(true), None);
        record(&conn, &p, "challenge", "X asked for a check", None, Some(false), Some("/tmp/a.png"));
        record(&conn, &p, "bogus", "x", None, None, None);
        let l = list(&conn, &p, 10).unwrap();
        assert_eq!(l.len(), 2);
        assert_eq!(l[0].kind, "challenge");
        assert_eq!(l[0].ok, Some(false));
        assert_eq!(l[0].screenshot.as_deref(), Some("/tmp/a.png"));
        assert_eq!(l[1].detail.as_deref(), Some("12 scanned"));
    }
}
