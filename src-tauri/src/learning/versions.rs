//! Version history, diffs and rollback for the scout rubric and style guide (LRN-08, LRN-09, STY-07),
//! plus the few-shot example library that winner promotion writes to (LRN-10).

use super::patch::rubric_lines;
use super::textdiff::{line_diff, DiffLine};
use crate::error::{AppError, AppResult};
use crate::util::{new_id, now_rfc3339};
use rusqlite::{params, Connection};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize)]
pub struct VersionInfo {
    pub kind: &'static str,
    pub version: i64,
    pub is_active: bool,
    pub created_at: String,
    pub lines: Vec<String>,
    /// Change from the previous version; empty for the first one.
    pub diff: Vec<DiffLine>,
    /// Insight that produced this version, if it came from the learning loop.
    pub insight_id: Option<String>,
}

fn attach_insights(conn: &Connection, profile_id: &str, kind: &str, versions: &mut [VersionInfo]) -> AppResult<()> {
    let mut stmt = conn.prepare("SELECT to_version, insight_id FROM learning_changelog WHERE profile_id=?1 AND kind=?2 AND action='accepted'")?;
    let rows = stmt.query_map(params![profile_id, kind], |r| Ok((r.get::<_, Option<i64>>(0)?, r.get::<_, Option<String>>(1)?)))?;
    let mut by: BTreeMap<i64, Option<String>> = BTreeMap::new();
    for r in rows {
        let (v, i) = r?;
        if let Some(v) = v {
            by.insert(v, i);
        }
    }
    for v in versions {
        v.insight_id = by.get(&v.version).cloned().flatten();
    }
    Ok(())
}

fn with_diffs(mut asc: Vec<VersionInfo>) -> Vec<VersionInfo> {
    for i in 1..asc.len() {
        asc[i].diff = line_diff(&asc[i - 1].lines, &asc[i].lines);
    }
    asc.reverse();
    asc
}

pub fn rubric_versions(conn: &Connection, profile_id: &str) -> AppResult<Vec<VersionInfo>> {
    let mut stmt = conn.prepare("SELECT version, scout_criteria_text, weights_json, is_active, created_at FROM learning_rubrics WHERE profile_id=?1 ORDER BY version")?;
    let rows = stmt.query_map([profile_id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, i64>(3)?, r.get::<_, String>(4)?)))?;
    let mut asc = vec![];
    for r in rows {
        let (version, text, w, active, created_at) = r?;
        let weights: BTreeMap<String, f64> = serde_json::from_str(&w).map_err(|e| AppError::Other(format!("corrupt rubric weights: {e}")))?;
        asc.push(VersionInfo { kind: "rubric", version, is_active: active != 0, created_at, lines: rubric_lines(&text, &weights), diff: vec![], insight_id: None });
    }
    let mut out = with_diffs(asc);
    attach_insights(conn, profile_id, "rubric", &mut out)?;
    Ok(out)
}

