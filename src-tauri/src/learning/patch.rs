//! Structured patches (LRN-06), their preview, and the only code path that changes rubric or style
//! versions (LRN-07, LRN-08). Every apply creates a new immutable version; nothing is edited in place.

use super::textdiff::{line_diff, DiffLine};
use crate::error::{AppError, AppResult};
use crate::storage::repo;
use crate::util::{new_id, now_rfc3339};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const LEARNED_HEADING: &str = "Learned adjustments:";
const MAX_OPS: usize = 10;
const MAX_LINE: usize = 280;
const MAX_BANNED: usize = 80;

const DEFAULT_RUBRIC_BASE: &str = "A post is high potential when:\n\
- it is gaining momentum early (fresh, with engagement growing fast)\n\
- it invites a reply (a question, a claim, an open loop)\n\
- there is room for a sharp or funny take that adds something\n\
- the audience overlaps with the people you want to reach";

/// Rubric used as the base of the first learned version when Scout has not seeded one yet.
pub fn default_rubric_text(prefs: &repo::Preferences) -> String {
    let mut t = DEFAULT_RUBRIC_BASE.to_string();
    if !prefs.niche_topics.is_empty() {
        t.push_str(&format!("\nNiche topics: {}", prefs.niche_topics.join(", ")));
    }
    if !prefs.prefer_accounts.is_empty() {
        t.push_str(&format!("\nPrefer authors: {}", prefs.prefer_accounts.iter().map(|h| format!("@{h}")).collect::<Vec<_>>().join(", ")));
    }
    if !prefs.avoid_accounts.is_empty() {
        t.push_str(&format!("\nAvoid authors: {}", prefs.avoid_accounts.iter().map(|h| format!("@{h}")).collect::<Vec<_>>().join(", ")));
    }
    t.push_str(&format!("\nLanguages: {}", prefs.languages.join(", ")));
    t
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PatchTarget {
    Rubric,
    Style,
    Examples,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum PatchOp {
    AddCriterion { text: String },
    RemoveCriterion { text: String },
    SetWeight { key: String, value: f64 },
    AddRule { text: String },
    RemoveRule { text: String },
    AddBanned { pattern: String },
    RemoveBanned { pattern: String },
    PromoteExample {
        draft_id: String,
        example_text: String,
        source_context_text: Option<String>,
        why_it_works: Option<String>,
        performance_score: Option<f64>,
    },
}

impl PatchOp {
    fn target(&self) -> PatchTarget {
        match self {
            PatchOp::AddCriterion { .. } | PatchOp::RemoveCriterion { .. } | PatchOp::SetWeight { .. } => PatchTarget::Rubric,
            PatchOp::AddRule { .. } | PatchOp::RemoveRule { .. } | PatchOp::AddBanned { .. } | PatchOp::RemoveBanned { .. } => {
                PatchTarget::Style
            }
            PatchOp::PromoteExample { .. } => PatchTarget::Examples,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Patch {
    pub target: PatchTarget,
    pub ops: Vec<PatchOp>,
}

fn clean_line(label: &str, s: &str, max: usize) -> AppResult<String> {
    let t = s.trim();
    if t.is_empty() {
        return Err(AppError::Validation(format!("{label} cannot be empty")));
    }
    if t.chars().count() > max {
        return Err(AppError::Validation(format!("{label} is longer than {max} characters")));
    }
    if t.contains('\n') || t.contains('\r') {
        return Err(AppError::Validation(format!("{label} must be a single line")));
    }
    Ok(t.to_string())
}

impl Patch {
    /// Checks shape and limits. Free text only ever becomes prompt text, never an instruction to the app.
    pub fn validated(self) -> AppResult<Patch> {
        if self.ops.is_empty() {
            return Err(AppError::Validation("a patch needs at least one change".into()));
        }
        if self.ops.len() > MAX_OPS {
            return Err(AppError::Validation(format!("a patch can hold at most {MAX_OPS} changes")));
        }
        let mut ops = Vec::with_capacity(self.ops.len());
        for op in self.ops {
            if op.target() != self.target {
                return Err(AppError::Validation("patch mixes changes for different targets".into()));
            }
            ops.push(match op {
                PatchOp::AddCriterion { text } => PatchOp::AddCriterion { text: clean_line("criterion", &text, MAX_LINE)? },
                PatchOp::RemoveCriterion { text } => PatchOp::RemoveCriterion { text: clean_line("criterion", &text, MAX_LINE)? },
                PatchOp::SetWeight { key, value } => {
                    let k = key.trim().to_lowercase();
                    let ok = !k.is_empty() && k.len() <= 40 && k.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
                    if !ok {
                        return Err(AppError::Validation("weight names use lowercase letters, digits and underscores".into()));
                    }
                    if !value.is_finite() || !(0.0..=5.0).contains(&value) {
                        return Err(AppError::Validation("weights must be between 0 and 5".into()));
                    }
                    PatchOp::SetWeight { key: k, value: (value * 100.0).round() / 100.0 }
                }
                PatchOp::AddRule { text } => PatchOp::AddRule { text: clean_line("rule", &text, MAX_LINE)? },
                PatchOp::RemoveRule { text } => PatchOp::RemoveRule { text: clean_line("rule", &text, MAX_LINE)? },
                PatchOp::AddBanned { pattern } => PatchOp::AddBanned { pattern: clean_line("banned pattern", &pattern, MAX_BANNED)? },
                PatchOp::RemoveBanned { pattern } => PatchOp::RemoveBanned { pattern: clean_line("banned pattern", &pattern, MAX_BANNED)? },
                PatchOp::PromoteExample { draft_id, example_text, source_context_text, why_it_works, performance_score } => {
                    PatchOp::PromoteExample {
                        draft_id: clean_line("draft id", &draft_id, 80)?,
                        example_text: clean_line("example", &example_text.replace('\n', " "), 1000)?,
                        source_context_text: source_context_text.map(|s| s.trim().chars().take(1000).collect()).filter(|s: &String| !s.is_empty()),
                        why_it_works: why_it_works.map(|s| s.trim().chars().take(MAX_LINE).collect()).filter(|s: &String| !s.is_empty()),
                        performance_score: performance_score.filter(|p| p.is_finite()),
                    }
                }
            });
        }
        Ok(Patch { target: self.target, ops })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RubricState {
    pub version: Option<i64>,
    pub text: String,
    pub weights: BTreeMap<String, f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StyleState {
    pub version: Option<i64>,
    pub free_text: String,
    pub rules: Vec<String>,
    pub banned: Vec<String>,
}

pub fn active_rubric(conn: &Connection, profile_id: &str) -> AppResult<RubricState> {
    let row = conn
        .query_row(
            "SELECT version, scout_criteria_text, weights_json FROM learning_rubrics
             WHERE profile_id=?1 AND is_active=1 ORDER BY version DESC LIMIT 1",
            [profile_id],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)),
        )
        .optional()?;
    Ok(match row {
        Some((version, text, w)) => RubricState {
            version: Some(version),
            text,
            weights: serde_json::from_str(&w).map_err(|e| AppError::Other(format!("corrupt rubric weights: {e}")))?,
        },
        None => {
            let prefs = repo::get_preferences(conn, profile_id)?.unwrap_or_default();
            RubricState { version: None, text: default_rubric_text(&prefs), weights: BTreeMap::new() }
        }
    })
}

pub fn active_style(conn: &Connection, profile_id: &str) -> AppResult<StyleState> {
    let row = conn
        .query_row(
            "SELECT version, free_text_guide, learned_rules_json, banned_patterns_json FROM style_guides
             WHERE profile_id=?1 AND is_active=1 ORDER BY version DESC LIMIT 1",
            [profile_id],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?)),
        )
        .optional()?;
    let parse = |s: &str| -> AppResult<Vec<String>> {
        serde_json::from_str(s).map_err(|e| AppError::Other(format!("corrupt style JSON: {e}")))
    };
    Ok(match row {
        Some((version, free_text, rules, banned)) => {
            StyleState { version: Some(version), free_text, rules: parse(&rules)?, banned: parse(&banned)? }
        }
        None => StyleState { version: None, free_text: String::new(), rules: vec![], banned: vec![] },
    })
}

fn same(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() == b.trim().to_lowercase()
}

pub fn apply_rubric_ops(text: &str, weights: &BTreeMap<String, f64>, ops: &[PatchOp]) -> AppResult<(String, BTreeMap<String, f64>)> {
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let mut weights = weights.clone();
    for op in ops {
        match op {
            PatchOp::AddCriterion { text } => {
                let line = format!("- {text}");
                if lines.iter().any(|l| same(l, &line)) {
                    return Err(AppError::Validation(format!("the rubric already contains \"{text}\"")));
                }
                if !lines.iter().any(|l| l.trim() == LEARNED_HEADING) {
                    if lines.last().map(|l| !l.trim().is_empty()).unwrap_or(false) {
                        lines.push(String::new());
                    }
                    lines.push(LEARNED_HEADING.to_string());
                }
                lines.push(line);
            }
            PatchOp::RemoveCriterion { text } => {
                let line = format!("- {text}");
                let before = lines.len();
                lines.retain(|l| !same(l, &line));
                if lines.len() == before {
                    return Err(AppError::Validation(format!("the rubric no longer contains \"{text}\"")));
                }
            }
            PatchOp::SetWeight { key, value } => {
                weights.insert(key.clone(), *value);
            }
            _ => unreachable!("validated patches only mix ops of one target"),
        }
    }
    Ok((lines.join("\n"), weights))
}

pub fn apply_style_ops(rules: &[String], banned: &[String], ops: &[PatchOp]) -> AppResult<(Vec<String>, Vec<String>)> {
    let (mut rules, mut banned) = (rules.to_vec(), banned.to_vec());
    for op in ops {
        match op {
            PatchOp::AddRule { text } => {
                if rules.iter().any(|r| same(r, text)) {
                    return Err(AppError::Validation(format!("the style already has the rule \"{text}\"")));
                }
                rules.push(text.clone());
            }
            PatchOp::RemoveRule { text } => {
                let n = rules.len();
                rules.retain(|r| !same(r, text));
                if rules.len() == n {
                    return Err(AppError::Validation(format!("the style no longer has the rule \"{text}\"")));
                }
            }
            PatchOp::AddBanned { pattern } => {
                if banned.iter().any(|b| same(b, pattern)) {
                    return Err(AppError::Validation(format!("\"{pattern}\" is already banned")));
                }
                banned.push(pattern.clone());
            }
            PatchOp::RemoveBanned { pattern } => {
                let n = banned.len();
                banned.retain(|b| !same(b, pattern));
                if banned.len() == n {
                    return Err(AppError::Validation(format!("\"{pattern}\" is not in the banned list")));
                }
            }
            _ => unreachable!("validated patches only mix ops of one target"),
        }
    }
    Ok((rules, banned))
}

pub fn rubric_lines(text: &str, weights: &BTreeMap<String, f64>) -> Vec<String> {
    let mut l: Vec<String> = text.lines().map(str::to_string).collect();
    l.extend(weights.iter().map(|(k, v)| format!("weight {k} = {v}")));
    l
}

pub fn style_lines(rules: &[String], banned: &[String]) -> Vec<String> {
    rules.iter().map(|r| format!("rule: {r}")).chain(banned.iter().map(|b| format!("banned: {b}"))).collect()
}

/// What the patch would change, as diff lines. Writes nothing.
pub fn preview(conn: &Connection, profile_id: &str, patch: &Patch) -> AppResult<Vec<DiffLine>> {
    let patch = patch.clone().validated()?;
    match patch.target {
        PatchTarget::Rubric => {
            let cur = active_rubric(conn, profile_id)?;
            let (text, weights) = apply_rubric_ops(&cur.text, &cur.weights, &patch.ops)?;
            Ok(line_diff(&rubric_lines(&cur.text, &cur.weights), &rubric_lines(&text, &weights)))
        }
        PatchTarget::Style => {
            let cur = active_style(conn, profile_id)?;
            let (rules, banned) = apply_style_ops(&cur.rules, &cur.banned, &patch.ops)?;
            Ok(line_diff(&style_lines(&cur.rules, &cur.banned), &style_lines(&rules, &banned)))
        }
        PatchTarget::Examples => Ok(patch
            .ops
            .iter()
            .filter_map(|op| match op {
                PatchOp::PromoteExample { example_text, .. } => {
                    Some(DiffLine { kind: "add", text: format!("example: {example_text}") })
                }
                _ => None,
            })
            .collect()),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AppliedChange {
    pub kind: &'static str,
    pub from_version: Option<i64>,
    pub to_version: Option<i64>,
}

/// Creates the new version (or example rows) for an approved patch. The caller owns the transaction and
/// must only call this after an explicit human accept.
pub fn apply(conn: &Connection, profile_id: &str, patch: &Patch, insight_id: Option<&str>) -> AppResult<AppliedChange> {
    let patch = patch.clone().validated()?;
    let now = now_rfc3339();
    let change = match patch.target {
        PatchTarget::Rubric => {
            let cur = active_rubric(conn, profile_id)?;
            let (text, weights) = apply_rubric_ops(&cur.text, &cur.weights, &patch.ops)?;
            if text == cur.text && weights == cur.weights {
                return Err(AppError::Validation("this change is already part of the active rubric".into()));
            }
            conn.execute("UPDATE learning_rubrics SET is_active=0 WHERE profile_id=?1", [profile_id])?;
            // The onboarding-derived default is not stored until the first change; keep it as version one so it can be restored.
            let from = match cur.version {
                Some(v) => v,
                None => {
                    conn.execute(
                        "INSERT INTO learning_rubrics (profile_id, scout_criteria_text, weights_json, is_active, created_at)
                         VALUES (?1,?2,'{}',0,?3)",
                        params![profile_id, cur.text, now],
                    )?;
                    conn.last_insert_rowid()
                }
            };
            conn.execute(
                "INSERT INTO learning_rubrics (profile_id, scout_criteria_text, weights_json, is_active, created_at)
                 VALUES (?1,?2,?3,1,?4)",
                params![profile_id, text, serde_json::to_string(&weights).expect("weights serialize"), now],
            )?;
            AppliedChange { kind: "rubric", from_version: Some(from), to_version: Some(conn.last_insert_rowid()) }
        }
        PatchTarget::Style => {
            let cur = active_style(conn, profile_id)?;
            let (rules, banned) = apply_style_ops(&cur.rules, &cur.banned, &patch.ops)?;
            conn.execute("UPDATE style_guides SET is_active=0 WHERE profile_id=?1", [profile_id])?;
            let from = match cur.version {
                Some(v) => v,
                None => {
                    conn.execute(
                        "INSERT INTO style_guides (id, profile_id, version, free_text_guide, learned_rules_json, banned_patterns_json, is_active, created_at, note)
                         VALUES (?1,?2,1,?3,'[]','[]',0,?4,'Starting point before learning changes')",
                        params![new_id(), profile_id, cur.free_text, now],
                    )?;
                    1
                }
            };
            let next: i64 = conn.query_row(
                "SELECT COALESCE(MAX(version),0)+1 FROM style_guides WHERE profile_id=?1",
                [profile_id],
                |r| r.get(0),
            )?;
            conn.execute(
                "INSERT INTO style_guides (id, profile_id, version, free_text_guide, learned_rules_json, banned_patterns_json, is_active, created_at, note)
                 VALUES (?1,?2,?3,?4,?5,?6,1,?7,'Applied from Learning')",
                params![
                    new_id(),
                    profile_id,
                    next,
                    cur.free_text,
                    serde_json::to_string(&rules).expect("rules serialize"),
                    serde_json::to_string(&banned).expect("banned serialize"),
                    now
                ],
            )?;
            AppliedChange { kind: "style", from_version: Some(from), to_version: Some(next) }
        }
        PatchTarget::Examples => {
            for op in &patch.ops {
                let PatchOp::PromoteExample { draft_id, example_text, source_context_text, why_it_works, performance_score } = op else {
                    continue;
                };
                let status: Option<String> = conn
                    .query_row(
                        "SELECT status FROM reply_drafts WHERE id=?1 AND profile_id=?2",
                        params![draft_id, profile_id],
                        |r| r.get(0),
                    )
                    .optional()?;
                if status.as_deref() != Some("published") {
                    return Err(AppError::Validation("only drafts you actually published can become examples".into()));
                }
                let exists: i64 = conn.query_row(
                    "SELECT count(*) FROM style_examples WHERE profile_id=?1 AND lower(example_text)=lower(?2)",
                    params![profile_id, example_text],
                    |r| r.get(0),
                )?;
                if exists > 0 {
                    return Err(AppError::Validation("this reply is already in your examples".into()));
                }
                let id = new_id();
                conn.execute(
                    "INSERT INTO style_examples (id, profile_id, kind, source_context_text, example_text, why_it_works, tags_json, pinned, source, performance_score, created_at)
                     VALUES (?1,?2,'reply',?3,?4,?5,'[]',0,'promoted_winner',?6,?7)",
                    params![id, profile_id, source_context_text, example_text, why_it_works, performance_score, now],
                )?;
                conn.execute(
                    "INSERT INTO style_examples_fts (example_id, profile_id, kind, source_context_text, example_text, why_it_works, tags_text)
                     VALUES (?1, ?2, 'reply', ?3, ?4, ?5, '')",
                    params![id, profile_id, source_context_text.clone().unwrap_or_default(), example_text, why_it_works.clone().unwrap_or_default()],
                )?;
            }
            AppliedChange { kind: "examples", from_version: None, to_version: None }
        }
    };
    conn.execute(
        "INSERT INTO learning_changelog (id, profile_id, kind, action, from_version, to_version, insight_id, created_at)
         VALUES (?1,?2,?3,'accepted',?4,?5,?6,?7)",
        params![new_id(), profile_id, change.kind, change.from_version, change.to_version, insight_id, now],
    )?;
    Ok(change)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::storage::open;
    use tempfile::{tempdir, TempDir};

    pub(crate) fn db() -> (TempDir, Connection, String) {
        let dir = tempdir().unwrap();
        let (c, _) = open(&dir.path().join("a.db"), &dir.path().join("b")).unwrap();
        let pid = repo::ensure_default_profile(&c, "/p").unwrap();
        (dir, c, pid)
    }

    fn rule(t: &str) -> PatchOp {
        PatchOp::AddRule { text: t.into() }
    }

    #[test]
    fn validation_rejects_bad_patches() {
        let p = |target, ops| Patch { target, ops }.validated();
        assert!(p(PatchTarget::Style, vec![]).is_err());
        assert!(p(PatchTarget::Style, vec![rule("  ")]).is_err());
        assert!(p(PatchTarget::Style, vec![rule("two\nlines")]).is_err());
        assert!(p(PatchTarget::Rubric, vec![rule("wrong target")]).is_err());
        assert!(p(PatchTarget::Rubric, vec![PatchOp::SetWeight { key: "Bad Key".into(), value: 1.0 }]).is_err());
        assert!(p(PatchTarget::Rubric, vec![PatchOp::SetWeight { key: "k".into(), value: 9.0 }]).is_err());
        assert!(p(PatchTarget::Style, (0..11).map(|i| rule(&format!("r{i}"))).collect()).is_err());
        assert!(p(PatchTarget::Style, vec![PatchOp::AddBanned { pattern: "x".repeat(81) }]).is_err());
        let ok = p(PatchTarget::Style, vec![rule("  keep it short  ")]).unwrap();
        assert_eq!(ok.ops, vec![rule("keep it short")]);
    }

    #[test]
    fn patch_json_round_trips() {
        let p = Patch { target: PatchTarget::Style, ops: vec![PatchOp::AddBanned { pattern: "great point".into() }] };
        let j = serde_json::to_string(&p).unwrap();
        assert!(j.contains(r#""op":"add_banned""#));
        assert_eq!(serde_json::from_str::<Patch>(&j).unwrap(), p);
        assert!(serde_json::from_str::<Patch>(r#"{"target":"style","ops":[{"op":"run_shell","cmd":"x"}]}"#).is_err());
    }

    #[test]
    fn rubric_patch_creates_new_immutable_version_from_default() {
        let (_d, c, pid) = db();
        let patch = Patch {
            target: PatchTarget::Rubric,
            ops: vec![
                PatchOp::AddCriterion { text: "Prefer replies within 15-60 minutes".into() },
                PatchOp::SetWeight { key: "freshness".into(), value: 1.5 },
            ],
        };
        let lines = preview(&c, &pid, &patch).unwrap();
        assert!(lines.iter().any(|l| l.kind == "add" && l.text == "- Prefer replies within 15-60 minutes"));
        assert!(lines.iter().any(|l| l.kind == "add" && l.text == "weight freshness = 1.5"));
        let n: i64 = c.query_row("SELECT count(*) FROM learning_rubrics", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0, "preview writes nothing");

        let ch = apply(&c, &pid, &patch, Some("i1")).unwrap();
        assert_eq!((ch.from_version, ch.to_version), (Some(1), Some(2)), "the default is stored as version 1 so it can be restored");
        let cur = active_rubric(&c, &pid).unwrap();
        assert_eq!(cur.version, Some(2));
        assert!(cur.text.contains(LEARNED_HEADING));
        assert!(cur.text.starts_with("A post is high potential"));

        let ch2 = apply(&c, &pid, &Patch { target: PatchTarget::Rubric, ops: vec![PatchOp::AddCriterion { text: "Skip engagement bait".into() }] }, None).unwrap();
        assert_eq!((ch2.from_version, ch2.to_version), (Some(2), Some(3)));
        let active: i64 = c.query_row("SELECT count(*) FROM learning_rubrics WHERE is_active=1", [], |r| r.get(0)).unwrap();
        assert_eq!(active, 1);
        assert_eq!(active_rubric(&c, &pid).unwrap().text.matches(LEARNED_HEADING).count(), 1);

        let err = c.execute("UPDATE learning_rubrics SET scout_criteria_text='x' WHERE version=1", []).unwrap_err();
        assert!(err.to_string().contains("immutable"));
        let log: i64 = c.query_row("SELECT count(*) FROM learning_changelog WHERE action='accepted'", [], |r| r.get(0)).unwrap();
        assert_eq!(log, 2);
    }

    #[test]
    fn default_rubric_carries_onboarding_answers() {
        let (_d, c, pid) = db();
        repo::save_preferences(&c, &pid, repo::Preferences {
            niche_topics: vec!["rust".into()],
            prefer_accounts: vec!["naval".into()],
            avoid_accounts: vec!["spam1".into()],
            languages: vec!["en".into(), "de".into()],
        }).unwrap();
        let t = active_rubric(&c, &pid).unwrap().text;
        assert!(t.contains("Niche topics: rust") && t.contains("@naval") && t.contains("@spam1") && t.contains("en, de"));
    }

    #[test]
    fn style_patch_versions_rules_and_banned_patterns() {
        let (_d, c, pid) = db();
        let p = Patch { target: PatchTarget::Style, ops: vec![rule("Keep replies under 140 characters"), PatchOp::AddBanned { pattern: "great point".into() }] };
        let ch = apply(&c, &pid, &p, None).unwrap();
        assert_eq!((ch.from_version, ch.to_version), (Some(1), Some(2)));
        let s = active_style(&c, &pid).unwrap();
        assert_eq!(s.rules, vec!["Keep replies under 140 characters"]);
        assert_eq!(s.banned, vec!["great point"]);

        c.execute("UPDATE style_guides SET free_text_guide='Dry and curious' WHERE version=2", []).unwrap();
        apply(&c, &pid, &Patch { target: PatchTarget::Style, ops: vec![PatchOp::AddBanned { pattern: "love this".into() }] }, None).unwrap();
        let s = active_style(&c, &pid).unwrap();
        assert_eq!((s.version, s.free_text.as_str()), (Some(3), "Dry and curious"), "free-text guide is carried forward untouched");
        assert_eq!(s.banned.len(), 2);
        let again = apply(&c, &pid, &Patch { target: PatchTarget::Style, ops: vec![PatchOp::AddBanned { pattern: "GREAT POINT".into() }] }, None);
        assert!(again.is_err(), "duplicates are rejected");
        let rm = apply(&c, &pid, &Patch { target: PatchTarget::Style, ops: vec![PatchOp::RemoveRule { text: "nope".into() }] }, None);
        assert!(rm.is_err());
    }

    pub(crate) fn seed_published_draft(c: &Connection, pid: &str, id: &str, text: &str) {
        c.execute("INSERT OR IGNORE INTO observed_posts (id, profile_id, platform_post_id, author_handle, text, feed_source, first_seen_at, metrics_at_capture_json) VALUES ('op-'||?1, ?2, 'pp-'||?1, 'someone', 'orig text', 'for_you', '2026-01-01T00:00:00Z', '{}')", params![id, pid]).unwrap();
        c.execute("INSERT INTO reply_drafts (id, profile_id, observed_post_id, initial_text, edited_text, model_used, style_guide_version, status, created_at) VALUES (?1, ?2, 'op-'||?1, ?3, ?3, 'claude', 1, 'published', '2026-01-01T00:00:00Z')", params![id, pid, text]).unwrap();
    }

    #[test]
    fn promoting_examples_requires_a_published_draft_and_skips_duplicates() {
        let (_d, c, pid) = db();
        let promote = |id: &str, text: &str| Patch {
            target: PatchTarget::Examples,
            ops: vec![PatchOp::PromoteExample { draft_id: id.into(), example_text: text.into(), source_context_text: Some("orig".into()), why_it_works: Some("2x baseline".into()), performance_score: Some(2.0) }],
        };
        assert!(apply(&c, &pid, &promote("missing", "hi there"), None).is_err());
        seed_published_draft(&c, &pid, "d1", "Shipping beats polishing every time");
        apply(&c, &pid, &promote("d1", "Shipping beats polishing every time"), Some("i9")).unwrap();
        let (src, pinned): (String, i64) = c.query_row("SELECT source, pinned FROM style_examples", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!((src.as_str(), pinned), ("promoted_winner", 0));
        let hits: i64 = c.query_row("SELECT count(*) FROM style_examples_fts WHERE style_examples_fts MATCH 'beats'", [], |r| r.get(0)).unwrap();
        assert_eq!(hits, 1, "promoted examples must be searchable by Phase 2 retrieval");
        assert!(apply(&c, &pid, &promote("d1", "shipping beats polishing every time"), None).is_err());
    }
}
