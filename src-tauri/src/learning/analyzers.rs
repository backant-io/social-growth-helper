//! Rule-based analyzers: edit diffs (LRN-02), dismissals (LRN-03), model keep rate (LRN-04), timing and
//! author tier (LRN-13), anti-patterns (LRN-14), rubric evolution (LRN-12) and winner promotion (LRN-10).
//!
//! Every analyzer is a pure function over loaded rows. Each returns a signal report for the UI and zero
//! or more proposals. Proposals never apply themselves; they wait for a human accept.

use super::outcomes::{median, OutcomeItem};
use super::patch::{Patch, PatchOp, PatchTarget, StyleState};
use super::textdiff::{emoji_count, hashtag_count, levenshtein, tokens, word_diff};
use crate::error::AppResult;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

pub const MIN_PHRASE_COUNT: usize = 3;
const MIN_BUCKET: usize = 3;
const TOP_NORMALIZED: f64 = 1.5;
const FLOP_NORMALIZED: f64 = 0.6;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvidenceItem {
    pub label: String,
    pub draft_id: Option<String>,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Evidence {
    pub support: usize,
    pub total: usize,
    pub items: Vec<EvidenceItem>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InsightDraft {
    pub category: &'static str,
    pub hypothesis: String,
    pub evidence: Evidence,
    /// Raw confidence before the minimum-sample guard.
    pub confidence: f64,
    pub patch: Patch,
    pub dedupe_key: String,
    pub source: &'static str,
}

fn style_patch(ops: Vec<PatchOp>) -> Patch {
    Patch { target: PatchTarget::Style, ops }
}

fn rubric_patch(ops: Vec<PatchOp>) -> Patch {
    Patch { target: PatchTarget::Rubric, ops }
}

const STOP: &[&str] = &[
    "the", "and", "for", "that", "this", "with", "you", "your", "are", "was", "but", "not", "have", "has", "had", "its", "it's", "just",
    "from", "they", "them", "what", "when", "will", "can", "all", "our", "out", "who", "how", "why", "about", "into", "than", "then",
    "there", "their", "would", "could", "should", "been", "being", "more", "very", "really", "also", "one", "get", "got", "like",
    "a", "an", "of", "to", "in", "is", "it", "on", "i", "me", "my", "we", "so", "do", "be", "as", "at", "or", "if", "no", "up", "he",
    "she", "his", "her", "by", "im", "i'm", "dont", "don't", "these", "those", "some", "most", "much", "many", "any", "were", "does",
];

fn is_stop(w: &str) -> bool {
    STOP.contains(&w)
}

fn wordlike(w: &str) -> bool {
    !w.starts_with('#') && !w.starts_with('@') && w.chars().any(|c| c.is_alphabetic())
}

fn phrase_ok(words: &[String]) -> bool {
    if !words.iter().all(|w| wordlike(w)) {
        return false;
    }
    if words.len() == 1 {
        return words[0].chars().count() >= 4 && !is_stop(&words[0]);
    }
    !words.iter().all(|w| is_stop(w))
}

// ---------------------------------------------------------------------------------------------
// Edit diffs
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct EditSample {
    pub draft_id: String,
    pub model_used: String,
    pub initial: String,
    pub edited: String,
    pub edit_distance: i64,
}

/// Approved or published reply and original-post drafts, optionally only those created since a timestamp.
pub fn load_edit_samples(conn: &Connection, profile_id: &str, since: Option<&str>) -> AppResult<Vec<EditSample>> {
    let mut out = vec![];
    for table in ["reply_drafts", "ogc_drafts"] {
        let sql = format!(
            "SELECT id, model_used, initial_text, edited_text, edit_distance FROM {table}
             WHERE profile_id=?1 AND (status='published' OR user_approved_at IS NOT NULL) AND created_at >= ?2
             ORDER BY created_at, id"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![profile_id, since.unwrap_or("")], |r| {
            Ok(EditSample {
                draft_id: r.get(0)?,
                model_used: r.get(1)?,
                initial: r.get(2)?,
                edited: r.get(3)?,
                edit_distance: r.get(4)?,
            })
        })?;
        for r in rows {
            out.push(r?);
        }
    }
    Ok(out)
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PhraseStat {
    pub phrase: String,
    pub count: usize,
    pub draft_ids: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct EditReport {
    pub drafts: usize,
    pub edited_drafts: usize,
    pub avg_edit_distance: f64,
    pub shortened_drafts: usize,
    pub median_final_length: Option<usize>,
    pub avg_length_change_pct: f64,
    pub emoji_removed_drafts: usize,
    pub emoji_added_drafts: usize,
    pub hashtag_removed_drafts: usize,
    pub removed: Vec<PhraseStat>,
    pub added: Vec<PhraseStat>,
}

fn count_phrases(runs_per_draft: &[(String, Vec<Vec<String>>)]) -> Vec<PhraseStat> {
    let mut map: HashMap<Vec<String>, Vec<String>> = HashMap::new();
    for (id, runs) in runs_per_draft {
        let mut seen: HashSet<Vec<String>> = HashSet::new();
        for run in runs {
            for n in 1..=3usize {
                if run.len() < n {
                    continue;
                }
                for w in run.windows(n) {
                    if phrase_ok(w) {
                        seen.insert(w.to_vec());
                    }
                }
            }
        }
        for g in seen {
            map.entry(g).or_default().push(id.clone());
        }
    }
    let grams: Vec<(Vec<String>, Vec<String>)> = map.into_iter().filter(|(_, ids)| ids.len() >= 2).collect();
    let contains = |long: &[String], short: &[String]| long.len() > short.len() && long.windows(short.len()).any(|w| w == short);
    let mut kept: Vec<PhraseStat> = grams
        .iter()
        .filter(|(g, ids)| !grams.iter().any(|(g2, ids2)| contains(g2, g) && ids2.len() >= ids.len()))
        .map(|(g, ids)| PhraseStat { phrase: g.join(" "), count: ids.len(), draft_ids: ids.iter().take(5).cloned().collect() })
        .collect();
    kept.sort_by(|a, b| b.count.cmp(&a.count).then(b.phrase.len().cmp(&a.phrase.len())).then(a.phrase.cmp(&b.phrase)));
    kept.truncate(12);
    kept
}

pub fn edit_report(samples: &[EditSample]) -> EditReport {
    let mut r = EditReport { drafts: samples.len(), ..Default::default() };
    let mut removed_runs = vec![];
    let mut added_runs = vec![];
    let mut dist_sum = 0.0;
    let mut change_sum = 0.0;
    let mut final_lens: Vec<f64> = vec![];
    for s in samples {
        final_lens.push(s.edited.chars().count() as f64);
        if s.initial.trim() == s.edited.trim() {
            continue;
        }
        r.edited_drafts += 1;
        let dist = if s.edit_distance > 0 { s.edit_distance as usize } else { levenshtein(&s.initial, &s.edited) };
        dist_sum += dist as f64;
        let (li, le) = (s.initial.chars().count() as f64, s.edited.chars().count() as f64);
        if le < li {
            r.shortened_drafts += 1;
        }
        if li > 0.0 {
            change_sum += (le - li) / li * 100.0;
        }
        let (ei, ee) = (emoji_count(&s.initial), emoji_count(&s.edited));
        r.emoji_removed_drafts += usize::from(ei > ee);
        r.emoji_added_drafts += usize::from(ee > ei);
        r.hashtag_removed_drafts += usize::from(hashtag_count(&s.initial) > hashtag_count(&s.edited));
        let d = word_diff(&s.initial, &s.edited);
        removed_runs.push((s.draft_id.clone(), d.removed));
        added_runs.push((s.draft_id.clone(), d.added));
    }
    if r.edited_drafts > 0 {
        r.avg_edit_distance = dist_sum / r.edited_drafts as f64;
        r.avg_length_change_pct = change_sum / r.edited_drafts as f64;
    }
    r.median_final_length = median(&mut final_lens).map(|m| m.round() as usize);
    r.removed = count_phrases(&removed_runs);
    r.added = count_phrases(&added_runs);
    r
}

fn evidence_for(ids: &[String], label: &str, total: usize, support: usize) -> Evidence {
    Evidence {
        support,
        total,
        items: ids
            .iter()
            .take(5)
            .map(|id| EvidenceItem { label: label.to_string(), draft_id: Some(id.clone()), detail: String::new() })
            .collect(),
    }
}

pub fn edit_insights(rep: &EditReport, style: &StyleState) -> Vec<InsightDraft> {
    let mut out = vec![];
    let edited = rep.edited_drafts;
    if edited < MIN_PHRASE_COUNT {
        return out;
    }
    let banned: HashSet<String> = style.banned.iter().map(|b| b.to_lowercase()).collect();
    let rules: HashSet<String> = style.rules.iter().map(|b| b.to_lowercase()).collect();
    let share = |n: usize| n as f64 / edited as f64;

    for p in rep.removed.iter().filter(|p| p.count >= MIN_PHRASE_COUNT && share(p.count) >= 0.15 && !banned.contains(&p.phrase)).take(3) {
        out.push(InsightDraft {
            category: "anti_pattern",
            hypothesis: format!("You delete \"{}\" in {} of {} edited drafts. Ban it so drafts stop including it.", p.phrase, p.count, edited),
            evidence: evidence_for(&p.draft_ids, "removed in edit", edited, p.count),
            confidence: (0.3 + 0.6 * share(p.count)).min(0.9),
            patch: style_patch(vec![PatchOp::AddBanned { pattern: p.phrase.clone() }]),
            dedupe_key: format!("ban:{}", p.phrase),
            source: "rules",
        });
    }

    if rep.shortened_drafts >= MIN_PHRASE_COUNT && share(rep.shortened_drafts) >= 0.6 && rep.avg_length_change_pct <= -15.0 {
        let target = rep.median_final_length.map(|m| ((m as f64 / 10.0).round() as usize * 10).max(40)).unwrap_or(140);
        let rule = format!("Keep replies short: aim for about {target} characters or fewer");
        if !rules.contains(&rule.to_lowercase()) {
            out.push(InsightDraft {
                category: "voice",
                hypothesis: format!(
                    "You shorten {} of {} edited drafts by {:.0}% on average. Tell the drafter to start shorter.",
                    rep.shortened_drafts, edited, -rep.avg_length_change_pct
                ),
                evidence: Evidence { support: rep.shortened_drafts, total: edited, items: vec![] },
                confidence: (0.3 + 0.6 * share(rep.shortened_drafts)).min(0.9),
                patch: style_patch(vec![PatchOp::AddRule { text: rule }]),
                dedupe_key: "rule:shorter".into(),
                source: "rules",
            });
        }
    }

    for (n, rule, label, key) in [
        (rep.emoji_removed_drafts, "Do not use emoji", "emoji", "rule:no_emoji"),
        (rep.hashtag_removed_drafts, "Do not use hashtags", "hashtags", "rule:no_hashtags"),
    ] {
        if n >= MIN_PHRASE_COUNT && share(n) >= 0.2 && !rules.contains(&rule.to_lowercase()) {
            out.push(InsightDraft {
                category: "voice",
                hypothesis: format!("You remove {label} from {n} of {edited} edited drafts."),
                evidence: Evidence { support: n, total: edited, items: vec![] },
                confidence: (0.3 + 0.6 * share(n)).min(0.9),
                patch: style_patch(vec![PatchOp::AddRule { text: rule.into() }]),
                dedupe_key: key.into(),
                source: "rules",
            });
        }
    }

    if let Some(p) = rep.added.iter().find(|p| p.count >= MIN_PHRASE_COUNT && share(p.count) >= 0.15 && p.phrase.contains(' ')) {
        let rule = format!("Where it fits, use wording like \"{}\"", p.phrase);
        if !rules.contains(&rule.to_lowercase()) {
            out.push(InsightDraft {
                category: "voice",
                hypothesis: format!("You add \"{}\" to {} of {} edited drafts. It looks like part of your voice.", p.phrase, p.count, edited),
                evidence: evidence_for(&p.draft_ids, "added in edit", edited, p.count),
                confidence: (0.25 + 0.5 * share(p.count)).min(0.85),
                patch: style_patch(vec![PatchOp::AddRule { text: rule }]),
                dedupe_key: format!("add:{}", p.phrase),
                source: "rules",
            });
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Dismissals
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct DismissalRow {
    pub evaluation_id: String,
    pub author: String,
    pub reason: Option<String>,
    pub score: f64,
    pub dismissed: bool,
}

pub fn load_dismissals(conn: &Connection, profile_id: &str, since: Option<&str>) -> AppResult<Vec<DismissalRow>> {
    let mut stmt = conn.prepare(
        "SELECT e.id, o.author_handle, e.dismiss_reason, e.potential_score, e.status
         FROM scout_evaluations e JOIN observed_posts o ON o.id = e.observed_post_id
         WHERE o.profile_id=?1 AND e.status IN ('dismissed','drafted') AND e.created_at >= ?2",
    )?;
    let rows = stmt.query_map(params![profile_id, since.unwrap_or("")], |r| {
        Ok(DismissalRow {
            evaluation_id: r.get(0)?,
            author: r.get(1)?,
            reason: r.get(2)?,
            score: r.get(3)?,
            dismissed: r.get::<_, String>(4)? == "dismissed",
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct DismissalReport {
    pub dismissed: usize,
    pub drafted: usize,
    pub avg_score_dismissed: Option<f64>,
    pub avg_score_drafted: Option<f64>,
    pub reasons: Vec<(String, usize)>,
    pub authors: Vec<(String, usize)>,
}

fn avg(v: &[f64]) -> Option<f64> {
    (!v.is_empty()).then(|| v.iter().sum::<f64>() / v.len() as f64)
}

fn ranked(map: HashMap<String, usize>) -> Vec<(String, usize)> {
    let mut v: Vec<_> = map.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    v
}

pub fn dismissal_report(rows: &[DismissalRow]) -> DismissalReport {
    let mut reasons: HashMap<String, usize> = HashMap::new();
    let mut authors: HashMap<String, usize> = HashMap::new();
    let (mut ds, mut dr) = (vec![], vec![]);
    for r in rows {
        if r.dismissed {
            ds.push(r.score);
            *reasons.entry(r.reason.as_deref().map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty()).unwrap_or_else(|| "no reason given".into())).or_default() += 1;
            *authors.entry(r.author.to_lowercase()).or_default() += 1;
        } else {
            dr.push(r.score);
        }
    }
    DismissalReport {
        dismissed: ds.len(),
        drafted: dr.len(),
        avg_score_dismissed: avg(&ds),
        avg_score_drafted: avg(&dr),
        reasons: ranked(reasons),
        authors: ranked(authors),
    }
}

pub fn dismissal_insights(rep: &DismissalReport, rows: &[DismissalRow], rubric_text: &str) -> Vec<InsightDraft> {
    let mut out = vec![];
    if rep.dismissed < MIN_PHRASE_COUNT {
        return out;
    }
    let present = |t: &str| rubric_text.to_lowercase().contains(&t.to_lowercase());
    for (reason, n) in rep.reasons.iter().filter(|(_, n)| *n >= MIN_PHRASE_COUNT) {
        let share = *n as f64 / rep.dismissed as f64;
        if share < 0.3 {
            continue;
        }
        let text = match reason.as_str() {
            "wrong niche" => "Skip posts outside your niche topics",
            "too late" => "Prefer posts that are still fresh; late posts are often not worth answering",
            "low quality" => "Skip low-quality or engagement-bait posts",
            _ => continue,
        };
        if present(text) {
            continue;
        }
        out.push(InsightDraft {
            category: "dismissal",
            hypothesis: format!("You dismissed {n} of {} cards as \"{reason}\". Teach the scout rubric to filter these earlier.", rep.dismissed),
            evidence: Evidence { support: *n, total: rep.dismissed, items: vec![EvidenceItem { label: format!("dismissed: {reason}"), draft_id: None, detail: format!("{n} cards") }] },
            confidence: (0.3 + 0.6 * share).min(0.9),
            patch: rubric_patch(vec![PatchOp::AddCriterion { text: text.into() }]),
            dedupe_key: format!("dismiss:{reason}"),
            source: "rules",
        });
    }
    let drafted_authors: HashSet<String> = rows.iter().filter(|r| !r.dismissed).map(|r| r.author.to_lowercase()).collect();
    for (author, n) in rep.authors.iter().filter(|(a, n)| *n >= MIN_PHRASE_COUNT && !drafted_authors.contains(a)).take(2) {
        let text = format!("Score posts from @{author} lower: you dismissed {n} of their posts and replied to none");
        if present(&text) {
            continue;
        }
        out.push(InsightDraft {
            category: "dismissal",
            hypothesis: format!("You dismissed {n} posts from @{author} and replied to none."),
            evidence: Evidence { support: *n, total: rep.dismissed, items: vec![EvidenceItem { label: format!("@{author}"), draft_id: None, detail: format!("{n} dismissed") }] },
            confidence: 0.6,
            patch: rubric_patch(vec![PatchOp::AddCriterion { text }]),
            dedupe_key: format!("dismiss_author:{author}"),
            source: "rules",
        });
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Models (LRN-04)
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct ModelStat {
    pub model: String,
    pub drafts: usize,
    pub kept_as_is: usize,
    pub kept_lightly_edited: usize,
    pub keep_rate: f64,
    pub avg_edit_distance: f64,
    pub avg_normalized: Option<f64>,
}

/// A draft counts as kept when the final text is within 20% (by edit distance) of what the model wrote.
pub fn model_report(samples: &[EditSample], outcomes: &[OutcomeItem]) -> Vec<ModelStat> {
    let mut by: BTreeMap<&str, Vec<&EditSample>> = BTreeMap::new();
    for s in samples {
        by.entry(&s.model_used).or_default().push(s);
    }
    let norm: HashMap<&str, f64> = outcomes.iter().filter_map(|o| o.normalized.map(|n| (o.draft_id.as_str(), n))).collect();
    by.into_iter()
        .map(|(model, v)| {
            let as_is = v.iter().filter(|s| s.initial.trim() == s.edited.trim()).count();
            let light = v
                .iter()
                .filter(|s| {
                    let len = s.initial.chars().count().max(1) as f64;
                    let d = if s.edit_distance > 0 { s.edit_distance as usize } else { levenshtein(&s.initial, &s.edited) };
                    d as f64 / len <= 0.2
                })
                .count();
            let n: Vec<f64> = v.iter().filter_map(|s| norm.get(s.draft_id.as_str()).copied()).collect();
            ModelStat {
                model: model.to_string(),
                drafts: v.len(),
                kept_as_is: as_is,
                kept_lightly_edited: light,
                keep_rate: light as f64 / v.len() as f64,
                avg_edit_distance: v.iter().map(|s| s.edit_distance as f64).sum::<f64>() / v.len() as f64,
                avg_normalized: avg(&n),
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Timing and tier (LRN-13)
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct BucketStat {
    pub label: String,
    pub n: usize,
    pub mean_normalized: f64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct TimingReport {
    pub age: Vec<BucketStat>,
    pub tier: Vec<BucketStat>,
    pub overall_mean: Option<f64>,
    pub scored_items: usize,
}

fn age_bucket(m: f64) -> &'static str {
    match m {
        x if x < 15.0 => "under 15 minutes",
        x if x < 60.0 => "15 to 60 minutes",
        x if x < 180.0 => "1 to 3 hours",
        _ => "over 3 hours",
    }
}

fn tier_bucket(f: i64) -> &'static str {
    match f {
        x if x < 1_000 => "under 1k",
        x if x < 10_000 => "1k to 10k",
        x if x < 100_000 => "10k to 100k",
        _ => "over 100k",
    }
}

const AGE_ORDER: [&str; 4] = ["under 15 minutes", "15 to 60 minutes", "1 to 3 hours", "over 3 hours"];
const TIER_ORDER: [&str; 4] = ["under 1k", "1k to 10k", "10k to 100k", "over 100k"];

fn buckets<'a>(pairs: impl Iterator<Item = (&'static str, f64)> + 'a, order: &[&str]) -> Vec<BucketStat> {
    let mut m: HashMap<&str, Vec<f64>> = HashMap::new();
    for (k, v) in pairs {
        m.entry(k).or_default().push(v);
    }
    order
        .iter()
        .filter_map(|k| m.get(k).map(|v| BucketStat { label: (*k).to_string(), n: v.len(), mean_normalized: v.iter().sum::<f64>() / v.len() as f64 }))
        .collect()
}

pub fn timing_report(items: &[OutcomeItem]) -> TimingReport {
    let scored: Vec<&OutcomeItem> = items.iter().filter(|i| i.normalized.is_some()).collect();
    let all: Vec<f64> = scored.iter().filter_map(|i| i.normalized).collect();
    TimingReport {
        age: buckets(scored.iter().filter_map(|i| Some((age_bucket(i.reply_age_minutes?), i.normalized?))), &AGE_ORDER),
        tier: buckets(scored.iter().filter_map(|i| Some((tier_bucket(i.author_followers?), i.normalized?))), &TIER_ORDER),
        overall_mean: avg(&all),
        scored_items: scored.len(),
    }
}

pub fn timing_insights(rep: &TimingReport, rubric_text: &str) -> Vec<InsightDraft> {
    let mut out = vec![];
    let Some(overall) = rep.overall_mean.filter(|_| rep.scored_items >= 8) else { return out };
    let present = |t: &str| rubric_text.to_lowercase().contains(&t.to_lowercase());
    let best = |b: &[BucketStat]| {
        b.iter()
            .filter(|s| s.n >= MIN_BUCKET && s.mean_normalized >= 1.25 && s.mean_normalized >= overall * 1.2)
            .max_by(|a, b| a.mean_normalized.partial_cmp(&b.mean_normalized).unwrap())
            .cloned()
    };
    if let Some(b) = best(&rep.age) {
        let text = format!("Prefer posts you can answer within {} of when they were posted", b.label);
        if !present(&text) {
            out.push(InsightDraft {
                category: "scout_timing",
                hypothesis: format!("Replies sent {} after the original averaged {:.1}x your baseline ({} replies, overall {:.1}x).", b.label, b.mean_normalized, b.n, overall),
                evidence: Evidence { support: b.n, total: rep.scored_items, items: vec![EvidenceItem { label: b.label.clone(), draft_id: None, detail: format!("{:.2}x over {} replies", b.mean_normalized, b.n) }] },
                confidence: (0.3 + 0.1 * (b.n as f64).min(6.0)).min(0.85),
                patch: rubric_patch(vec![PatchOp::AddCriterion { text }]),
                dedupe_key: format!("timing:{}", b.label),
                source: "rules",
            });
        }
    }
    if let Some(b) = best(&rep.tier) {
        let text = format!("Prefer posts from authors with {} followers", b.label);
        if !present(&text) {
            out.push(InsightDraft {
                category: "author_tier",
                hypothesis: format!("Replies to authors with {} followers averaged {:.1}x your baseline ({} replies, overall {:.1}x).", b.label, b.mean_normalized, b.n, overall),
                evidence: Evidence { support: b.n, total: rep.scored_items, items: vec![EvidenceItem { label: b.label.clone(), draft_id: None, detail: format!("{:.2}x over {} replies", b.mean_normalized, b.n) }] },
                confidence: (0.3 + 0.1 * (b.n as f64).min(6.0)).min(0.85),
                patch: rubric_patch(vec![PatchOp::AddCriterion { text }]),
                dedupe_key: format!("tier:{}", b.label),
                source: "rules",
            });
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Rubric evolution (LRN-12), anti-patterns from flops (LRN-14), winner promotion (LRN-10)
// ---------------------------------------------------------------------------------------------

pub fn pearson(xs: &[f64], ys: &[f64]) -> Option<f64> {
    let n = xs.len();
    if n < 3 || n != ys.len() {
        return None;
    }
    let (mx, my) = (xs.iter().sum::<f64>() / n as f64, ys.iter().sum::<f64>() / n as f64);
    let (mut sxy, mut sxx, mut syy) = (0.0, 0.0, 0.0);
    for (x, y) in xs.iter().zip(ys) {
        sxy += (x - mx) * (y - my);
        sxx += (x - mx).powi(2);
        syy += (y - my).powi(2);
    }
    (sxx > 0.0 && syy > 0.0).then(|| sxy / (sxx.sqrt() * syy.sqrt()))
}

/// Correlation between the rubric's potential score and the normalized result, when there is enough data.
pub fn rubric_correlation(items: &[OutcomeItem]) -> Option<f64> {
    let pairs: Vec<(f64, f64)> = items.iter().filter_map(|i| Some((i.potential_score?, i.normalized?))).collect();
    if pairs.len() < 10 {
        return None;
    }
    pearson(&pairs.iter().map(|p| p.0).collect::<Vec<_>>(), &pairs.iter().map(|p| p.1).collect::<Vec<_>>())
}

fn term_set(text: &str) -> HashSet<String> {
    tokens(text).into_iter().filter(|w| wordlike(w) && w.chars().count() >= 5 && !is_stop(w)).collect()
}

pub fn rubric_insights(items: &[OutcomeItem], rubric_text: &str, niche: &[String]) -> Vec<InsightDraft> {
    let mut out = vec![];
    let scored: Vec<&OutcomeItem> = items.iter().filter(|i| i.normalized.is_some()).collect();
    if scored.len() < 10 {
        return out;
    }
    let top: Vec<&&OutcomeItem> = scored.iter().filter(|i| i.normalized.unwrap() >= TOP_NORMALIZED).collect();
    let flop: Vec<&&OutcomeItem> = scored.iter().filter(|i| i.normalized.unwrap() <= FLOP_NORMALIZED).collect();
    let present = |t: &str| rubric_text.to_lowercase().contains(&t.to_lowercase());
    let corr = rubric_correlation(items);
    let corr_note = corr.map(|c| format!(" The current rubric score correlates only {c:.2} with results.")).unwrap_or_default();

    if top.len() >= 3 && flop.len() >= 3 {
        let top_sets: Vec<HashSet<String>> = top.iter().map(|i| term_set(&i.target_text)).collect();
        let flop_sets: Vec<HashSet<String>> = flop.iter().map(|i| term_set(&i.target_text)).collect();
        let mut counts: HashMap<&String, usize> = HashMap::new();
        for s in &top_sets {
            for t in s {
                *counts.entry(t).or_default() += 1;
            }
        }
        let mut terms: Vec<(&String, usize)> = counts
            .into_iter()
            .filter(|(t, c)| *c >= 3 && !flop_sets.iter().any(|s| s.contains(*t)) && !niche.iter().any(|n| n.to_lowercase() == **t))
            .collect();
        terms.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        terms.truncate(3);
        if !terms.is_empty() {
            let words: Vec<String> = terms.iter().map(|t| t.0.clone()).collect();
            let text = format!("Favor posts about {}", words.join(", "));
            if !present(&text) {
                let ids: Vec<String> = top.iter().filter(|i| term_set(&i.target_text).contains(&words[0])).map(|i| i.draft_id.clone()).collect();
                out.push(InsightDraft {
                    category: "rubric_evolution",
                    hypothesis: format!("Your best replies ({} at 1.5x baseline or more) were all to posts mentioning {}; none of your {} weakest were.{}", top.len(), words.join(", "), flop.len(), corr_note),
                    evidence: Evidence { support: terms[0].1, total: scored.len(), items: ids.iter().take(5).map(|id| EvidenceItem { label: "top reply".into(), draft_id: Some(id.clone()), detail: String::new() }).collect() },
                    confidence: (0.3 + 0.08 * terms[0].1 as f64).min(0.8),
                    patch: rubric_patch(vec![PatchOp::AddCriterion { text }]),
                    dedupe_key: format!("rubric_terms:{}", words.join(",")),
                    source: "rules",
                });
            }
        }
    }

    let (with, without): (Vec<f64>, Vec<f64>) = {
        let w: Vec<f64> = scored.iter().filter(|i| i.has_media).filter_map(|i| i.normalized).collect();
        let wo: Vec<f64> = scored.iter().filter(|i| !i.has_media).filter_map(|i| i.normalized).collect();
        (w, wo)
    };
    if with.len() >= MIN_BUCKET && without.len() >= MIN_BUCKET {
        let (mw, mo) = (avg(&with).unwrap(), avg(&without).unwrap());
        let (text, label) = if mw >= mo * 1.4 && mw >= 1.2 {
            ("Favor posts with an image or chart", "with media")
        } else if mo >= mw * 1.4 && mo >= 1.2 {
            ("Deprioritize posts that are mostly an image or video", "text only")
        } else {
            ("", "")
        };
        if !text.is_empty() && !present(text) {
            out.push(InsightDraft {
                category: "rubric_evolution",
                hypothesis: format!("Replies to posts {label} did better: {:.1}x vs {:.1}x your baseline.{corr_note}", mw.max(mo), mw.min(mo)),
                evidence: Evidence { support: with.len().min(without.len()), total: scored.len(), items: vec![EvidenceItem { label: "with media".into(), draft_id: None, detail: format!("{mw:.2}x over {} replies", with.len()) }, EvidenceItem { label: "text only".into(), draft_id: None, detail: format!("{mo:.2}x over {} replies", without.len()) }] },
                confidence: 0.55,
                patch: rubric_patch(vec![PatchOp::AddCriterion { text: text.into() }]),
                dedupe_key: format!("rubric_media:{label}"),
                source: "rules",
            });
        }
    }
    out
}

fn bigrams(text: &str) -> HashSet<String> {
    let t = tokens(text);
    let mut out = HashSet::new();
    for n in [2usize, 3] {
        for w in t.windows(n) {
            if phrase_ok(w) {
                out.insert(w.join(" "));
            }
        }
    }
    out
}

/// Phrases that show up in several of your weakest replies and in none of your best (LRN-14).
pub fn flop_insights(items: &[OutcomeItem], style: &StyleState) -> Vec<InsightDraft> {
    let scored: Vec<&OutcomeItem> = items.iter().filter(|i| i.normalized.is_some()).collect();
    if scored.len() < 10 {
        return vec![];
    }
    let flop: Vec<&&OutcomeItem> = scored.iter().filter(|i| i.normalized.unwrap() <= FLOP_NORMALIZED).collect();
    let top: Vec<HashSet<String>> = scored.iter().filter(|i| i.normalized.unwrap() >= TOP_NORMALIZED).map(|i| bigrams(&i.final_text)).collect();
    if flop.len() < 4 {
        return vec![];
    }
    let banned: HashSet<String> = style.banned.iter().map(|b| b.to_lowercase()).collect();
    let mut counts: HashMap<String, Vec<String>> = HashMap::new();
    for i in &flop {
        for g in bigrams(&i.final_text) {
            counts.entry(g).or_default().push(i.draft_id.clone());
        }
    }
    let mut cands: Vec<(String, Vec<String>)> = counts
        .into_iter()
        .filter(|(g, ids)| ids.len() >= MIN_PHRASE_COUNT && !top.iter().any(|s| s.contains(g)) && !banned.contains(g))
        .collect();
    cands.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(b.0.len().cmp(&a.0.len())).then(a.0.cmp(&b.0)));
    let mut out: Vec<InsightDraft> = vec![];
    for (g, ids) in cands {
        if out.iter().any(|o| matches!(&o.patch.ops[0], PatchOp::AddBanned { pattern } if pattern.contains(&g) || g.contains(pattern.as_str()))) {
            continue;
        }
        out.push(InsightDraft {
            category: "anti_pattern",
            hypothesis: format!("\"{g}\" appears in {} of your {} weakest replies and in none of your strongest.", ids.len(), flop.len()),
            evidence: evidence_for(&ids, "weak reply", flop.len(), ids.len()),
            confidence: (0.25 + 0.1 * ids.len() as f64).min(0.7),
            patch: style_patch(vec![PatchOp::AddBanned { pattern: g.clone() }]),
            dedupe_key: format!("ban:{g}"),
            source: "rules",
        });
        if out.len() == 2 {
            break;
        }
    }
    out
}

/// Suggests promoting your best-performing published replies into the few-shot library (LRN-10).
pub fn winner_insights(items: &[OutcomeItem], existing_examples: &HashSet<String>) -> Vec<InsightDraft> {
    let mut winners: Vec<&OutcomeItem> = items
        .iter()
        .filter(|i| i.normalized.map(|n| n >= TOP_NORMALIZED).unwrap_or(false))
        .filter(|i| i.final_text.chars().count() >= 20 && !existing_examples.contains(&i.final_text.trim().to_lowercase()))
        .collect();
    winners.sort_by(|a, b| b.normalized.partial_cmp(&a.normalized).unwrap());
    winners
        .into_iter()
        .take(3)
        .map(|w| {
            let n = w.normalized.unwrap();
            InsightDraft {
                category: "few_shot_promotion",
                hypothesis: format!("This reply did {n:.1}x your usual results after adjusting for the author's reach. Use it as a voice example."),
                evidence: Evidence {
                    support: 1,
                    total: items.len(),
                    items: vec![EvidenceItem { label: "winner".into(), draft_id: Some(w.draft_id.clone()), detail: format!("{} impressions, {:.2}x baseline", w.impressions.unwrap_or(0), n) }],
                },
                confidence: (0.4 + 0.15 * (n - TOP_NORMALIZED)).clamp(0.4, 0.85),
                patch: Patch {
                    target: PatchTarget::Examples,
                    ops: vec![PatchOp::PromoteExample {
                        draft_id: w.draft_id.clone(),
                        example_text: w.final_text.clone(),
                        source_context_text: Some(w.target_text.chars().take(500).collect()),
                        why_it_works: Some(format!("{n:.1}x your baseline reply performance")),
                        performance_score: Some((n * 100.0).round() / 100.0),
                    }],
                },
                dedupe_key: format!("promote:{}", w.draft_id),
                source: "rules",
            }
        })
        .collect()
}

#[cfg(test)]
pub(crate) fn edit_distance_for_tests(a: &str, b: &str) -> i64 {
    levenshtein(a, b) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(id: &str, initial: &str, edited: &str) -> EditSample {
        EditSample { draft_id: id.into(), model_used: "claude".into(), initial: initial.into(), edited: edited.into(), edit_distance: levenshtein(initial, edited) as i64 }
    }

    fn style() -> StyleState {
        StyleState::empty()
    }

    #[test]
    fn repeated_removals_become_a_ban_proposal_with_evidence() {
        let samples: Vec<EditSample> = (0..6)
            .map(|i| sample(&format!("d{i}"), &format!("Great point! Shipping matters {i}"), &format!("Shipping matters {i}")))
            .collect();
        let rep = edit_report(&samples);
        assert_eq!(rep.edited_drafts, 6);
        assert_eq!(rep.removed[0].phrase, "great point");
        assert_eq!(rep.removed[0].count, 6);
        let ins = edit_insights(&rep, &style());
        let ban = ins.iter().find(|i| i.category == "anti_pattern").expect("ban proposal");
        assert_eq!(ban.patch.ops, vec![PatchOp::AddBanned { pattern: "great point".into() }]);
        assert!(ban.hypothesis.contains("6 of 6"));
        assert_eq!(ban.evidence.items.len(), 5);
        assert!(ban.evidence.items.iter().all(|e| e.draft_id.is_some()));
    }

    #[test]
    fn already_banned_phrases_are_not_proposed_again() {
        let samples: Vec<EditSample> = (0..4).map(|i| sample(&format!("d{i}"), "Great point! ok then", "ok then")).collect();
        let mut s = style();
        s.banned.push("Great Point".into());
        assert!(edit_insights(&edit_report(&samples), &s).iter().all(|i| i.category != "anti_pattern"));
    }

    #[test]
    fn too_few_edits_produce_nothing() {
        let samples = vec![sample("a", "Great point! hi", "hi"), sample("b", "Great point! yo", "yo")];
        assert!(edit_insights(&edit_report(&samples), &style()).is_empty());
    }

    #[test]
    fn consistent_shortening_proposes_a_length_rule() {
        let long = "This is a fairly long reply that goes on and on about the topic at hand without really needing to";
        let samples: Vec<EditSample> = (0..5).map(|i| sample(&format!("d{i}"), long, "Short and sharp take")).collect();
        let rep = edit_report(&samples);
        assert_eq!(rep.shortened_drafts, 5);
        let ins = edit_insights(&rep, &style());
        let rule = ins.iter().find(|i| i.dedupe_key == "rule:shorter").unwrap();
        assert!(matches!(&rule.patch.ops[0], PatchOp::AddRule { text } if text.contains("about 20") || text.contains("about 40")));
    }

    #[test]
    fn emoji_removal_proposes_a_no_emoji_rule() {
        let samples: Vec<EditSample> = (0..4).map(|i| sample(&format!("d{i}"), "love this take \u{1F525}", "love this take")).collect();
        let ins = edit_insights(&edit_report(&samples), &style());
        assert!(ins.iter().any(|i| i.patch.ops == vec![PatchOp::AddRule { text: "Do not use emoji".into() }]));
    }

    #[test]
    fn unedited_drafts_are_counted_but_not_diffed() {
        let rep = edit_report(&[sample("a", "same", "same"), sample("b", "x y z", "x z")]);
        assert_eq!((rep.drafts, rep.edited_drafts), (2, 1));
    }

    fn dis(id: &str, author: &str, reason: Option<&str>, dismissed: bool) -> DismissalRow {
        DismissalRow { evaluation_id: id.into(), author: author.into(), reason: reason.map(Into::into), score: 80.0, dismissed }
    }

    #[test]
    fn dismissal_reasons_and_authors_feed_the_rubric() {
        let mut rows: Vec<DismissalRow> = (0..4).map(|i| dis(&format!("w{i}"), "spammy", Some("Wrong niche"), true)).collect();
        rows.push(dis("x", "friend", None, true));
        rows.push(dis("y", "friend", None, false));
        let rep = dismissal_report(&rows);
        assert_eq!((rep.dismissed, rep.drafted), (5, 1));
        assert_eq!(rep.reasons[0], ("wrong niche".to_string(), 4));
        let ins = dismissal_insights(&rep, &rows, "base rubric");
        assert!(ins.iter().any(|i| i.patch.ops == vec![PatchOp::AddCriterion { text: "Skip posts outside your niche topics".into() }]));
        assert!(ins.iter().any(|i| i.dedupe_key == "dismiss_author:spammy"));
        assert!(!ins.iter().any(|i| i.dedupe_key == "dismiss_author:friend"), "never penalize an author you also replied to");
        assert!(dismissal_insights(&rep, &rows, "Skip posts outside your niche topics").iter().all(|i| i.dedupe_key != "dismiss:wrong niche"));
    }

    fn out_item(id: &str, followers: i64, minutes: f64, norm: f64, text: &str, media: bool, model: &str) -> OutcomeItem {
        OutcomeItem {
            draft_id: id.into(), evaluation_id: None, model_used: model.into(), initial_text: text.into(), final_text: text.into(), edit_distance: 0,
            author_handle: "a".into(), author_followers: Some(followers), target_text: text.into(), has_media: media, potential_score: Some(70.0),
            published_at: None, reply_age_minutes: Some(minutes), impressions: Some(100), engagement_rate: None, snapshot_age_hours: Some(30.0),
            mature: true, normalized: Some(norm),
        }
    }

    #[test]
    fn timing_and_tier_insights_need_enough_data_per_bucket() {
        let mut items = vec![];
        for i in 0..4 {
            items.push(out_item(&format!("fast{i}"), 5_000, 20.0, 2.0, "x", false, "m"));
        }
        for i in 0..6 {
            items.push(out_item(&format!("slow{i}"), 500, 240.0, 0.7, "x", false, "m"));
        }
        let rep = timing_report(&items);
        assert_eq!(rep.scored_items, 10);
        let ins = timing_insights(&rep, "");
        let timing = ins.iter().find(|i| i.category == "scout_timing").unwrap();
        assert!(timing.hypothesis.contains("15 to 60 minutes"));
        assert!(ins.iter().any(|i| i.category == "author_tier" && i.hypothesis.contains("1k to 10k")));
        assert!(timing_insights(&timing_report(&items[..5]), "").is_empty(), "under 8 scored items stays quiet");
    }

    #[test]
    fn rubric_evolution_finds_terms_only_in_top_replies() {
        let mut items = vec![];
        for i in 0..4 {
            items.push(out_item(&format!("t{i}"), 1000, 30.0, 2.0, "building bootstrapped startups is hard", false, "m"));
        }
        for i in 0..4 {
            items.push(out_item(&format!("f{i}"), 1000, 30.0, 0.4, "celebrity gossip roundup today", false, "m"));
        }
        for i in 0..3 {
            items.push(out_item(&format!("m{i}"), 1000, 30.0, 1.0, "random chatter", false, "m"));
        }
        let ins = rubric_insights(&items, "base", &[]);
        let t = ins.iter().find(|i| i.dedupe_key.starts_with("rubric_terms")).expect("term proposal");
        assert!(matches!(&t.patch.ops[0], PatchOp::AddCriterion { text } if text.contains("bootstrapped") && text.contains("startups")));
        assert!(!matches!(&t.patch.ops[0], PatchOp::AddCriterion { text } if text.contains("gossip")));
    }

    #[test]
    fn flop_phrases_become_ban_proposals_only_when_absent_from_winners() {
        let mut items = vec![];
        for i in 0..4 {
            items.push(out_item(&format!("f{i}"), 1000, 30.0, 0.3, "100% agree with you", false, "m"));
        }
        for i in 0..4 {
            items.push(out_item(&format!("t{i}"), 1000, 30.0, 2.0, "shipping beats planning", false, "m"));
        }
        for i in 0..3 {
            items.push(out_item(&format!("m{i}"), 1000, 30.0, 1.0, "neutral words here", false, "m"));
        }
        let ins = flop_insights(&items, &style());
        assert!(!ins.is_empty());
        assert!(ins.iter().all(|i| i.category == "anti_pattern"));
        assert!(ins.iter().any(|i| matches!(&i.patch.ops[0], PatchOp::AddBanned { pattern } if pattern.contains("agree"))));
    }

    #[test]
    fn winners_are_promoted_unless_already_examples() {
        let items = vec![
            out_item("a", 1000, 30.0, 2.5, "Shipping beats polishing every single time", false, "m"),
            out_item("b", 1000, 30.0, 1.0, "An average reply that did average", false, "m"),
            out_item("c", 1000, 30.0, 3.0, "Already an example in the library", false, "m"),
        ];
        let existing: HashSet<String> = ["already an example in the library".to_string()].into();
        let ins = winner_insights(&items, &existing);
        assert_eq!(ins.len(), 1);
        assert_eq!(ins[0].category, "few_shot_promotion");
        assert!(matches!(&ins[0].patch.ops[0], PatchOp::PromoteExample { draft_id, .. } if draft_id == "a"));
    }

    #[test]
    fn model_report_computes_keep_rate() {
        let samples = vec![
            sample("a", "hello there friend", "hello there friend"),
            sample("b", "hello there friend", "hello there friends"),
            EditSample { model_used: "grok".into(), ..sample("c", "completely different text", "nothing alike at all here") },
        ];
        let r = model_report(&samples, &[]);
        let claude = r.iter().find(|m| m.model == "claude").unwrap();
        assert_eq!((claude.drafts, claude.kept_as_is, claude.kept_lightly_edited), (2, 1, 2));
        let grok = r.iter().find(|m| m.model == "grok").unwrap();
        assert_eq!(grok.keep_rate, 0.0);
    }

    #[test]
    fn pearson_basic() {
        assert!((pearson(&[1.0, 2.0, 3.0], &[2.0, 4.0, 6.0]).unwrap() - 1.0).abs() < 1e-9);
        assert!(pearson(&[1.0, 1.0, 1.0], &[1.0, 2.0, 3.0]).is_none());
    }
}
