//! The reflection pass (LRN-05): runs when you ask for it or when the app opens and a pass is due. There is
//! no background daemon. It only ever creates proposed insights; it never changes a rubric or style guide
//! and never touches X.
//!
//! The light pass reads edit diffs and dismissals. The deep pass adds outcomes (timing, tier, rubric
//! evolution, anti-patterns, winners) and, when an analysis model is connected, a model-written synthesis.

use super::analyzers::*;
use super::outcomes::{self, OutcomeItem};
use super::patch::{self, Patch};
use super::store;
use crate::error::{AppError, AppResult};
use crate::llm::router::LlmRouter;
use crate::storage::repo;
use crate::util::{new_id, now_rfc3339};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

/// A light pass runs on app open when the last pass is older than this.
pub const AUTO_INTERVAL_HOURS: i64 = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Light,
    Deep,
}

impl Mode {
    fn as_str(self) -> &'static str {
        match self {
            Mode::Light => "light",
            Mode::Deep => "deep",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ReflectionSummary {
    pub run_id: String,
    pub mode: Mode,
    pub published_items: usize,
    pub min_sample: usize,
    pub low_confidence: bool,
    pub insights_created: usize,
    pub used_llm: bool,
    pub note: String,
    pub started_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct OutcomeBrief {
    pub draft_id: String,
    pub text: String,
    pub normalized: Option<f64>,
    pub impressions: Option<i64>,
    pub mature: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Signals {
    pub published_items: usize,
    pub mature_items: usize,
    pub baseline_impressions: Option<f64>,
    pub min_sample: usize,
    pub low_confidence: bool,
    pub edit: EditReport,
    pub dismissal: DismissalReport,
    pub models: Vec<ModelStat>,
    pub timing: TimingReport,
    pub rubric_correlation: Option<f64>,
    pub best: Vec<OutcomeBrief>,
    pub weakest: Vec<OutcomeBrief>,
    pub last_run: Option<ReflectionSummary>,
    pub proposed: i64,
}

fn brief(i: &OutcomeItem) -> OutcomeBrief {
    OutcomeBrief { draft_id: i.draft_id.clone(), text: i.final_text.clone(), normalized: i.normalized, impressions: i.impressions, mature: i.mature }
}

pub fn last_run(conn: &Connection, profile_id: &str) -> AppResult<Option<ReflectionSummary>> {
    let mut stmt = conn.prepare(
        "SELECT id, mode, started_at, published_items, min_sample, insights_created, used_llm, COALESCE(note,'')
         FROM learning_runs WHERE profile_id=?1 ORDER BY started_at DESC, rowid DESC LIMIT 1",
    )?;
    let mut rows = stmt.query([profile_id])?;
    Ok(match rows.next()? {
        Some(r) => {
            let published: i64 = r.get(3)?;
            let min: i64 = r.get(4)?;
            Some(ReflectionSummary {
                run_id: r.get(0)?,
                mode: if r.get::<_, String>(1)? == "deep" { Mode::Deep } else { Mode::Light },
                started_at: r.get(2)?,
                published_items: published as usize,
                min_sample: min as usize,
                low_confidence: published < min,
                insights_created: r.get::<_, i64>(5)? as usize,
                used_llm: r.get::<_, i64>(6)? != 0,
                note: r.get(7)?,
            })
        }
        None => None,
    })
}

pub fn compute_signals(conn: &Connection, profile_id: &str) -> AppResult<Signals> {
    let (items, baseline) = outcomes::load(conn, profile_id)?;
    let samples = load_edit_samples(conn, profile_id, None)?;
    let dismissals = load_dismissals(conn, profile_id, None)?;
    let min = store::min_sample(conn)?;
    let mut scored: Vec<&OutcomeItem> = items.iter().filter(|i| i.normalized.is_some()).collect();
    scored.sort_by(|a, b| b.normalized.partial_cmp(&a.normalized).unwrap());
    Ok(Signals {
        published_items: items.len(),
        mature_items: items.iter().filter(|i| i.mature).count(),
        baseline_impressions: baseline,
        min_sample: min,
        low_confidence: items.len() < min,
        edit: edit_report(&samples),
        dismissal: dismissal_report(&dismissals),
        models: model_report(&samples, &items),
        timing: timing_report(&items),
        rubric_correlation: rubric_correlation(&items),
        best: scored.iter().take(3).map(|i| brief(i)).collect(),
        weakest: scored.iter().rev().take(3).filter(|i| i.normalized.map(|n| n <= 0.6).unwrap_or(false)).map(|i| brief(i)).collect(),
        last_run: last_run(conn, profile_id)?,
        proposed: store::proposed_count(conn, profile_id)?,
    })
}

fn niche_topics(conn: &Connection, profile_id: &str) -> AppResult<Vec<String>> {
    Ok(repo::get_preferences(conn, profile_id)?.unwrap_or_default().niche_topics)
}

fn existing_examples(conn: &Connection, profile_id: &str) -> AppResult<HashSet<String>> {
    let mut stmt = conn.prepare("SELECT lower(trim(example_text)) FROM style_examples WHERE profile_id=?1")?;
    let rows = stmt.query_map([profile_id], |r| r.get::<_, String>(0))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Rule-based proposals for the given mode. Pure reads.
pub fn rule_proposals(conn: &Connection, profile_id: &str, mode: Mode) -> AppResult<Vec<InsightDraft>> {
    let style = patch::active_style(conn, profile_id)?;
    let rubric = patch::active_rubric(conn, profile_id)?;
    let samples = load_edit_samples(conn, profile_id, None)?;
    let dismissals = load_dismissals(conn, profile_id, None)?;
    let mut out = edit_insights(&edit_report(&samples), &style);
    out.extend(dismissal_insights(&dismissal_report(&dismissals), &dismissals, &rubric.text));
    if mode == Mode::Deep {
        let (items, _) = outcomes::load(conn, profile_id)?;
        out.extend(timing_insights(&timing_report(&items), &rubric.text));
        out.extend(rubric_insights(&items, &rubric.text, &niche_topics(conn, profile_id)?));
        out.extend(flop_insights(&items, &style));
        out.extend(winner_insights(&items, &existing_examples(conn, profile_id)?));
    }
    Ok(out)
}

/// Runs one pass and stores new proposals. `extra` carries model-written proposals already validated by
/// `parse_llm_proposals`; they go through the same guard and dedupe as rule-based ones.
pub fn run(conn: &Connection, profile_id: &str, mode: Mode, extra: Vec<InsightDraft>, note_extra: Option<&str>) -> AppResult<ReflectionSummary> {
    let tx = conn.unchecked_transaction()?;
    let min = store::min_sample(&tx)?;
    let (items, _) = outcomes::load(&tx, profile_id)?;
    let published = items.len();
    let used_llm = !extra.is_empty();
    let mut proposals = rule_proposals(&tx, profile_id, mode)?;
    proposals.extend(extra);
    let now = now_rfc3339();
    let cycle = now.split('T').next().unwrap_or("").to_string();
    let mut created = 0;
    for p in &proposals {
        if store::insert_draft(&tx, profile_id, p, published, min, &cycle)?.is_some() {
            created += 1;
        }
    }
    let mut note = String::new();
    if published < min {
        note.push_str(&format!("Only {published} published replies so far (minimum {min}); suggestions are marked low confidence. "));
    }
    if created == 0 {
        note.push_str("Nothing new to suggest. ");
    }
    if let Some(n) = note_extra {
        note.push_str(n);
    }
    let note = note.trim().to_string();
    let run_id = new_id();
    tx.execute(
        "INSERT INTO learning_runs (id, profile_id, mode, started_at, published_items, min_sample, insights_created, used_llm, note)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
        params![run_id, profile_id, mode.as_str(), now, published as i64, min as i64, created as i64, used_llm as i64, note],
    )?;
    tx.commit()?;
    Ok(ReflectionSummary { run_id, mode, published_items: published, min_sample: min, low_confidence: published < min, insights_created: created, used_llm, note, started_at: now })
}

/// Light pass when none ran in the last `AUTO_INTERVAL_HOURS` and there is anything to learn from.
pub fn run_if_due(conn: &Connection, profile_id: &str, now_unix: i64) -> AppResult<Option<ReflectionSummary>> {
    if let Some(last) = last_run(conn, profile_id)? {
        if let Some(t) = outcomes::parse_ts(&last.started_at) {
            if now_unix - t < AUTO_INTERVAL_HOURS * 3600 {
                return Ok(None);
            }
        }
    }
    let has_data: i64 = conn.query_row(
        "SELECT (SELECT count(*) FROM reply_drafts WHERE profile_id=?1 AND (status='published' OR user_approved_at IS NOT NULL))
              + (SELECT count(*) FROM scout_evaluations e JOIN observed_posts o ON o.id=e.observed_post_id WHERE o.profile_id=?1 AND e.status='dismissed')",
        [profile_id],
        |r| r.get(0),
    )?;
    if has_data == 0 {
        return Ok(None);
    }
    run(conn, profile_id, Mode::Light, vec![], None).map(Some)
}

// ---------------------------------------------------------------------------------------------
// Model synthesis (LRN-05 deep pass). `RouterLlm` calls the model through the Phase 2 router; everything it
// returns is parsed and validated here.
// ---------------------------------------------------------------------------------------------

pub type LlmFuture<'a> = Pin<Box<dyn Future<Output = Result<String, String>> + Send + 'a>>;

/// Sends a prompt to the `learning_analysis` route and returns the raw model text.
pub trait ReflectionLlm: Send + Sync {
    fn complete<'a>(&'a self, system: &'a str, user: &'a str) -> LlmFuture<'a>;
}

/// Adapter that sends the deep-pass prompt through the Phase 2 router on the `learning_analysis` route.
pub struct RouterLlm {
    router: Arc<LlmRouter>,
    db: Arc<Mutex<Connection>>,
}

impl RouterLlm {
    pub const TASK: &'static str = "learning_analysis";

    pub fn new(router: Arc<LlmRouter>, db: Arc<Mutex<Connection>>) -> Self {
        Self { router, db }
    }
}

impl ReflectionLlm for RouterLlm {
    fn complete<'a>(&'a self, system: &'a str, user: &'a str) -> LlmFuture<'a> {
        Box::pin(async move {
            self.router
                .complete_json_for_task(&self.db, Self::TASK, system, user)
                .await
                .map_err(|e| e.to_string())
        })
    }
}

pub const LLM_SYSTEM_PROMPT: &str = "You review how a person edits AI-drafted replies and how their posts performed. \
Propose at most 3 small, concrete changes. Reply with JSON only: {\"proposals\":[{\"category\":string,\"hypothesis\":string,\
\"confidence\":number between 0 and 1,\"evidence\":[string],\"patch\":{\"target\":\"rubric\"|\"style\",\"ops\":[{\"op\":\"add_criterion\"|\"remove_criterion\"|\
\"set_weight\"|\"add_rule\"|\"remove_rule\"|\"add_banned\"|\"remove_banned\", ...}]}}]}. Only propose changes the numbers support. \
Never propose actions on X, never include secrets or personal data.";

pub fn build_llm_prompt(signals: &Signals) -> String {
    let rem: Vec<String> = signals.edit.removed.iter().take(5).map(|p| format!("\"{}\" x{}", p.phrase, p.count)).collect();
    let add: Vec<String> = signals.edit.added.iter().take(5).map(|p| format!("\"{}\" x{}", p.phrase, p.count)).collect();
    let reasons: Vec<String> = signals.dismissal.reasons.iter().take(5).map(|(r, n)| format!("{r} x{n}")).collect();
    let best: Vec<String> = signals.best.iter().map(|b| format!("{:.1}x: {}", b.normalized.unwrap_or(0.0), b.text)).collect();
    let weak: Vec<String> = signals.weakest.iter().map(|b| format!("{:.1}x: {}", b.normalized.unwrap_or(0.0), b.text)).collect();
    format!(
        "Published replies: {}. Edited drafts: {} of {}. Average length change when editing: {:.0}%.\nPhrases you remove: {}.\nPhrases you add: {}.\nDismissed cards: {} (reasons: {}).\nBest replies (vs personal baseline): {}.\nWeakest replies: {}.",
        signals.published_items,
        signals.edit.edited_drafts,
        signals.edit.drafts,
        signals.edit.avg_length_change_pct,
        rem.join(", "),
        add.join(", "),
        signals.dismissal.dismissed,
        reasons.join(", "),
        best.join(" | "),
        weak.join(" | "),
    )
}

#[derive(Deserialize)]
struct LlmEnvelope {
    proposals: Vec<serde_json::Value>,
}

#[derive(Deserialize)]
struct LlmProposal {
    hypothesis: String,
    #[serde(default)]
    confidence: f64,
    #[serde(default)]
    evidence: Vec<String>,
    patch: Patch,
}

const LLM_CONFIDENCE_CAP: f64 = 0.8;
const LLM_MAX_PROPOSALS: usize = 3;

/// Parses and validates model output. Invalid proposals are dropped rather than repaired, and the model can
/// only produce rubric or style patches (never example promotion, which needs a measured outcome).
pub fn parse_llm_proposals(raw: &str) -> Vec<InsightDraft> {
    let start = raw.find('{');
    let end = raw.rfind('}');
    let Some((s, e)) = start.zip(end).filter(|(s, e)| e > s) else { return vec![] };
    let Ok(env) = serde_json::from_str::<LlmEnvelope>(&raw[s..=e]) else { return vec![] };
    env.proposals
        .into_iter()
        .filter_map(|v| serde_json::from_value::<LlmProposal>(v).ok())
        .filter(|p| p.patch.target != patch::PatchTarget::Examples && !p.hypothesis.trim().is_empty())
        .filter_map(|p| {
            let patch = p.patch.validated().ok()?;
            let hypothesis: String = p.hypothesis.trim().chars().take(400).collect();
            let key = format!("llm:{}", serde_json::to_string(&patch).ok()?);
            Some(InsightDraft {
                category: "model_suggestion",
                hypothesis,
                evidence: Evidence {
                    support: p.evidence.len(),
                    total: p.evidence.len(),
                    items: p.evidence.into_iter().take(5).map(|e| EvidenceItem { label: e.chars().take(200).collect(), draft_id: None, detail: String::new() }).collect(),
                },
                confidence: p.confidence.clamp(0.0, LLM_CONFIDENCE_CAP),
                patch,
                dedupe_key: key,
                source: "llm",
            })
        })
        .take(LLM_MAX_PROPOSALS)
        .collect()
}

/// Deep pass with an analysis model. A model failure never fails the pass: the rule-based proposals still land.
pub async fn run_deep_with_llm(
    lock: &std::sync::Mutex<Connection>,
    profile_id: &str,
    llm: &dyn ReflectionLlm,
) -> AppResult<ReflectionSummary> {
    let prompt = {
        let db = lock.lock().map_err(|_| AppError::Other("database lock poisoned".into()))?;
        build_llm_prompt(&compute_signals(&db, profile_id)?)
    };
    let (extra, note) = match llm.complete(LLM_SYSTEM_PROMPT, &prompt).await {
        Ok(text) => {
            let p = parse_llm_proposals(&text);
            let n = if p.is_empty() { Some("The analysis model returned no usable suggestions.".to_string()) } else { None };
            (p, n)
        }
        Err(e) => (vec![], Some(format!("The analysis model call failed ({e}); rule-based suggestions only."))),
    };
    let db = lock.lock().map_err(|_| AppError::Other("database lock poisoned".into()))?;
    run(&db, profile_id, Mode::Deep, extra, note.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::learning::patch::tests::{db, seed_published_draft};
    use crate::learning::patch::{active_rubric, active_style};

    fn seed_edited(c: &Connection, pid: &str, i: usize, initial: &str, edited: &str) {
        seed_published_draft(c, pid, &format!("d{i}"), edited);
        c.execute("UPDATE reply_drafts SET initial_text=?1, edited_text=?2, edit_distance=?3 WHERE id=?4", params![initial, edited, crate::learning::analyzers::edit_distance_for_tests(initial, edited), format!("d{i}")]).unwrap();
    }

    #[test]
    fn light_pass_creates_cards_but_applies_nothing() {
        let (_d, c, pid) = db();
        for i in 0..6 {
            seed_edited(&c, &pid, i, &format!("Great point! Shipping beats planning {i}"), &format!("Shipping beats planning {i}"));
        }
        let before_rubric = active_rubric(&c, &pid).unwrap();
        let s = run(&c, &pid, Mode::Light, vec![], None).unwrap();
        assert!(s.insights_created >= 1);
        assert!(s.low_confidence, "6 published replies is below the default 30");
        assert!(s.note.contains("low confidence"));
        let cards = store::list(&c, &pid, &["proposed"]).unwrap();
        assert!(cards.iter().all(|i| i.low_confidence && i.confidence <= 0.5));
        assert!(cards.iter().any(|i| i.category == "anti_pattern" && i.hypothesis.contains("great point")));
        assert_eq!(active_style(&c, &pid).unwrap().version, None, "a pass never writes a style version");
        assert_eq!(active_rubric(&c, &pid).unwrap(), before_rubric);

        let again = run(&c, &pid, Mode::Light, vec![], None).unwrap();
        assert_eq!(again.insights_created, 0, "no duplicate cards on a second pass");
        assert!(last_run(&c, &pid).unwrap().is_some());
    }

    #[test]
    fn confidence_is_not_capped_once_the_minimum_sample_is_met() {
        let (_d, c, pid) = db();
        store::set_min_sample(&c, 5).unwrap();
        for i in 0..6 {
            seed_edited(&c, &pid, i, &format!("Great point! Shipping beats planning {i}"), &format!("Shipping beats planning {i}"));
        }
        let s = run(&c, &pid, Mode::Light, vec![], None).unwrap();
        assert!(!s.low_confidence);
        assert!(store::list(&c, &pid, &["proposed"]).unwrap().iter().any(|i| !i.low_confidence && i.confidence > 0.5));
    }

    #[test]
    fn auto_pass_respects_the_interval_and_needs_data() {
        let (_d, c, pid) = db();
        let now = outcomes::parse_ts(&now_rfc3339()).unwrap();
        assert!(run_if_due(&c, &pid, now).unwrap().is_none(), "nothing to learn from yet");
        seed_edited(&c, &pid, 0, "a b c", "a c");
        assert!(run_if_due(&c, &pid, now).unwrap().is_some());
        assert!(run_if_due(&c, &pid, now + 3600).unwrap().is_none());
        assert!(run_if_due(&c, &pid, now + AUTO_INTERVAL_HOURS * 3600 + 5).unwrap().is_some());
    }

    #[test]
    fn llm_output_is_validated_capped_and_never_promotes_examples() {
        let raw = r#"Sure! {"proposals":[
          {"category":"x","hypothesis":"You prefer short openers","confidence":0.99,"evidence":["a","b"],
           "patch":{"target":"style","ops":[{"op":"add_rule","text":"Open with the point"}]}},
          {"category":"x","hypothesis":"bad","confidence":0.5,"patch":{"target":"style","ops":[{"op":"add_rule","text":"two\nlines"}]}},
          {"category":"x","hypothesis":"promote","confidence":0.5,"patch":{"target":"examples","ops":[{"op":"promote_example","draft_id":"d","example_text":"hello hello hello hello"}]}},
          {"category":"x","hypothesis":"shell","confidence":0.5,"patch":{"target":"style","ops":[{"op":"run_shell","cmd":"rm"}]}}
        ]} trailing"#;
        let p = parse_llm_proposals(raw);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].confidence, 0.8);
        assert_eq!(p[0].source, "llm");
        assert!(parse_llm_proposals("not json").is_empty());
        assert!(parse_llm_proposals("{\"proposals\": 4}").is_empty());
    }

    struct FakeLlm(Result<String, String>);
    impl ReflectionLlm for FakeLlm {
        fn complete<'a>(&'a self, _s: &'a str, _u: &'a str) -> LlmFuture<'a> {
            let r = self.0.clone();
            Box::pin(async move { r })
        }
    }

    #[tokio::test]
    async fn deep_pass_with_a_model_adds_its_proposals_and_survives_model_failure() {
        let (_d, c, pid) = db();
        let lock = std::sync::Mutex::new(c);
        let ok = FakeLlm(Ok(r#"{"proposals":[{"category":"x","hypothesis":"Lead with the point","confidence":0.6,"evidence":["e"],"patch":{"target":"style","ops":[{"op":"add_rule","text":"Lead with the point"}]}}]}"#.into()));
        let s = run_deep_with_llm(&lock, &pid, &ok).await.unwrap();
        assert!(s.used_llm);
        assert_eq!(s.insights_created, 1);

        let bad = FakeLlm(Err("timeout".into()));
        let s = run_deep_with_llm(&lock, &pid, &bad).await.unwrap();
        assert!(!s.used_llm);
        assert!(s.note.contains("timeout"));
    }

    #[tokio::test]
    async fn router_llm_uses_the_learning_analysis_route_and_reports_an_unset_route() {
        use crate::llm::providers::mock::MockLlmProvider;
        use crate::llm::repo::{ProviderAccount, ProviderAccountConfig, TaskModelRoutePatch};
        use crate::llm::{ProviderCapabilities, ProviderType, TaskType};

        let (_d, c, pid) = db();
        let db = Arc::new(Mutex::new(c));
        let router = Arc::new(LlmRouter::new());
        let llm = RouterLlm::new(router.clone(), db.clone());

        let s = run_deep_with_llm(&db, &pid, &llm).await.unwrap();
        assert!(!s.used_llm, "no route configured yet");
        assert!(s.note.contains("analysis model call failed"), "{}", s.note);

        {
            let conn = db.lock().unwrap();
            crate::llm::repo::upsert_provider_account(
                &conn,
                &ProviderAccount {
                    id: "p1".into(),
                    provider_type: ProviderType::AnthropicApi,
                    label: "p1".into(),
                    keychain_ref: None,
                    billing_lane: ProviderType::AnthropicApi.billing_lane(),
                    status: "connected".into(),
                    last_verified_at: None,
                    last_error: None,
                    config: ProviderAccountConfig::default(),
                },
            )
            .unwrap();
            crate::llm::repo::save_task_route(
                &conn,
                TaskModelRoutePatch {
                    task_type: TaskType::LearningAnalysis,
                    provider_account_id: Some("p1".into()),
                    model_id: Some("m1".into()),
                    reasoning_effort: None,
                    temperature: None,
                    fallback_account_id: None,
                    fallback_model_id: None,
                    max_calls_per_hour: None,
                },
            )
            .unwrap();
        }
        let mock = Arc::new(MockLlmProvider::new(
            "p1",
            ProviderType::AnthropicApi,
            ProviderCapabilities { vision: false, json_schema: true, streaming: false, thinking: false },
        ));
        mock.push_ok(r#"```json
{"proposals":[{"category":"x","hypothesis":"Lead with the point","confidence":0.6,"evidence":["e"],"patch":{"target":"style","ops":[{"op":"add_rule","text":"Lead with the point"}]}}]}
```"#);
        router.register_provider(mock.clone());

        let s = run_deep_with_llm(&db, &pid, &llm).await.unwrap();
        assert!(s.used_llm, "{}", s.note);
        assert_eq!(s.insights_created, 1);
        let calls = mock.recorded_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].task_type, TaskType::LearningAnalysis);
        let conn = db.lock().unwrap();
        let cards = store::list(&conn, &pid, &["proposed"]).unwrap();
        assert!(cards.iter().any(|i| i.source == "llm"));
    }

    #[test]
    fn signals_report_counts_and_the_low_sample_flag() {
        let (_d, c, pid) = db();
        seed_edited(&c, &pid, 0, "Great point! hi there", "hi there");
        let sg = compute_signals(&c, &pid).unwrap();
        assert_eq!((sg.published_items, sg.min_sample, sg.low_confidence), (1, 30, true));
        assert_eq!(sg.edit.edited_drafts, 1);
        assert_eq!(sg.models.len(), 1);
    }
}
