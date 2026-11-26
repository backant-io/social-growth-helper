//! Continuous detection (OGC-01, OGC-06) and the candidate-to-format pipeline.
//!
//! [`consider_post`] is the hook every post source calls: Scout for each post it extracts, a watchlist
//! scan, or a manual add. It costs one arithmetic check and one insert, so it can run on every post.
//! Model calls happen only in [`process_candidate`], on demand.

use super::cluster::{find_match, MERGE_THRESHOLD};
use super::detect::{evaluate, PostMetrics, PostSignals, ViralThresholds, ViralVerdict};
use super::extract::{extract_blueprint, ExtractError};
use super::llm::{LlmError, StructuredLlm};
use super::repo::{self, Candidate, FormatRecord, FormatSource, NewCandidate};
use crate::error::{AppError, AppResult};
use rusqlite::Connection;
use serde::Serialize;
use std::sync::Mutex;

#[derive(Debug, Clone)]
pub struct PostInput {
    pub platform_post_id: String,
    pub author_handle: String,
    pub author_followers: Option<u64>,
    pub text: String,
    pub metrics: PostMetrics,
    pub posted_at: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostSource {
    Scout,
    Watchlist,
    Manual,
}

impl PostSource {
    fn as_str(self) -> &'static str {
        match self {
            Self::Scout => "scout",
            Self::Watchlist => "watchlist",
            Self::Manual => "manual",
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Detection {
    pub verdict: ViralVerdict,
    /// `None` when the post was not flagged, or was already stored.
    pub candidate: Option<Candidate>,
    pub already_known: bool,
}

/// Flags the post when it over-performs and stores it as a candidate. With `force` (a manual add) the post
/// is stored even below the thresholds, because the user decided it is worth studying.
pub fn consider_post(
    conn: &Connection,
    profile_id: &str,
    post: &PostInput,
    source: PostSource,
    thresholds: &ViralThresholds,
    force: bool,
) -> AppResult<Detection> {
    if post.text.trim().is_empty() {
        return Err(AppError::Validation("the post has no text to study".into()));
    }
    if post.platform_post_id.trim().is_empty() {
        return Err(AppError::Validation("the post needs an id".into()));
    }
    let verdict = evaluate(&PostSignals { author_followers: post.author_followers, metrics: post.metrics.clone() }, thresholds);
    if !verdict.flagged && !force {
        return Ok(Detection { verdict, candidate: None, already_known: false });
    }
    let mut reasons = verdict.reasons.clone();
    if !verdict.flagged {
        reasons.push("Added by hand; it is below the automatic viral thresholds.".into());
    }
    let candidate = repo::insert_candidate(
        conn,
        profile_id,
        &NewCandidate {
            platform_post_id: post.platform_post_id.trim().to_string(),
            author_handle: post.author_handle.trim().trim_start_matches('@').to_string(),
            author_followers: post.author_followers,
            text: post.text.trim().to_string(),
            metrics: post.metrics.clone(),
            posted_at: post.posted_at.clone(),
            source: source.as_str(),
            virality_score: verdict.score,
            reasons,
        },
    )?;
    let already_known = candidate.is_none();
    Ok(Detection { verdict, candidate, already_known })
}

#[derive(Debug, Serialize)]
pub struct Processed {
    pub candidate_id: String,
    pub format: FormatRecord,
    /// True when the post matched a format already in the library instead of creating a new one.
    pub merged: bool,
    pub similarity: Option<f64>,
}

/// Extracts a blueprint for one pending candidate and stores or merges it. The database lock is never held
/// across the model call.
pub async fn process_candidate(
    db: &Mutex<Connection>,
    profile_id: &str,
    llm: &dyn StructuredLlm,
    candidate_id: &str,
) -> Result<Processed, AppError> {
    let candidate = {
        let conn = db.lock().unwrap();
        repo::get_candidate(&conn, profile_id, candidate_id)?.ok_or_else(|| AppError::Validation("candidate not found".into()))?
    };
    if candidate.status == "extracted" {
        return Err(AppError::Validation("this post has already been turned into a format".into()));
    }

    let blueprint = match extract_blueprint(llm, &candidate.text).await {
        Ok(bp) => bp,
        Err(e) => {
            // A missing model is the user's to fix, not a property of the post, so the candidate stays pending.
            if !matches!(e, ExtractError::Llm(LlmError::NoModel(_))) {
                let conn = db.lock().unwrap();
                repo::set_candidate_status(&conn, profile_id, candidate_id, "failed", None, Some(&e.to_string()))?;
            }
            return Err(AppError::Other(e.to_string()));
        }
    };

    let conn = db.lock().unwrap();
    let existing = repo::list_formats(&conn, profile_id, true, repo::FormatSort::Recent)?;
    let blueprints: Vec<_> = existing.iter().map(|f| f.blueprint()).collect();
    let source = FormatSource {
        post_id: candidate.platform_post_id.clone(),
        author: candidate.author_handle.clone(),
        url: candidate.url(),
        seen_at: crate::util::now_rfc3339(),
        score: candidate.virality_score,
    };
    let (format, merged, similarity) = match find_match(&blueprints, &blueprint, MERGE_THRESHOLD) {
        Some((i, score)) => {
            let f = repo::merge_sighting(&conn, profile_id, &existing[i].id, source, &candidate.text)?;
            (f, true, Some(score))
        }
        None => (repo::insert_format(&conn, profile_id, &blueprint, "detected", Some(source), Some(&candidate.text))?, false, None),
    };
    repo::set_candidate_status(&conn, profile_id, candidate_id, "extracted", Some(&format.id), None)?;
    Ok(Processed { candidate_id: candidate_id.to_string(), format, merged, similarity })
}

#[derive(Debug, Default, Serialize)]
pub struct BatchSummary {
    pub created: usize,
    pub merged: usize,
    pub failed: usize,
    /// Set when the batch stopped early because no model is chosen.
    pub stopped_reason: Option<String>,
}

pub async fn process_pending(db: &Mutex<Connection>, profile_id: &str, llm: &dyn StructuredLlm, limit: usize) -> AppResult<BatchSummary> {
    let pending = {
        let conn = db.lock().unwrap();
        repo::list_candidates(&conn, profile_id, "pending", limit)?
    };
    let mut summary = BatchSummary::default();
    for c in pending {
        match process_candidate(db, profile_id, llm, &c.id).await {
            Ok(p) if p.merged => summary.merged += 1,
            Ok(_) => summary.created += 1,
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("No model is chosen") {
                    summary.stopped_reason = Some(msg);
                    break;
                }
                summary.failed += 1;
            }
        }
    }
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formats::llm::testing::ScriptedLlm;
    use crate::formats::llm::UnconfiguredLlm;
    use crate::formats::repo::testing::db;

    fn post(id: &str, likes: u64, followers: u64) -> PostInput {
        PostInput {
            platform_post_id: id.into(),
            author_handle: "@alice".into(),
            author_followers: Some(followers),
            text: format!("Unpopular opinion: hustle {id} is wrong. It burns you out."),
            metrics: PostMetrics { likes, reposts: likes / 10, replies: likes / 20, bookmarks: likes / 8, views: None },
            posted_at: None,
        }
    }

    fn reply(skeleton_word: &str) -> String {
        format!(
            r#"{{"name":"Unpopular truth {skeleton_word}","archetype":"contrarian_take","hook_formula":"State a belief then say it is wrong","structural_pattern":"One line hook then one line twist","skeleton":"Unpopular opinion: {{belief}} is wrong. {{twist}}","slots":[{{"key":"belief","label":"Belief","hint":""}},{{"key":"twist","label":"Twist","hint":""}}],"media_hint":"text_only"}}"#
        )
    }

    #[test]
    fn only_over_performing_posts_become_candidates() {
        let (conn, pid, _d) = db();
        let t = ViralThresholds::default();
        let hit = consider_post(&conn, &pid, &post("1", 900, 2_000), PostSource::Scout, &t, false).unwrap();
        assert!(hit.verdict.flagged && hit.candidate.is_some());
        let miss = consider_post(&conn, &pid, &post("2", 30, 2_000), PostSource::Scout, &t, false).unwrap();
        assert!(!miss.verdict.flagged && miss.candidate.is_none());
        assert_eq!(repo::list_candidates(&conn, &pid, "pending", 10).unwrap().len(), 1);
    }

    #[test]
    fn manual_add_is_stored_below_threshold_and_dedupes() {
        let (conn, pid, _d) = db();
        let t = ViralThresholds::default();
        let d = consider_post(&conn, &pid, &post("3", 20, 2_000), PostSource::Manual, &t, true).unwrap();
        let c = d.candidate.unwrap();
        assert_eq!(c.author_handle, "alice");
        assert!(c.reasons.iter().any(|r| r.contains("by hand")));
        let again = consider_post(&conn, &pid, &post("3", 20, 2_000), PostSource::Manual, &t, true).unwrap();
        assert!(again.already_known && again.candidate.is_none());
    }

    #[test]
    fn empty_text_is_rejected() {
        let (conn, pid, _d) = db();
        let mut p = post("4", 900, 2_000);
        p.text = "  ".into();
        assert!(consider_post(&conn, &pid, &p, PostSource::Manual, &ViralThresholds::default(), true).is_err());
    }

    #[tokio::test]
    async fn extraction_creates_then_merges_equivalent_formats() {
        let (conn, pid, _d) = db();
        let t = ViralThresholds::default();
        for id in ["1", "2"] {
            consider_post(&conn, &pid, &post(id, 900, 2_000), PostSource::Scout, &t, false).unwrap();
        }
        let db = Mutex::new(conn);
        let llm = ScriptedLlm::with(vec![Ok(reply("one").as_str()), Ok(reply("two").as_str())]);
        let summary = process_pending(&db, &pid, &llm, 10).await.unwrap();
        assert_eq!((summary.created, summary.merged, summary.failed), (1, 1, 0));
        let conn = db.lock().unwrap();
        let formats = repo::list_formats(&conn, &pid, false, repo::FormatSort::Recent).unwrap();
        assert_eq!(formats.len(), 1);
        assert_eq!(formats[0].observation_count, 2);
        assert_eq!(formats[0].sources.len(), 2);
        assert!(repo::list_candidates(&conn, &pid, "pending", 10).unwrap().is_empty());
    }

    #[tokio::test]
    async fn an_unset_model_stops_the_batch_and_keeps_candidates_pending() {
        let (conn, pid, _d) = db();
        consider_post(&conn, &pid, &post("1", 900, 2_000), PostSource::Scout, &ViralThresholds::default(), false).unwrap();
        let db = Mutex::new(conn);
        let summary = process_pending(&db, &pid, &UnconfiguredLlm, 10).await.unwrap();
        assert!(summary.stopped_reason.unwrap().contains("No model is chosen"));
        let conn = db.lock().unwrap();
        assert_eq!(repo::list_candidates(&conn, &pid, "pending", 10).unwrap().len(), 1);
    }

    #[tokio::test]
    async fn bad_model_output_marks_the_candidate_failed() {
        let (conn, pid, _d) = db();
        consider_post(&conn, &pid, &post("1", 900, 2_000), PostSource::Scout, &ViralThresholds::default(), false).unwrap();
        let db = Mutex::new(conn);
        let llm = ScriptedLlm::with(vec![Ok("garbage"), Ok("garbage")]);
        let summary = process_pending(&db, &pid, &llm, 10).await.unwrap();
        assert_eq!(summary.failed, 1);
        let conn = db.lock().unwrap();
        let failed = repo::list_candidates(&conn, &pid, "failed", 10).unwrap();
        assert_eq!(failed.len(), 1);
        assert!(failed[0].error_message.is_some());
    }
}
