//! SQLite & FTS5 persistence for Style Guides and Few-Shot Examples (STY-01..03, STY-06, STY-09).

use super::{
    EmojiPolicy, ExampleKind, HashtagPolicy, RetrievedExample, StyleExample, StyleExampleInput, StyleGuide,
    StyleGuideInput,
};
use crate::error::{AppError, AppResult};
use crate::util::{new_id, now_rfc3339};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashSet;

pub fn default_style_guide(profile_id: &str) -> StyleGuide {
    StyleGuide {
        id: "default-style-guide".into(),
        profile_id: profile_id.to_string(),
        version: 0,
        free_text_guide: "Write crisp, peer-to-peer replies grounded in concrete experience. Lead with a specific observation, trade-off, or sharp follow-up question. Avoid hype, generic praise, and corporate filler.".into(),
        learned_rules: Vec::new(),
        banned_patterns: vec![
            "game changer".into(),
            "delve".into(),
            "unlock".into(),
            "let's dive in".into(),
            "great post!".into(),
        ],
        max_reply_chars: 220,
        emoji_policy: EmojiPolicy::Minimal,
        hashtag_policy: HashtagPolicy::None,
        note: Some("Starter guide (unsaved until you click Save New Version)".into()),
        is_active: true,
        created_at: String::new(),
    }
}

fn row_to_style_guide(row: &rusqlite::Row<'_>) -> rusqlite::Result<StyleGuide> {
    let learned_raw: String = row.get(4)?;
    let banned_raw: String = row.get(5)?;
    let emoji_raw: String = row.get(7)?;
    let hashtag_raw: String = row.get(8)?;
    Ok(StyleGuide {
        id: row.get(0)?,
        profile_id: row.get(1)?,
        version: row.get(2)?,
        free_text_guide: row.get(3)?,
        learned_rules: serde_json::from_str(&learned_raw).unwrap_or_default(),
        banned_patterns: serde_json::from_str(&banned_raw).unwrap_or_default(),
        max_reply_chars: row.get::<_, u32>(6)?,
        emoji_policy: EmojiPolicy::from_str(&emoji_raw).unwrap_or(EmojiPolicy::Minimal),
        hashtag_policy: HashtagPolicy::from_str(&hashtag_raw).unwrap_or(HashtagPolicy::None),
        note: row.get(9)?,
        is_active: row.get::<_, i64>(10)? != 0,
        created_at: row.get(11)?,
    })
}

pub fn get_active_style_guide(conn: &Connection, profile_id: &str) -> AppResult<StyleGuide> {
    let row = conn
        .query_row(
            "SELECT id, profile_id, version, free_text_guide, learned_rules_json, banned_patterns_json,
                    max_reply_chars, emoji_policy, hashtag_policy, note, is_active, created_at
             FROM style_guides
             WHERE profile_id=?1 AND is_active=1
             ORDER BY version DESC LIMIT 1",
            [profile_id],
            row_to_style_guide,
        )
        .optional()?;
    Ok(row.unwrap_or_else(|| default_style_guide(profile_id)))
}

pub fn list_style_guide_versions(conn: &Connection, profile_id: &str) -> AppResult<Vec<StyleGuide>> {
    let mut stmt = conn.prepare(
        "SELECT id, profile_id, version, free_text_guide, learned_rules_json, banned_patterns_json,
                max_reply_chars, emoji_policy, hashtag_policy, note, is_active, created_at
         FROM style_guides
         WHERE profile_id=?1
         ORDER BY version DESC",
    )?;
    let rows = stmt
        .query_map([profile_id], row_to_style_guide)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn normalize_list(items: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for raw in items {
        let trimmed = raw.trim().to_string();
        if trimmed.is_empty() {
            continue;
        }
        let key = trimmed.to_lowercase();
        if seen.insert(key) {
            out.push(trimmed);
        }
    }
    out
}

pub fn save_style_guide_version(
    conn: &Connection,
    profile_id: &str,
    input: StyleGuideInput,
) -> AppResult<StyleGuide> {
    let text = input.free_text_guide.trim();
    if text.is_empty() {
        return Err(AppError::Validation("Style guide prose cannot be empty".into()));
    }
    if !(40..=2000).contains(&input.max_reply_chars) {
        return Err(AppError::Validation(
            "max_reply_chars must be between 40 and 2000".into(),
        ));
    }

    let learned = normalize_list(input.learned_rules);
    let banned = normalize_list(input.banned_patterns);
    let note = input
        .note
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string);

    let max_ver: u32 = conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM style_guides WHERE profile_id=?1",
        [profile_id],
        |r| r.get(0),
    )?;
    let next_ver = max_ver + 1;
    let id = new_id();
    let now = now_rfc3339();

    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE style_guides SET is_active=0 WHERE profile_id=?1 AND is_active=1",
        [profile_id],
    )?;
    tx.execute(
        "INSERT INTO style_guides
         (id, profile_id, version, free_text_guide, learned_rules_json, banned_patterns_json,
          max_reply_chars, emoji_policy, hashtag_policy, note, is_active, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 1, ?11)",
        params![
            id,
            profile_id,
            next_ver,
            text,
            serde_json::to_string(&learned).unwrap_or_else(|_| "[]".into()),
            serde_json::to_string(&banned).unwrap_or_else(|_| "[]".into()),
            input.max_reply_chars,
            input.emoji_policy.as_str(),
            input.hashtag_policy.as_str(),
            note,
            now,
        ],
    )?;
    tx.commit()?;
    get_active_style_guide(conn, profile_id)
}

