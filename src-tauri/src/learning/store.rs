//! Persistence for insight cards (LRN-06) and the human approval gate (LRN-07).
//!
//! Nothing in this module changes a rubric, style guide or example library except `accept`, which is only
//! reachable from an explicit user action.

use super::analyzers::{Evidence, InsightDraft};
use super::patch::{self, AppliedChange, Patch};
use super::textdiff::DiffLine;
use crate::error::{AppError, AppResult};
use crate::storage::repo;
use crate::util::{new_id, now_rfc3339};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

pub const DEFAULT_MIN_SAMPLE: usize = 30;
const MIN_SAMPLE_KEY: &str = "learning_min_sample";
const LOW_CONFIDENCE_CAP: f64 = 0.5;
const CONFIDENCE_CAP: f64 = 0.95;

pub fn min_sample(conn: &Connection) -> AppResult<usize> {
    Ok(repo::get_setting(conn, MIN_SAMPLE_KEY)?
        .and_then(|v| v.parse::<usize>().ok())
        .map(|n| n.clamp(1, 1000))
        .unwrap_or(DEFAULT_MIN_SAMPLE))
}

pub fn set_min_sample(conn: &Connection, n: usize) -> AppResult<usize> {
    if !(1..=1000).contains(&n) {
        return Err(AppError::Validation("minimum sample size must be between 1 and 1000".into()));
    }
    repo::set_setting(conn, MIN_SAMPLE_KEY, &n.to_string())?;
    Ok(n)
}

/// Below the minimum sample every proposal is flagged and its confidence capped, so a handful of posts
/// never reads as a trend (LRN-11).
pub fn guard_confidence(raw: f64, sample_size: usize, min_sample: usize) -> (f64, bool) {
    let raw = if raw.is_finite() { raw.clamp(0.0, 1.0) } else { 0.0 };
    if sample_size < min_sample {
        (raw.min(LOW_CONFIDENCE_CAP), true)
    } else {
        (raw.min(CONFIDENCE_CAP), false)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Insight {
    pub id: String,
    pub cycle_date: String,
    pub category: String,
    pub hypothesis: String,
    pub evidence: Evidence,
    pub confidence: f64,
    pub patch: Patch,
    pub original_patch: Option<Patch>,
    pub edited: bool,
    pub status: String,
    pub decided_at: Option<String>,
    pub created_at: String,
    pub sample_size: i64,
    pub low_confidence: bool,
    pub applied_version: Option<i64>,
    pub source: String,
    pub diff: Vec<DiffLine>,
    /// The change no longer applies cleanly (for example it is already part of the active version).
    pub stale: bool,
}

/// Stores a proposal as `proposed`. Returns `None` when the same finding was already raised before,
/// whatever its status, so a rejected idea does not keep coming back.
pub fn insert_draft(conn: &Connection, profile_id: &str, d: &InsightDraft, sample_size: usize, min_sample: usize, cycle_date: &str) -> AppResult<Option<String>> {
    let patch = d.patch.clone().validated()?;
    let (confidence, low) = guard_confidence(d.confidence, sample_size, min_sample);
    let id = new_id();
    let n = conn.execute(
        "INSERT OR IGNORE INTO learning_insights
           (id, profile_id, cycle_date, category, hypothesis, evidence_json, confidence, proposed_patch_json, status,
            created_at, original_patch_json, sample_size, low_confidence, source, dedupe_key)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,'proposed',?9,NULL,?10,?11,?12,?13)",
        params![
            id,
            profile_id,
            cycle_date,
            d.category,
            d.hypothesis,
            serde_json::to_string(&d.evidence).expect("evidence serializes"),
            confidence,
            serde_json::to_string(&patch).expect("patch serializes"),
            now_rfc3339(),
            sample_size as i64,
            low as i64,
            d.source,
            d.dedupe_key,
        ],
    )?;
    Ok((n > 0).then_some(id))
}

const COLS: &str = "id, cycle_date, category, hypothesis, evidence_json, confidence, proposed_patch_json, original_patch_json, status,
                    decided_at, created_at, sample_size, low_confidence, applied_version, source";

fn row_to_insight(conn: &Connection, profile_id: &str, r: &rusqlite::Row<'_>) -> AppResult<Insight> {
    let evidence: Evidence = serde_json::from_str(&r.get::<_, String>(4)?).map_err(|e| AppError::Other(format!("corrupt insight evidence: {e}")))?;
    let patch: Patch = serde_json::from_str(&r.get::<_, String>(6)?).map_err(|e| AppError::Other(format!("corrupt insight patch: {e}")))?;
    let original: Option<Patch> = r
        .get::<_, Option<String>>(7)?
        .map(|s| serde_json::from_str(&s))
        .transpose()
        .map_err(|e| AppError::Other(format!("corrupt insight patch: {e}")))?;
    let status: String = r.get(8)?;
    let (diff, stale) = if status == "proposed" {
        match patch::preview(conn, profile_id, &patch) {
            Ok(d) => (d, false),
            Err(_) => (vec![], true),
        }
    } else {
        (vec![], false)
    };
    Ok(Insight {
        id: r.get(0)?,
        cycle_date: r.get(1)?,
        category: r.get(2)?,
        hypothesis: r.get(3)?,
        evidence,
        confidence: r.get(5)?,
        edited: original.is_some(),
        original_patch: original,
        patch,
        status,
        decided_at: r.get(9)?,
        created_at: r.get(10)?,
        sample_size: r.get(11)?,
        low_confidence: r.get::<_, i64>(12)? != 0,
        applied_version: r.get(13)?,
        source: r.get(14)?,
        diff,
        stale,
    })
}

/// `statuses` empty means all. Proposed first, then newest.
pub fn list(conn: &Connection, profile_id: &str, statuses: &[&str]) -> AppResult<Vec<Insight>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM learning_insights WHERE profile_id=?1
         ORDER BY CASE status WHEN 'proposed' THEN 0 ELSE 1 END, created_at DESC, id"
    ))?;
    let mut rows = stmt.query([profile_id])?;
    let mut out = vec![];
    while let Some(r) = rows.next()? {
        let ins = row_to_insight(conn, profile_id, r)?;
        if statuses.is_empty() || statuses.contains(&ins.status.as_str()) {
            out.push(ins);
        }
    }
    Ok(out)
}

