//! OGC creator (Phase 5): turns a viral format plus the user's rough thought into three drafts in their voice,
//! checks them for originality, and publishes one only after an explicit click. Additive: own tables, own
//! publisher, and the model behind the [`StructuredLlm`] seam.

pub mod commands;
pub mod compose;
pub mod drafts;
pub mod similarity;

use crate::error::{AppError, AppResult};
use crate::formats::llm::StructuredLlm;
use crate::formats::repo;
use crate::storage::repo as settings;
use drafts::{NewDraft, OgcDraft};
use rusqlite::Connection;
use serde::Serialize;
use std::sync::Mutex;

const SIMILARITY_KEY: &str = "ogc_similarity_threshold";
const MAX_CHARS_KEY: &str = "ogc_max_chars";
pub const DEFAULT_MAX_CHARS: usize = 280;
const MAX_CHARS_CEILING: usize = 25_000;
const TOPIC_LIMIT: usize = 1_000;

#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize)]
pub struct CreatorSettings {
    pub similarity_threshold: f64,
    pub max_chars: usize,
}

pub fn get_settings(conn: &Connection) -> AppResult<CreatorSettings> {
    let threshold = settings::get_setting(conn, SIMILARITY_KEY)?
        .and_then(|v| v.parse::<f64>().ok())
        .and_then(|v| similarity::validate_threshold(v).ok())
        .unwrap_or(similarity::DEFAULT_THRESHOLD);
    let max_chars = settings::get_setting(conn, MAX_CHARS_KEY)?
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|v| (1..=MAX_CHARS_CEILING).contains(v))
        .unwrap_or(DEFAULT_MAX_CHARS);
    Ok(CreatorSettings { similarity_threshold: threshold, max_chars })
}

pub fn save_settings(conn: &Connection, s: &CreatorSettings) -> AppResult<CreatorSettings> {
    let t = similarity::validate_threshold(s.similarity_threshold).map_err(AppError::Validation)?;
    if !(1..=MAX_CHARS_CEILING).contains(&s.max_chars) {
        return Err(AppError::Validation(format!("the character limit must be between 1 and {MAX_CHARS_CEILING}")));
    }
    settings::set_setting(conn, SIMILARITY_KEY, &t.to_string())?;
    settings::set_setting(conn, MAX_CHARS_KEY, &s.max_chars.to_string())?;
    get_settings(conn)
}

fn check(conn: &Connection, text: &str, format_id: Option<&str>, profile_id: &str) -> AppResult<Option<f64>> {
    let Some(fid) = format_id else { return Ok(None) };
    let Some(f) = repo::get_format(conn, profile_id, fid)? else { return Ok(None) };
    let t = get_settings(conn)?.similarity_threshold;
    Ok(Some(similarity::check(text, &f.example_texts, t).score))
}

#[derive(Debug, Serialize)]
pub struct Generated {
    pub batch_id: String,
    pub drafts: Vec<OgcDraft>,
}

/// The topic when the user gave none: their niche topics from Preferences, else a neutral prompt.
fn default_topic(conn: &Connection, profile_id: &str) -> String {
    let topics = settings::get_preferences(conn, profile_id).ok().flatten().map(|p| p.niche_topics).unwrap_or_default();
    if topics.is_empty() {
        "something useful I learned recently from my own work".to_string()
    } else {
        format!("one of my usual topics: {}", topics.join(", "))
    }
}

/// Generates three variants for `format_id` and stores them. Nothing leaves the machine except the model call.
/// An empty `topic` falls back to the user's niche topics, so a template alone is enough to get examples.
pub async fn generate_variants(
    db: &Mutex<Connection>,
    profile_id: &str,
    llm: &dyn StructuredLlm,
    format_id: &str,
    topic: &str,
) -> AppResult<Generated> {
    generate_with_label(db, profile_id, llm, format_id, topic, topic).await
}

/// The brief for "Create a variation": same hook and structure as the source post, new angle and wording.
pub fn variation_brief(source_text: &str) -> String {
    let source: String = source_text.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(600).collect();
    format!(
        "Write a variation of the post below. Keep its hook style and structure, but change the angle, the example and the wording so it is clearly a new post and not a paraphrase.\nPost to vary: {source}"
    )
}

/// Turns one viral post into three similar-but-different drafts. The post's format is extracted first when
/// it has none yet, so this needs the format-extraction model once per post.
pub async fn generate_variation(
    db: &Mutex<Connection>,
    profile_id: &str,
    llm: &dyn StructuredLlm,
    candidate_id: &str,
) -> AppResult<Generated> {
    let candidate = {
        let conn = db.lock().unwrap();
        repo::get_candidate(&conn, profile_id, candidate_id)?.ok_or_else(|| AppError::Validation("that post is no longer in your list".into()))?
    };
    let format_id = match candidate.format_id.clone() {
        Some(id) => id,
        None => crate::formats::pipeline::process_candidate(db, profile_id, llm, candidate_id).await?.format.id,
    };
    let label = format!("Variation of @{}", candidate.author_handle);
    generate_with_label(db, profile_id, llm, &format_id, &variation_brief(&candidate.text), &label).await
}