pub fn activate_style_guide_version(
    conn: &Connection,
    profile_id: &str,
    version: u32,
) -> AppResult<StyleGuide> {
    let exists: i64 = conn.query_row(
        "SELECT COUNT(*) FROM style_guides WHERE profile_id=?1 AND version=?2",
        params![profile_id, version],
        |r| r.get(0),
    )?;
    if exists == 0 {
        return Err(AppError::Validation(format!(
            "Style guide version v{version} not found"
        )));
    }
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE style_guides SET is_active=0 WHERE profile_id=?1",
        [profile_id],
    )?;
    tx.execute(
        "UPDATE style_guides SET is_active=1 WHERE profile_id=?1 AND version=?2",
        params![profile_id, version],
    )?;
    tx.commit()?;
    get_active_style_guide(conn, profile_id)
}

fn row_to_example(row: &rusqlite::Row<'_>) -> rusqlite::Result<StyleExample> {
    let kind_raw: String = row.get(2)?;
    let tags_raw: String = row.get(6)?;
    Ok(StyleExample {
        id: row.get(0)?,
        profile_id: row.get(1)?,
        kind: ExampleKind::from_str(&kind_raw).unwrap_or(ExampleKind::Reply),
        source_context_text: row.get(3)?,
        example_text: row.get(4)?,
        why_it_works: row.get(5)?,
        tags: serde_json::from_str(&tags_raw).unwrap_or_default(),
        pinned: row.get::<_, i64>(7)? != 0,
        source: row.get(8)?,
        performance_score: row.get(9)?,
        created_at: row.get(10)?,
    })
}

fn sync_example_fts(tx: &rusqlite::Transaction<'_>, ex: &StyleExample) -> rusqlite::Result<()> {
    tx.execute("DELETE FROM style_examples_fts WHERE example_id=?1", [&ex.id])?;
    tx.execute(
        "INSERT INTO style_examples_fts
         (example_id, profile_id, kind, source_context_text, example_text, why_it_works, tags_text)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            ex.id,
            ex.profile_id,
            ex.kind.as_str(),
            ex.source_context_text.as_deref().unwrap_or(""),
            ex.example_text,
            ex.why_it_works.as_deref().unwrap_or(""),
            ex.tags.join(" "),
        ],
    )?;
    Ok(())
}