pub fn get(conn: &Connection, profile_id: &str, id: &str) -> AppResult<Insight> {
    let mut stmt = conn.prepare(&format!("SELECT {COLS} FROM learning_insights WHERE profile_id=?1 AND id=?2"))?;
    let mut rows = stmt.query(params![profile_id, id])?;
    match rows.next()? {
        Some(r) => row_to_insight(conn, profile_id, r),
        None => Err(AppError::Validation("that insight no longer exists".into())),
    }
}

pub fn proposed_count(conn: &Connection, profile_id: &str) -> AppResult<i64> {
    Ok(conn.query_row("SELECT count(*) FROM learning_insights WHERE profile_id=?1 AND status='proposed'", [profile_id], |r| r.get(0))?)
}

/// Applies an insight's patch after the user approved it, optionally with their own edits to the patch.
/// The patch must keep its target (a rubric proposal stays a rubric proposal).
pub fn accept(conn: &Connection, profile_id: &str, insight_id: &str, edited_patch: Option<Patch>) -> AppResult<AppliedChange> {
    let tx = conn.unchecked_transaction()?;
    let ins = get(&tx, profile_id, insight_id)?;
    if ins.status != "proposed" {
        return Err(AppError::Validation(format!("this insight was already {}", ins.status)));
    }
    let final_patch = match edited_patch {
        Some(p) => {
            let p = p.validated()?;
            if p.target != ins.patch.target {
                return Err(AppError::Validation("an edited change must keep the same target".into()));
            }
            p
        }
        None => ins.patch.clone(),
    };
    let changed = final_patch != ins.patch;
    let change = patch::apply(&tx, profile_id, &final_patch, Some(insight_id))?;
    tx.execute(
        "UPDATE learning_insights SET status='accepted', decided_at=?1, applied_version=?2, proposed_patch_json=?3,
                original_patch_json=CASE WHEN ?4 THEN ?5 ELSE original_patch_json END
         WHERE id=?6 AND profile_id=?7",
        params![
            now_rfc3339(),
            change.to_version,
            serde_json::to_string(&final_patch).expect("patch serializes"),
            changed,
            serde_json::to_string(&ins.patch).expect("patch serializes"),
            insight_id,
            profile_id
        ],
    )?;
    tx.commit()?;
    Ok(change)
}

