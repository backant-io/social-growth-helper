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

/// Generates three variants for `format_id` and stores them. Nothing leaves the machine except the model call.
pub async fn generate_variants(
    db: &Mutex<Connection>,
    profile_id: &str,
    llm: &dyn StructuredLlm,
    format_id: &str,
    topic: &str,
) -> AppResult<Generated> {
    let topic = topic.trim();
    if topic.is_empty() {
        return Err(AppError::Validation("write a rough thought or topic first".into()));
    }
    if topic.chars().count() > TOPIC_LIMIT {
        return Err(AppError::Validation(format!("keep the topic under {TOPIC_LIMIT} characters")));
    }
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
                topic_seed: topic,
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