pub fn upsert_style_example(
    conn: &Connection,
    profile_id: &str,
    input: StyleExampleInput,
) -> AppResult<StyleExample> {
    let example_text = input.example_text.trim().to_string();
    if example_text.is_empty() {
        return Err(AppError::Validation("Example text cannot be empty".into()));
    }
    let tags = normalize_list(input.tags);
    let source_context_text = input
        .source_context_text
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string);
    let why_it_works = input
        .why_it_works
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string);
    let source = input
        .source
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("manual")
        .to_string();

    let id = input
        .id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(new_id);
    let now = now_rfc3339();
    let tags_json = serde_json::to_string(&tags).unwrap_or_else(|_| "[]".into());

    let tx = conn.unchecked_transaction()?;
    let existing_created_at: Option<String> = tx
        .query_row(
            "SELECT created_at FROM style_examples WHERE id=?1 AND profile_id=?2",
            params![id, profile_id],
            |r| r.get(0),
        )
        .optional()?;

    let created_at = match existing_created_at {
        Some(ca) => {
            tx.execute(
                "UPDATE style_examples
                 SET kind=?2, source_context_text=?3, example_text=?4, why_it_works=?5,
                     tags_json=?6, pinned=?7, source=?8, performance_score=?9
                 WHERE id=?1 AND profile_id=?10",
                params![
                    id,
                    input.kind.as_str(),
                    source_context_text,
                    example_text,
                    why_it_works,
                    tags_json,
                    if input.pinned { 1 } else { 0 },
                    source,
                    input.performance_score,
                    profile_id,
                ],
            )?;
            ca
        }
        None => {
            tx.execute(
                "INSERT INTO style_examples
                 (id, profile_id, kind, source_context_text, example_text, why_it_works, tags_json,
                  pinned, source, performance_score, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    id,
                    profile_id,
                    input.kind.as_str(),
                    source_context_text,
                    example_text,
                    why_it_works,
                    tags_json,
                    if input.pinned { 1 } else { 0 },
                    source,
                    input.performance_score,
                    now,
                ],
            )?;
            now
        }
    };

    let ex = StyleExample {
        id,
        profile_id: profile_id.to_string(),
        kind: input.kind,
        source_context_text,
        example_text,
        why_it_works,
        tags,
        pinned: input.pinned,
        source,
        performance_score: input.performance_score,
        created_at,
    };
    sync_example_fts(&tx, &ex)?;
    tx.commit()?;
    Ok(ex)
}

pub fn delete_style_example(conn: &Connection, profile_id: &str, example_id: &str) -> AppResult<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "DELETE FROM style_examples WHERE id=?1 AND profile_id=?2",
        params![example_id, profile_id],
    )?;
    tx.execute(
        "DELETE FROM style_examples_fts WHERE example_id=?1",
        [example_id],
    )?;
    tx.commit()?;
    Ok(())
}

pub fn list_style_examples(
    conn: &Connection,
    profile_id: &str,
    kind: Option<ExampleKind>,
) -> AppResult<Vec<StyleExample>> {
    match kind {
        Some(k) => {
            let mut stmt = conn.prepare(
                "SELECT id, profile_id, kind, source_context_text, example_text, why_it_works,
                        tags_json, pinned, source, performance_score, created_at
                 FROM style_examples
                 WHERE profile_id=?1 AND kind=?2
                 ORDER BY pinned DESC, created_at DESC",
            )?;
            let rows = stmt
                .query_map(params![profile_id, k.as_str()], row_to_example)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        }
        None => {
            let mut stmt = conn.prepare(
                "SELECT id, profile_id, kind, source_context_text, example_text, why_it_works,
                        tags_json, pinned, source, performance_score, created_at
                 FROM style_examples
                 WHERE profile_id=?1
                 ORDER BY pinned DESC, created_at DESC",
            )?;
            let rows = stmt
                .query_map([profile_id], row_to_example)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        }
    }
}

/// Converts free-form query text into a safe FTS5 `OR` query string (preventing syntax injection).
pub fn build_safe_fts_query(raw: &str) -> Option<String> {
    let mut seen = HashSet::new();
    let mut terms = Vec::new();
    for token in raw.split(|c: char| !c.is_alphanumeric() && c != '_') {
        let lower = token.trim().to_lowercase();
        if lower.len() < 2 {
            continue;
        }
        if matches!(lower.as_str(), "and" | "or" | "not" | "near") {
            continue;
        }
        if seen.insert(lower.clone()) {
            terms.push(format!("\"{lower}\""));
        }
        if terms.len() >= 12 {
            break;
        }
    }
    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" OR "))
    }
}