/// Makes an older rubric version active again. Versions are never edited or deleted; rollback only moves the
/// active flag and marks the insights that produced the undone versions as rolled back.
/// Style versions are not rolled back here: the Style page owns that history (STY-07).
pub fn rollback(conn: &Connection, profile_id: &str, kind: &str, version: i64) -> AppResult<()> {
    let (table, col) = match kind {
        "rubric" => ("learning_rubrics", "version"),
        "style" => return Err(AppError::Validation("style guide versions are managed on the Style page (STY-07)".into())),
        _ => return Err(AppError::Validation("only the scout rubric has versions here".into())),
    };
    let tx = conn.unchecked_transaction()?;
    let active: Option<i64> = tx
        .query_row(&format!("SELECT {col} FROM {table} WHERE profile_id=?1 AND is_active=1 ORDER BY {col} DESC LIMIT 1"), [profile_id], |r| r.get(0))
        .ok();
    let exists: i64 = tx.query_row(&format!("SELECT count(*) FROM {table} WHERE profile_id=?1 AND {col}=?2"), params![profile_id, version], |r| r.get(0))?;
    if exists == 0 {
        return Err(AppError::Validation(format!("{kind} version {version} does not exist")));
    }
    let Some(active) = active else {
        return Err(AppError::Validation("there is no active version to roll back from".into()));
    };
    if version >= active {
        return Err(AppError::Validation("roll back to an older version than the active one".into()));
    }
    tx.execute(&format!("UPDATE {table} SET is_active=0 WHERE profile_id=?1"), [profile_id])?;
    tx.execute(&format!("UPDATE {table} SET is_active=1 WHERE profile_id=?1 AND {col}=?2"), params![profile_id, version])?;
    tx.execute(
        "UPDATE learning_insights SET status='rolled_back', decided_at=?3
         WHERE profile_id=?1 AND status='accepted' AND id IN (
           SELECT insight_id FROM learning_changelog
           WHERE profile_id=?1 AND kind=?2 AND action='accepted' AND insight_id IS NOT NULL AND to_version > ?4)",
        params![profile_id, kind, now_rfc3339(), version],
    )?;
    tx.execute(
        "INSERT INTO learning_changelog (id, profile_id, kind, action, from_version, to_version, insight_id, created_at)
         VALUES (?1,?2,?3,'rolled_back',?4,?5,NULL,?6)",
        params![new_id(), profile_id, kind, active, version, now_rfc3339()],
    )?;
    tx.commit()?;
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct ChangelogEntry {
    pub id: String,
    pub kind: String,
    pub action: String,
    pub from_version: Option<i64>,
    pub to_version: Option<i64>,
    pub insight_id: Option<String>,
    pub created_at: String,
}

pub fn changelog(conn: &Connection, profile_id: &str, limit: usize) -> AppResult<Vec<ChangelogEntry>> {
    let mut stmt = conn.prepare(
        "SELECT id, kind, action, from_version, to_version, insight_id, created_at FROM learning_changelog
         WHERE profile_id=?1 ORDER BY created_at DESC, rowid DESC LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![profile_id, limit as i64], |r| {
        Ok(ChangelogEntry { id: r.get(0)?, kind: r.get(1)?, action: r.get(2)?, from_version: r.get(3)?, to_version: r.get(4)?, insight_id: r.get(5)?, created_at: r.get(6)? })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

#[derive(Debug, Clone, Serialize)]
pub struct ExampleInfo {
    pub id: String,
    pub kind: String,
    pub example_text: String,
    pub source_context_text: Option<String>,
    pub why_it_works: Option<String>,
    pub source: String,
    pub performance_score: Option<f64>,
    pub pinned: bool,
    pub created_at: String,
}

pub fn examples(conn: &Connection, profile_id: &str) -> AppResult<Vec<ExampleInfo>> {
    let mut stmt = conn.prepare(
        "SELECT id, kind, example_text, source_context_text, why_it_works, source, performance_score, pinned, created_at
         FROM style_examples WHERE profile_id=?1 ORDER BY pinned DESC, created_at DESC, id",
    )?;
    let rows = stmt.query_map([profile_id], |r| {
        Ok(ExampleInfo {
            id: r.get(0)?,
            kind: r.get(1)?,
            example_text: r.get(2)?,
            source_context_text: r.get(3)?,
            why_it_works: r.get(4)?,
            source: r.get(5)?,
            performance_score: r.get(6)?,
            pinned: r.get::<_, i64>(7)? != 0,
            created_at: r.get(8)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Removes an example that the learning loop promoted. Examples you added yourself are managed in the style library.
pub fn remove_promoted_example(conn: &Connection, profile_id: &str, id: &str) -> AppResult<()> {
    let n = conn.execute("DELETE FROM style_examples WHERE id=?1 AND profile_id=?2 AND source='promoted_winner'", params![id, profile_id])?;
    if n == 0 {
        return Err(AppError::Validation("only promoted examples can be removed here".into()));
    }
    conn.execute(
        "INSERT INTO learning_changelog (id, profile_id, kind, action, from_version, to_version, insight_id, created_at)
         VALUES (?1,?2,'examples','rolled_back',NULL,NULL,NULL,?3)",
        params![new_id(), profile_id, now_rfc3339()],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::learning::patch::tests::db;
    use crate::learning::patch::{active_rubric, apply, Patch, PatchOp, PatchTarget};
    use crate::learning::store::{accept, get, insert_draft};

    fn rubric_patch(text: &str) -> Patch {
        Patch { target: PatchTarget::Rubric, ops: vec![PatchOp::AddCriterion { text: text.into() }] }
    }

    #[test]
    fn rubric_versions_list_with_diffs_and_rollback_restores_an_older_version() {
        let (_d, c, pid) = db();
        apply(&c, &pid, &rubric_patch("Favor fresh posts"), None).unwrap();
        apply(&c, &pid, &rubric_patch("Skip hiring threads"), None).unwrap();
        let v = rubric_versions(&c, &pid).unwrap();
        assert_eq!(v.iter().map(|x| x.version).collect::<Vec<_>>(), vec![3, 2, 1]);
        assert!(v[0].is_active && !v[1].is_active);
        assert!(v[0].diff.iter().any(|l| l.kind == "add" && l.text.contains("Skip hiring threads")));
        assert!(v[2].diff.is_empty());

        rollback(&c, &pid, "rubric", 2).unwrap();
        let r = active_rubric(&c, &pid).unwrap();
        assert_eq!(r.version, Some(2));
        assert!(r.text.contains("Favor fresh posts") && !r.text.contains("Skip hiring threads"));
        assert!(rollback(&c, &pid, "rubric", 3).is_err(), "cannot roll forward");
        assert!(rollback(&c, &pid, "rubric", 99).is_err());
        assert!(rollback(&c, &pid, "style", 1).is_err(), "style history lives on the Style page");
        assert!(rollback(&c, &pid, "voice", 1).is_err());
        let log = changelog(&c, &pid, 10).unwrap();
        assert_eq!((log[0].action.as_str(), log[0].from_version, log[0].to_version), ("rolled_back", Some(3), Some(2)));
    }

    #[test]
    fn rolling_back_the_first_change_restores_the_onboarding_default() {
        let (_d, c, pid) = db();
        let before = active_rubric(&c, &pid).unwrap();
        let id = insert_draft(&c, &pid, &crate::learning::store::tests::draft("k", "x y"), 40, 30, "d").unwrap().unwrap();
        let rub = Patch { target: PatchTarget::Rubric, ops: vec![PatchOp::AddCriterion { text: "Favor fresh posts".into() }] };
        let rid = {
            let mut d = crate::learning::store::tests::draft("r", "unused");
            d.patch = rub;
            insert_draft(&c, &pid, &d, 40, 30, "d").unwrap().unwrap()
        };
        let ch = accept(&c, &pid, &rid, None).unwrap();
        assert_eq!(active_rubric(&c, &pid).unwrap().version, ch.to_version);
        assert!(active_rubric(&c, &pid).unwrap().text.contains("Favor fresh posts"));
        rollback(&c, &pid, "rubric", ch.from_version.unwrap()).unwrap();
        let after = active_rubric(&c, &pid).unwrap();
        assert_eq!(after.text, before.text);
        assert_eq!(get(&c, &pid, &rid).unwrap().status, "rolled_back");
        assert_eq!(get(&c, &pid, &id).unwrap().status, "proposed", "unrelated insights are untouched");
        let n: i64 = c.query_row("SELECT count(*) FROM learning_rubrics", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 2, "history is kept");
    }

    #[test]
    fn promoted_examples_can_be_removed_but_user_examples_cannot() {
        let (_d, c, pid) = db();
        c.execute("INSERT INTO style_examples (id, profile_id, kind, example_text, source, created_at) VALUES ('u1', ?1, 'reply', 'mine', 'user', '2026-01-01T00:00:00Z')", [&pid]).unwrap();
        c.execute("INSERT INTO style_examples (id, profile_id, kind, example_text, source, created_at) VALUES ('p1', ?1, 'reply', 'promoted', 'promoted_winner', '2026-01-02T00:00:00Z')", [&pid]).unwrap();
        assert_eq!(examples(&c, &pid).unwrap().len(), 2);
        assert!(remove_promoted_example(&c, &pid, "u1").is_err());
        remove_promoted_example(&c, &pid, "p1").unwrap();
        assert_eq!(examples(&c, &pid).unwrap().len(), 1);
    }
}