pub fn reject(conn: &Connection, profile_id: &str, insight_id: &str) -> AppResult<()> {
    let n = conn.execute(
        "UPDATE learning_insights SET status='rejected', decided_at=?1 WHERE id=?2 AND profile_id=?3 AND status='proposed'",
        params![now_rfc3339(), insight_id, profile_id],
    )?;
    if n == 0 {
        let exists: Option<String> = conn
            .query_row("SELECT status FROM learning_insights WHERE id=?1 AND profile_id=?2", params![insight_id, profile_id], |r| r.get(0))
            .optional()?;
        return Err(AppError::Validation(match exists {
            Some(s) => format!("this insight was already {s}"),
            None => "that insight no longer exists".into(),
        }));
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::learning::analyzers::EvidenceItem;
    use crate::learning::patch::tests::db;
    use crate::learning::patch::{active_style, PatchOp, PatchTarget};

    pub(crate) fn draft(key: &str, text: &str) -> InsightDraft {
        InsightDraft {
            category: "anti_pattern",
            hypothesis: format!("You delete \"{text}\" often"),
            evidence: Evidence { support: 4, total: 5, items: vec![EvidenceItem { label: "removed".into(), draft_id: Some("d1".into()), detail: String::new() }] },
            confidence: 0.9,
            patch: Patch { target: PatchTarget::Style, ops: vec![PatchOp::AddBanned { pattern: text.into() }] },
            dedupe_key: key.into(),
            source: "rules",
        }
    }

    #[test]
    fn low_sample_labels_and_caps_confidence() {
        assert_eq!(guard_confidence(0.9, 10, 30), (0.5, true));
        assert_eq!(guard_confidence(0.9, 30, 30), (0.9, false));
        assert_eq!(guard_confidence(2.0, 99, 30), (0.95, false));
        assert_eq!(guard_confidence(f64::NAN, 99, 30), (0.0, false));
        let (_d, c, pid) = db();
        assert_eq!(min_sample(&c).unwrap(), 30);
        set_min_sample(&c, 5).unwrap();
        assert_eq!(min_sample(&c).unwrap(), 5);
        assert!(set_min_sample(&c, 0).is_err());
        let id = insert_draft(&c, &pid, &draft("k1", "great point"), 4, 5, "2026-01-01").unwrap().unwrap();
        let ins = get(&c, &pid, &id).unwrap();
        assert!(ins.low_confidence);
        assert_eq!(ins.confidence, 0.5);
        assert_eq!(ins.sample_size, 4);
    }

    #[test]
    fn duplicates_are_ignored_even_after_a_decision() {
        let (_d, c, pid) = db();
        let id = insert_draft(&c, &pid, &draft("k1", "great point"), 40, 30, "d").unwrap().unwrap();
        assert!(insert_draft(&c, &pid, &draft("k1", "great point"), 40, 30, "d").unwrap().is_none());
        reject(&c, &pid, &id).unwrap();
        assert!(insert_draft(&c, &pid, &draft("k1", "great point"), 40, 30, "d").unwrap().is_none());
    }

    #[test]
    fn nothing_changes_until_a_human_accepts() {
        let (_d, c, pid) = db();
        let id = insert_draft(&c, &pid, &draft("k1", "great point"), 40, 30, "d").unwrap().unwrap();
        let before = active_style(&c, &pid).unwrap();
        assert_eq!(before.version, None);
        let ins = get(&c, &pid, &id).unwrap();
        assert_eq!(ins.diff.iter().filter(|l| l.kind == "add").count(), 1, "the card shows the exact change");
        let n: i64 = c.query_row("SELECT count(*) FROM style_guides", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);

        let ch = accept(&c, &pid, &id, None).unwrap();
        assert_eq!(ch.kind, "style");
        assert_eq!(active_style(&c, &pid).unwrap().banned.last().map(String::as_str), Some("great point"));
        let ins = get(&c, &pid, &id).unwrap();
        assert_eq!((ins.status.as_str(), ins.applied_version), ("accepted", ch.to_version));
        assert!(!ins.edited);
        assert!(accept(&c, &pid, &id, None).is_err(), "cannot approve twice");
        assert!(reject(&c, &pid, &id).is_err());
    }

    #[test]
    fn rejecting_leaves_everything_untouched() {
        let (_d, c, pid) = db();
        let id = insert_draft(&c, &pid, &draft("k1", "great point"), 40, 30, "d").unwrap().unwrap();
        let before = active_style(&c, &pid).unwrap();
        reject(&c, &pid, &id).unwrap();
        assert_eq!(get(&c, &pid, &id).unwrap().status, "rejected");
        assert!(accept(&c, &pid, &id, None).is_err());
        assert_eq!(active_style(&c, &pid).unwrap(), before);
        assert_eq!(proposed_count(&c, &pid).unwrap(), 0);
    }

    #[test]
    fn edited_accept_applies_the_users_version_and_keeps_the_original() {
        let (_d, c, pid) = db();
        let id = insert_draft(&c, &pid, &draft("k1", "great point"), 40, 30, "d").unwrap().unwrap();
        let mine = Patch { target: PatchTarget::Style, ops: vec![PatchOp::AddBanned { pattern: "great point!".into() }, PatchOp::AddRule { text: "No flattery".into() }] };
        accept(&c, &pid, &id, Some(mine.clone())).unwrap();
        let s = active_style(&c, &pid).unwrap();
        assert!(s.banned.contains(&"great point!".to_string()));
        assert_eq!(s.rules, vec!["No flattery".to_string()]);
        let ins = get(&c, &pid, &id).unwrap();
        assert!(ins.edited);
        assert_eq!(ins.patch, mine);
        assert_eq!(ins.original_patch.unwrap().ops.len(), 1);

        let id2 = insert_draft(&c, &pid, &draft("k2", "love this"), 40, 30, "d").unwrap().unwrap();
        let wrong = Patch { target: PatchTarget::Rubric, ops: vec![PatchOp::AddCriterion { text: "x".into() }] };
        assert!(accept(&c, &pid, &id2, Some(wrong)).is_err());
        assert_eq!(get(&c, &pid, &id2).unwrap().status, "proposed", "a failed accept rolls back completely");
    }

    #[test]
    fn a_failing_apply_does_not_mark_the_insight_accepted() {
        let (_d, c, pid) = db();
        let id = insert_draft(&c, &pid, &draft("k1", "great point"), 40, 30, "d").unwrap().unwrap();
        let id2 = insert_draft(&c, &pid, &draft("k2", "Great Point"), 40, 30, "d").unwrap().unwrap();
        accept(&c, &pid, &id, None).unwrap();
        assert!(get(&c, &pid, &id2).unwrap().stale);
        assert!(accept(&c, &pid, &id2, None).is_err());
        assert_eq!(get(&c, &pid, &id2).unwrap().status, "proposed");
    }

    #[test]
    fn list_puts_proposals_first() {
        let (_d, c, pid) = db();
        let a = insert_draft(&c, &pid, &draft("k1", "aaa bbb"), 40, 30, "d").unwrap().unwrap();
        insert_draft(&c, &pid, &draft("k2", "ccc ddd"), 40, 30, "d").unwrap().unwrap();
        reject(&c, &pid, &a).unwrap();
        let all = list(&c, &pid, &[]).unwrap();
        assert_eq!(all[0].status, "proposed");
        assert_eq!(list(&c, &pid, &["rejected"]).unwrap().len(), 1);
    }
}