/// Searches few-shot examples using FTS5 BM25 (returns pinned status and match rank).
pub fn search_style_examples(
    conn: &Connection,
    profile_id: &str,
    kind: Option<ExampleKind>,
    query: &str,
    limit: usize,
) -> AppResult<Vec<RetrievedExample>> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        let all = list_style_examples(conn, profile_id, kind)?;
        return Ok(all
            .into_iter()
            .take(limit)
            .map(|example| {
                let reason = if example.pinned {
                    "Pinned example".to_string()
                } else {
                    "Recent example".to_string()
                };
                RetrievedExample {
                    example,
                    reason,
                    score: None,
                }
            })
            .collect());
    }

    let Some(fts_query) = build_safe_fts_query(trimmed) else {
        return Ok(Vec::new());
    };

    let sql = match kind {
        Some(_) => {
            "SELECT e.id, e.profile_id, e.kind, e.source_context_text, e.example_text, e.why_it_works,
                    e.tags_json, e.pinned, e.source, e.performance_score, e.created_at,
                    bm25(style_examples_fts) AS rank
             FROM style_examples_fts f
             JOIN style_examples e ON e.id = f.example_id
             WHERE style_examples_fts MATCH ?1 AND e.profile_id = ?2 AND e.kind = ?3
             ORDER BY e.pinned DESC, rank ASC
             LIMIT ?4"
        }
        None => {
            "SELECT e.id, e.profile_id, e.kind, e.source_context_text, e.example_text, e.why_it_works,
                    e.tags_json, e.pinned, e.source, e.performance_score, e.created_at,
                    bm25(style_examples_fts) AS rank
             FROM style_examples_fts f
             JOIN style_examples e ON e.id = f.example_id
             WHERE style_examples_fts MATCH ?1 AND e.profile_id = ?2
             ORDER BY e.pinned DESC, rank ASC
             LIMIT ?3"
        }
    };

    let mut stmt = conn.prepare(sql)?;
    let rows = match kind {
        Some(k) => stmt
            .query_map(params![fts_query, profile_id, k.as_str(), limit as i64], |r| {
                let ex = row_to_example(r)?;
                let rank: f64 = r.get(11)?;
                Ok(RetrievedExample {
                    reason: if ex.pinned {
                        "Pinned + FTS5 match".into()
                    } else {
                        "FTS5 BM25 match".into()
                    },
                    example: ex,
                    score: Some(rank),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?,
        None => stmt
            .query_map(params![fts_query, profile_id, limit as i64], |r| {
                let ex = row_to_example(r)?;
                let rank: f64 = r.get(11)?;
                Ok(RetrievedExample {
                    reason: if ex.pinned {
                        "Pinned + FTS5 match".into()
                    } else {
                        "FTS5 BM25 match".into()
                    },
                    example: ex,
                    score: Some(rank),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?,
    };
    Ok(rows)
}

/// Retrieves few-shot examples for prompt injection (STY-03):
/// 1. Pinned examples first,
/// 2. FTS5 BM25 matches against `context_query`,
/// 3. Top recent / high-performing examples as backfill.
pub fn retrieve_examples_for_prompt(
    conn: &Connection,
    profile_id: &str,
    kind: ExampleKind,
    context_query: &str,
    limit: usize,
) -> AppResult<Vec<RetrievedExample>> {
    let mut picked: Vec<RetrievedExample> = Vec::new();
    let mut seen_ids = HashSet::new();

    // 1. Pinned examples first
    let all_of_kind = list_style_examples(conn, profile_id, Some(kind))?;
    for ex in all_of_kind.iter().filter(|e| e.pinned) {
        if picked.len() >= limit {
            break;
        }
        seen_ids.insert(ex.id.clone());
        picked.push(RetrievedExample {
            example: ex.clone(),
            reason: "Pinned example".into(),
            score: None,
        });
    }

    // 2. FTS5 BM25 contextual matches
    if picked.len() < limit && !context_query.trim().is_empty() {
        let fts_hits = search_style_examples(conn, profile_id, Some(kind), context_query, limit)?;
        for hit in fts_hits {
            if picked.len() >= limit {
                break;
            }
            if seen_ids.insert(hit.example.id.clone()) {
                picked.push(hit);
            }
        }
    }

    // 3. Backfill with best-performing / recent examples
    if picked.len() < limit {
        let mut remaining: Vec<StyleExample> = all_of_kind
            .into_iter()
            .filter(|e| !seen_ids.contains(&e.id))
            .collect();
        remaining.sort_by(|a, b| {
            let sa = a.performance_score.unwrap_or(-1.0);
            let sb = b.performance_score.unwrap_or(-1.0);
            sb.partial_cmp(&sa)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.created_at.cmp(&a.created_at))
        });
        for ex in remaining {
            if picked.len() >= limit {
                break;
            }
            seen_ids.insert(ex.id.clone());
            let reason = if ex.performance_score.is_some() {
                "Top performance score backfill".into()
            } else {
                "Recent example backfill".into()
            };
            picked.push(RetrievedExample {
                example: ex,
                reason,
                score: None,
            });
        }
    }

    Ok(picked)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{open, repo as base_repo};
    use tempfile::tempdir;

    #[test]
    fn style_guide_versions_preserve_history_and_switch_active() {
        let dir = tempdir().unwrap();
        let (conn, _) = open(&dir.path().join("a.db"), &dir.path().join("b")).unwrap();
        let pid = base_repo::ensure_default_profile(&conn, "/tmp/prof").unwrap();

        let v1 = save_style_guide_version(
            &conn,
            &pid,
            StyleGuideInput {
                free_text_guide: "Short, technical, zero hype.".into(),
                learned_rules: vec!["Cite concrete numbers".into()],
                banned_patterns: vec!["game changer".into(), "Game Changer".into()],
                max_reply_chars: 200,
                emoji_policy: EmojiPolicy::None,
                hashtag_policy: HashtagPolicy::None,
                note: Some("initial".into()),
            },
        )
        .unwrap();
        assert_eq!(v1.version, 1);
        assert!(v1.is_active);
        assert_eq!(v1.banned_patterns.len(), 1);

        let v2 = save_style_guide_version(
            &conn,
            &pid,
            StyleGuideInput {
                free_text_guide: "Direct founder voice.".into(),
                learned_rules: vec![],
                banned_patterns: vec!["delve".into()],
                max_reply_chars: 220,
                emoji_policy: EmojiPolicy::Minimal,
                hashtag_policy: HashtagPolicy::Max1,
                note: Some("tweaked".into()),
            },
        )
        .unwrap();
        assert_eq!(v2.version, 2);
        assert!(v2.is_active);

        let versions = list_style_guide_versions(&conn, &pid).unwrap();
        assert_eq!(versions.len(), 2);
        assert!(!versions[1].is_active);

        let reactivated = activate_style_guide_version(&conn, &pid, 1).unwrap();
        assert_eq!(reactivated.version, 1);
        assert!(reactivated.is_active);
    }

    #[test]
    fn fts5_retrieves_pinned_then_bm25_matches_and_ignores_punctuation() {
        let dir = tempdir().unwrap();
        let (conn, _) = open(&dir.path().join("a.db"), &dir.path().join("b")).unwrap();
        let pid = base_repo::ensure_default_profile(&conn, "/tmp/prof").unwrap();

        upsert_style_example(
            &conn,
            &pid,
            StyleExampleInput {
                id: None,
                kind: ExampleKind::Reply,
                source_context_text: Some("Always anchor to concrete benchmarks.".into()),
                example_text: "Show the latency p99 before and after—benchmarks beat adjectives.".into(),
                why_it_works: Some("Pinned signature framing".into()),
                tags: vec!["benchmarks".into()],
                pinned: true,
                source: Some("manual".into()),
                performance_score: None,
            },
        )
        .unwrap();

        upsert_style_example(
            &conn,
            &pid,
            StyleExampleInput {
                id: None,
                kind: ExampleKind::Reply,
                source_context_text: Some("Postgres connection pooling under serverless load".into()),
                example_text: "PgBouncer in transaction mode solves 90% of Lambda connection spikes.".into(),
                why_it_works: Some("Actionable infra fix".into()),
                tags: vec!["postgres".into(), "serverless".into()],
                pinned: false,
                source: Some("manual".into()),
                performance_score: Some(8.5),
            },
        )
        .unwrap();

        upsert_style_example(
            &conn,
            &pid,
            StyleExampleInput {
                id: None,
                kind: ExampleKind::Reply,
                source_context_text: Some("CSS grid layout tips".into()),
                example_text: "Subgrid finally makes nested card footers line up without JS.".into(),
                why_it_works: None,
                tags: vec!["css".into()],
                pinned: false,
                source: Some("manual".into()),
                performance_score: Some(3.0),
            },
        )
        .unwrap();

        let retrieved = retrieve_examples_for_prompt(
            &conn,
            &pid,
            ExampleKind::Reply,
            "Our Postgres serverless connections are exhausting!! NOT/AND?",
            2,
        )
        .unwrap();
        assert_eq!(retrieved.len(), 2);
        assert!(retrieved[0].example.pinned);
        assert!(retrieved[1].example.example_text.contains("PgBouncer"));
        assert!(retrieved[1].reason.contains("FTS5"));
    }
}
