use crate::error::{AppError, AppResult};
use crate::util::{new_id, now_rfc3339};
use rusqlite::{params, Connection};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReplyFormat {
    pub id: String,
    pub name: String,
    pub pattern: String,
    pub when_to_use: String,
    pub why_it_works: String,
    pub source_replies: u32,
    pub avg_likes: f64,
    pub enabled: bool,
    pub created_at: String,
}

fn map(r: &rusqlite::Row<'_>) -> rusqlite::Result<ReplyFormat> {
    Ok(ReplyFormat {
        id: r.get(0)?,
        name: r.get(1)?,
        pattern: r.get(2)?,
        when_to_use: r.get(3)?,
        why_it_works: r.get(4)?,
        source_replies: r.get(5)?,
        avg_likes: r.get(6)?,
        enabled: r.get::<_, i64>(7)? != 0,
        created_at: r.get(8)?,
    })
}

const COLS: &str = "id, name, pattern, when_to_use, why_it_works, source_replies, avg_likes, enabled, created_at";

pub fn list(conn: &Connection, profile_id: &str) -> AppResult<Vec<ReplyFormat>> {
    let mut st = conn.prepare(&format!("SELECT {COLS} FROM reply_formats WHERE profile_id=?1 ORDER BY enabled DESC, avg_likes DESC, created_at DESC"))?;
    let rows = st.query_map([profile_id], map)?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn list_enabled(conn: &Connection, profile_id: &str, limit: usize) -> AppResult<Vec<ReplyFormat>> {
    let mut st = conn.prepare(&format!("SELECT {COLS} FROM reply_formats WHERE profile_id=?1 AND enabled=1 ORDER BY avg_likes DESC, created_at DESC LIMIT ?2"))?;
    let rows = st.query_map(params![profile_id, limit as i64], map)?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn key(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// Adds a format unless one with the same name or pattern is already stored. Returns it when added.
pub fn insert_if_new(conn: &Connection, profile_id: &str, f: &NewFormat) -> AppResult<Option<ReplyFormat>> {
    let existing = list(conn, profile_id)?;
    if existing.iter().any(|e| key(&e.name) == key(&f.name) || key(&e.pattern) == key(&f.pattern)) {
        return Ok(None);
    }
    let id = new_id();
    let now = now_rfc3339();
    conn.execute(
        "INSERT INTO reply_formats (id, profile_id, name, pattern, when_to_use, why_it_works, source_replies, avg_likes, enabled, created_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,1,?9)",
        params![id, profile_id, f.name, f.pattern, f.when_to_use, f.why_it_works, f.source_replies, f.avg_likes, now],
    )?;
    Ok(Some(ReplyFormat {
        id,
        name: f.name.clone(),
        pattern: f.pattern.clone(),
        when_to_use: f.when_to_use.clone(),
        why_it_works: f.why_it_works.clone(),
        source_replies: f.source_replies,
        avg_likes: f.avg_likes,
        enabled: true,
        created_at: now,
    }))
}

pub struct NewFormat {
    pub name: String,
    pub pattern: String,
    pub when_to_use: String,
    pub why_it_works: String,
    pub source_replies: u32,
    pub avg_likes: f64,
}

pub fn set_enabled(conn: &Connection, profile_id: &str, id: &str, enabled: bool) -> AppResult<()> {
    let n = conn.execute("UPDATE reply_formats SET enabled=?3 WHERE id=?2 AND profile_id=?1", params![profile_id, id, enabled as i64])?;
    if n == 0 {
        return Err(AppError::Validation("that format no longer exists".into()));
    }
    Ok(())
}

pub fn delete(conn: &Connection, profile_id: &str, id: &str) -> AppResult<()> {
    conn.execute("DELETE FROM reply_formats WHERE id=?2 AND profile_id=?1", params![profile_id, id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nf(name: &str, pattern: &str, likes: f64) -> NewFormat {
        NewFormat { name: name.into(), pattern: pattern.into(), when_to_use: "w".into(), why_it_works: "y".into(), source_replies: 3, avg_likes: likes }
    }

    #[test]
    fn stores_dedupes_toggles_and_orders_by_performance() {
        let conn = crate::storage::open_in_memory().unwrap();
        let profile = crate::storage::repo::ensure_default_profile(&conn, "/tmp/p").unwrap();
        assert!(insert_if_new(&conn, &profile, &nf("Counter then proof", "Disagree with [claim], then [example]", 10.0)).unwrap().is_some());
        assert!(insert_if_new(&conn, &profile, &nf("counter  THEN proof", "other", 1.0)).unwrap().is_none(), "same name");
        assert!(insert_if_new(&conn, &profile, &nf("Other", "disagree with [claim],  then [example]", 1.0)).unwrap().is_none(), "same pattern");
        let b = insert_if_new(&conn, &profile, &nf("Question pivot", "Ask [the obvious next question]", 25.0)).unwrap().unwrap();
        assert_eq!(list_enabled(&conn, &profile, 5).unwrap()[0].name, "Question pivot");
        set_enabled(&conn, &profile, &b.id, false).unwrap();
        let enabled = list_enabled(&conn, &profile, 5).unwrap();
        assert_eq!(enabled.len(), 1);
        assert_eq!(list(&conn, &profile).unwrap().len(), 2);
        delete(&conn, &profile, &b.id).unwrap();
        assert_eq!(list(&conn, &profile).unwrap().len(), 1);
        assert!(set_enabled(&conn, &profile, "nope", true).is_err());
    }
}