async fn generate_with_label(
    db: &Mutex<Connection>,
    profile_id: &str,
    llm: &dyn StructuredLlm,
    format_id: &str,
    topic: &str,
    label: &str,
) -> AppResult<Generated> {
    let topic = topic.trim();
    let topic = if topic.is_empty() { default_topic(&db.lock().unwrap(), profile_id) } else { topic.to_string() };
    let topic = topic.as_str();
    if topic.chars().count() > TOPIC_LIMIT {
        return Err(AppError::Validation(format!("keep the topic under {TOPIC_LIMIT} characters")));
    }
    let label = if label.trim().is_empty() { "From a template" } else { label.trim() };
    let (format, style, cfg) = {
        let conn = db.lock().unwrap();
        let format = repo::get_format(&conn, profile_id, format_id)?.ok_or_else(|| AppError::Validation("format not found".into()))?;
        if format.status != "active" {
            return Err(AppError::Validation("this format is archived; restore it to use it".into()));
        }
        (format, compose::load_style(&conn, profile_id)?, get_settings(&conn)?)
    };
    let variants = compose::generate(llm, &format, topic, &style, cfg.max_chars)
        .await
        .map_err(|e| AppError::Other(e.to_string()))?;

    let batch_id = crate::util::new_id();
    let conn = db.lock().unwrap();
    let mut out = Vec::new();
    for text in variants {
        let sim = similarity::check(&text, &format.example_texts, cfg.similarity_threshold).score;
        out.push(drafts::insert(
            &conn,
            profile_id,
            &NewDraft {
                format_id: Some(&format.id),
                batch_id: Some(&batch_id),
                topic_seed: label,
                text: &text,
                model_used: "ogc_generation",
                status: "variant",
                similarity: Some(sim),
            },
        )?);
    }
    Ok(Generated { batch_id, drafts: out })
}

/// A blank draft the user writes themselves (a format is optional context).
pub fn create_blank(conn: &Connection, profile_id: &str, format_id: Option<&str>, text: &str) -> AppResult<OgcDraft> {
    let sim = check(conn, text, format_id, profile_id)?;
    drafts::insert(
        conn,
        profile_id,
        &NewDraft { format_id, batch_id: None, topic_seed: "", text, model_used: "manual", status: "draft", similarity: sim },
    )
}

pub fn edit_draft(conn: &Connection, profile_id: &str, id: &str, text: &str) -> AppResult<OgcDraft> {
    let format_id = drafts::get(conn, profile_id, id)?.and_then(|d| d.viral_format_id);
    let sim = check(conn, text, format_id.as_deref(), profile_id)?;
    drafts::update_text(conn, profile_id, id, text, sim)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formats::blueprint::FormatBlueprint;
    use crate::formats::llm::testing::ScriptedLlm;
    use crate::formats::llm::UnconfiguredLlm;
    use crate::formats::repo::testing::db;

    fn seed_format(conn: &Connection, pid: &str) -> String {
        let bp = FormatBlueprint {
            name: "Contrarian take".into(),
            archetype: "contrarian_take".into(),
            hook_formula: "Reject a popular belief".into(),
            structural_pattern: "hook then reason".into(),
            skeleton: "Unpopular opinion: {belief} is wrong. {reason}".into(),
            slots: vec![
                crate::formats::blueprint::Slot { key: "belief".into(), label: "Belief".into(), hint: String::new() },
                crate::formats::blueprint::Slot { key: "reason".into(), label: "Reason".into(), hint: String::new() },
            ],
            media_hint: String::new(),
        };
        repo::insert_format(conn, pid, &bp, "detected", None, Some("Unpopular opinion: hustle culture is wrong. It burns people out.")).unwrap().id
    }

    const REPLY: &str = r#"{"variants":[{"text":"Unpopular opinion: hustle culture is wrong. It burns people out."},{"text":"Sleep is a growth strategy for founders."},{"text":"Most roadmaps fail because nobody says no."}]}"#;

    #[tokio::test]
    async fn generates_three_stored_variants_with_similarity_scores() {
        let (conn, pid, _d) = db();
        let fid = seed_format(&conn, &pid);
        let m = Mutex::new(conn);
        let llm = ScriptedLlm::with(vec![Ok(REPLY)]);
        let g = generate_variants(&m, &pid, &llm, &fid, "why founders need sleep").await.unwrap();
        assert_eq!(g.drafts.len(), 3);
        assert!(g.drafts.iter().all(|d| d.status == "variant" && d.batch_id.as_deref() == Some(g.batch_id.as_str())));
        assert!(g.drafts[0].similarity.unwrap() > 0.8, "a copy of the example scores high");
        assert!(g.drafts[1].similarity.unwrap() < 0.2);
        let reqs = llm.requests.lock().unwrap();
        assert_eq!(reqs[0].task, "ogc_generation");
        assert!(reqs[0].user.contains("why founders need sleep"));
    }

    #[tokio::test]
    async fn an_empty_topic_uses_niche_topics_and_a_variation_carries_the_source_post() {
        let (conn, pid, _d) = db();
        let fid = seed_format(&conn, &pid);
        conn.execute("UPDATE format_candidates SET format_id = NULL WHERE 0", []).ok();
        let cand = repo::insert_candidate(
            &conn,
            &pid,
            &repo::NewCandidate {
                platform_post_id: "7".into(),
                author_handle: "alice".into(),
                author_followers: Some(1000),
                text: "Unpopular opinion: standups are waste.".into(),
                metrics: Default::default(),
                posted_at: None,
                source: "manual",
                virality_score: 80.0,
                reasons: vec![],
                author_name: "Alice".into(),
                avatar_url: None,
                media: vec![],
            },
        )
        .unwrap()
        .unwrap();
        repo::set_candidate_status(&conn, &pid, &cand.id, "extracted", Some(&fid), None).unwrap();
        let m = Mutex::new(conn);
        let llm = ScriptedLlm::with(vec![Ok(REPLY), Ok(REPLY)]);
        let g = generate_variants(&m, &pid, &llm, &fid, "  ").await.unwrap();
        assert_eq!(g.drafts.len(), 3);
        let v = generate_variation(&m, &pid, &llm, &cand.id).await.unwrap();
        assert_eq!(v.drafts[0].topic_seed, "Variation of @alice");
        let reqs = llm.requests.lock().unwrap();
        assert!(reqs[0].user.contains("something useful I learned"));
        assert!(reqs[1].user.contains("Post to vary: Unpopular opinion: standups are waste."));
    }

    #[tokio::test]
    async fn without_a_model_nothing_is_stored_and_the_message_is_clear() {
        let (conn, pid, _d) = db();
        let fid = seed_format(&conn, &pid);
        let m = Mutex::new(conn);
        let err = generate_variants(&m, &pid, &UnconfiguredLlm, &fid, "topic").await.unwrap_err();
        assert!(err.to_string().contains("No model is chosen"));
        let n: i64 = m.lock().unwrap().query_row("SELECT COUNT(*) FROM ogc_drafts", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
    }

    #[tokio::test]
    async fn rejects_empty_topics_unknown_and_archived_formats() {
        let (conn, pid, _d) = db();
        let fid = seed_format(&conn, &pid);
        repo::set_format_status(&conn, &pid, &fid, "archived").unwrap();
        let m = Mutex::new(conn);
        let llm = ScriptedLlm::with(vec![]);
        assert!(generate_variants(&m, &pid, &llm, &fid, "  ").await.is_err());
        assert!(generate_variants(&m, &pid, &llm, "nope", "topic").await.is_err());
        assert!(generate_variants(&m, &pid, &llm, &fid, "topic").await.unwrap_err().to_string().contains("archived"));
    }

    #[test]
    fn settings_default_validate_and_persist() {
        let (conn, _pid, _d) = db();
        assert_eq!(get_settings(&conn).unwrap(), CreatorSettings { similarity_threshold: 0.5, max_chars: 280 });
        assert!(save_settings(&conn, &CreatorSettings { similarity_threshold: 0.99, max_chars: 280 }).is_err());
        assert!(save_settings(&conn, &CreatorSettings { similarity_threshold: 0.5, max_chars: 0 }).is_err());
        let s = save_settings(&conn, &CreatorSettings { similarity_threshold: 0.7, max_chars: 4000 }).unwrap();
        assert_eq!((s.similarity_threshold, s.max_chars), (0.7, 4000));
    }

    #[test]
    fn editing_rescores_similarity_and_records_the_edit() {
        let (conn, pid, _d) = db();
        let fid = seed_format(&conn, &pid);
        let d = create_blank(&conn, &pid, Some(&fid), "Totally different words about pricing pages.").unwrap();
        assert!(d.similarity.unwrap() < 0.2);
        let e = edit_draft(&conn, &pid, &d.id, "Unpopular opinion: hustle culture is wrong. It burns people out.").unwrap();
        assert!(e.similarity.unwrap() > 0.8);
        assert!(e.edit_distance > 0);
    }
}
