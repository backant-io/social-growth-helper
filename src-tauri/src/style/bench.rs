//! Style Test Bench, constraint & banned-phrase post-filter, and example import (STY-04..STY-06, STY-09).

use super::repo::{get_active_style_guide, retrieve_examples_for_prompt};
use super::{
    EmojiPolicy, ExampleKind, HashtagPolicy, ImportCandidate, RetrievedExample, StyleGuide,
};
use crate::browser::service::BrowserService;
use crate::error::AppResult;
use crate::llm::prompts::get_active_prompt;
use crate::llm::router::{LlmRouter, RouteOverride};
use crate::llm::{
    BillingLane, JsonSchemaSpec, LlmContentPart, LlmMessage, LlmRequest, LlmRole, LlmUsage,
    ProviderType, TaskType,
};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StyleBenchInput {
    pub sample_author: Option<String>,
    pub sample_post_text: String,
    pub override_route: Option<RouteOverrideInput>,
    pub compare_route: Option<RouteOverrideInput>,
    pub example_limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteOverrideInput {
    pub provider_account_id: String,
    pub model_id: String,
    pub reasoning_effort: Option<String>,
    pub temperature: Option<f32>,
}

impl From<RouteOverrideInput> for RouteOverride {
    fn from(r: RouteOverrideInput) -> Self {
        Self {
            provider_account_id: r.provider_account_id,
            model_id: r.model_id,
            reasoning_effort: r.reasoning_effort,
            temperature: r.temperature,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConstraintCheck {
    pub passed: bool,
    pub char_count: usize,
    pub max_reply_chars: u32,
    pub banned_hits: Vec<String>,
    pub emoji_count: usize,
    pub hashtag_count: usize,
    pub violations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchVariant {
    pub tone_tag: String,
    pub text: String,
    pub check: ConstraintCheck,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchModelRun {
    pub model_id: String,
    pub provider_type: ProviderType,
    pub provider_label: String,
    pub billing_lane: BillingLane,
    pub used_fallback: bool,
    pub fallback_reason: Option<String>,
    pub latency_ms: u64,
    pub usage: LlmUsage,
    pub variants: Vec<BenchVariant>,
    pub rejected_variants: Vec<BenchVariant>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StyleBenchResult {
    pub style_guide_version: u32,
    pub prompt_version: u32,
    pub system_prompt: String,
    pub rendered_user_prompt: String,
    pub retrieved_examples: Vec<RetrievedExample>,
    pub primary: BenchModelRun,
    pub comparison: Option<BenchModelRun>,
}

pub fn reply_variants_schema() -> JsonSchemaSpec {
    JsonSchemaSpec {
        name: "style_bench_replies".into(),
        schema: json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["variants"],
            "properties": {
                "variants": {
                    "type": "array",
                    "minItems": 2,
                    "maxItems": 3,
                    "items": {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["tone_tag", "text"],
                        "properties": {
                            "tone_tag": { "type": "string", "minLength": 1 },
                            "text": { "type": "string", "minLength": 1 }
                        }
                    }
                }
            }
        }),
    }
}

fn is_emoji_char(c: char) -> bool {
    let cp = c as u32;
    (0x1F300..=0x1FAFF).contains(&cp)
        || (0x2600..=0x27BF).contains(&cp)
        || (0x1F1E6..=0x1F1FF).contains(&cp)
}

fn count_hashtags(text: &str) -> usize {
    text.split_whitespace()
        .filter(|tok| {
            let clean = tok.trim_start_matches(|c: char| !c.is_alphanumeric() && c != '#');
            clean.starts_with('#') && clean.len() > 1 && clean[1..].chars().any(|c| c.is_alphanumeric())
        })
        .count()
}

pub fn check_reply_constraints(text: &str, guide: &StyleGuide) -> ConstraintCheck {
    check_text_constraints(text, &guide.banned_patterns, guide.max_reply_chars, guide.emoji_policy, guide.hashtag_policy)
}

/// Length, banned-phrase, emoji and hashtag rules shared by the Style Test Bench and draft generation.
pub fn check_text_constraints(
    text: &str,
    banned_patterns: &[String],
    max_reply_chars: u32,
    emoji_policy: EmojiPolicy,
    hashtag_policy: HashtagPolicy,
) -> ConstraintCheck {
    let char_count = text.chars().count();
    let lower = text.to_lowercase();
    let mut banned_hits = Vec::new();
    for pat in banned_patterns {
        let p = pat.trim();
        if !p.is_empty() && lower.contains(&p.to_lowercase()) {
            banned_hits.push(p.to_string());
        }
    }

    let emoji_count = text.chars().filter(|c| is_emoji_char(*c)).count();
    let hashtag_count = count_hashtags(text);

    let mut violations = Vec::new();
    if !banned_hits.is_empty() {
        violations.push(format!("Contains banned pattern(s): {}", banned_hits.join(", ")));
    }
    if char_count > max_reply_chars as usize {
        violations.push(format!(
            "Exceeds max length ({char_count} > {} chars)",
            max_reply_chars
        ));
    }
    match emoji_policy {
        EmojiPolicy::None if emoji_count > 0 => {
            violations.push(format!("Emoji policy is 'none' (found {emoji_count})"));
        }
        EmojiPolicy::Minimal if emoji_count > 1 => {
            violations.push(format!("Emoji policy is 'minimal' (found {emoji_count}, max 1)"));
        }
        _ => {}
    }
    match hashtag_policy {
        HashtagPolicy::None if hashtag_count > 0 => {
            violations.push(format!("Hashtag policy is 'none' (found {hashtag_count})"));
        }
        HashtagPolicy::Max1 if hashtag_count > 1 => {
            violations.push(format!(
                "Hashtag policy is 'max_1' (found {hashtag_count}, max 1)"
            ));
        }
        _ => {}
    }

    ConstraintCheck {
        passed: violations.is_empty(),
        char_count,
        max_reply_chars: max_reply_chars,
        banned_hits,
        emoji_count,
        hashtag_count,
        violations,
    }
}

fn render_rules_block(guide: &StyleGuide) -> String {
    let mut lines = vec![
        format!("- Maximum length: {} characters.", guide.max_reply_chars),
        format!("- Emoji policy: {}.", guide.emoji_policy.as_str()),
        format!("- Hashtag policy: {}.", guide.hashtag_policy.as_str()),
    ];
    if !guide.learned_rules.is_empty() {
        lines.push(format!("- Learned style rules: {}", guide.learned_rules.join(" | ")));
    }
    if !guide.banned_patterns.is_empty() {
        lines.push(format!(
            "- STRICTLY FORBIDDEN phrases/patterns (never output these): {}",
            guide.banned_patterns.join(", ")
        ));
    }
    lines.push("- Return 2 or 3 distinct reply variants as JSON: {\"variants\":[{\"tone_tag\":\"...\",\"text\":\"...\"}]}.".into());
    lines.join("\n")
}

fn render_examples_block(examples: &[RetrievedExample]) -> String {
    if examples.is_empty() {
        return "(No examples saved yet.)".into();
    }
    examples
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let ctx = item
                .example
                .source_context_text
                .as_deref()
                .map(|c| format!("Context: {c}\n"))
                .unwrap_or_default();
            let why = item
                .example
                .why_it_works
                .as_deref()
                .map(|w| format!("\nWhy it works: {w}"))
                .unwrap_or_default();
            format!(
                "Example {} [{}]:\n{}Reply: {}{}",
                i + 1,
                item.reason,
                ctx,
                item.example.example_text,
                why
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

pub fn render_reply_bench_prompt(
    template_user: &str,
    guide: &StyleGuide,
    examples: &[RetrievedExample],
    author: &str,
    post_text: &str,
) -> String {
    let rules = render_rules_block(guide);
    let ex_block = render_examples_block(examples);
    template_user
        .replace("{{style_guide}}", &guide.free_text_guide)
        .replace("{{rules}}", &rules)
        .replace("{{examples}}", &ex_block)
        .replace("{{author}}", author)
        .replace("{{post_text}}", post_text)
}

fn parse_variants_and_filter(raw: Option<&Value>, text: &str, guide: &StyleGuide) -> (Vec<BenchVariant>, Vec<BenchVariant>) {
    let parsed_val = raw
        .cloned()
        .or_else(|| serde_json::from_str::<Value>(text).ok());
    let mut accepted = Vec::new();
    let mut rejected = Vec::new();

    if let Some(arr) = parsed_val.as_ref().and_then(|v| v["variants"].as_array()) {
        for item in arr {
            let tone = item["tone_tag"].as_str().unwrap_or("direct").trim().to_string();
            let vtext = item["text"].as_str().unwrap_or("").trim().to_string();
            if vtext.is_empty() {
                continue;
            }
            let check = check_reply_constraints(&vtext, guide);
            let variant = BenchVariant {
                tone_tag: tone,
                text: vtext,
                check,
            };
            // Post-filter: banned patterns NEVER pass into `variants`; move them to `rejected_variants`.
            if variant.check.banned_hits.is_empty() {
                accepted.push(variant);
            } else {
                rejected.push(variant);
            }
        }
    }
    (accepted, rejected)
}

pub async fn run_style_bench(
    db: &Mutex<Connection>,
    router: &LlmRouter,
    profile_id: &str,
    input: StyleBenchInput,
) -> AppResult<StyleBenchResult> {
    let post_text = input.sample_post_text.trim();
    if post_text.is_empty() {
        return Err(crate::error::AppError::Validation(
            "Sample post text cannot be empty".into(),
        ));
    }
    let author = input
        .sample_author
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("@sample_author")
        .to_string();
    let limit = input.example_limit.unwrap_or(5).clamp(1, 10);

    let (guide, retrieved, prompt) = {
        let conn = db.lock().unwrap();
        let guide = get_active_style_guide(&conn, profile_id)?;
        let retrieved =
            retrieve_examples_for_prompt(&conn, profile_id, ExampleKind::Reply, post_text, limit)?;
        let prompt = get_active_prompt(&conn, TaskType::ReplyDrafting)?;
        (guide, retrieved, prompt)
    };

    let rendered_user =
        render_reply_bench_prompt(&prompt.user_template, &guide, &retrieved, &author, post_text);
    let base_req = LlmRequest {
        task_type: TaskType::ReplyDrafting,
        model_id: String::new(),
        system_prompt: Some(prompt.system_prompt.clone()),
        messages: vec![LlmMessage {
            role: LlmRole::User,
            parts: vec![LlmContentPart::Text {
                text: rendered_user.clone(),
            }],
        }],
        temperature: None,
        reasoning_effort: None,
        max_tokens: Some(600),
        json_schema: Some(reply_variants_schema()),
        prompt_version: Some(prompt.version),
    };

    let primary_resp = router
        .complete(db, base_req.clone(), input.override_route.map(Into::into))
        .await
        .map_err(crate::error::AppError::Llm)?;
    let (p_ok, p_rej) =
        parse_variants_and_filter(primary_resp.structured_json.as_ref(), &primary_resp.text, &guide);
    let primary = BenchModelRun {
        model_id: primary_resp.model_id,
        provider_type: primary_resp.provider_type,
        provider_label: primary_resp.provider_label,
        billing_lane: primary_resp.billing_lane,
        used_fallback: primary_resp.used_fallback,
        fallback_reason: primary_resp.fallback_reason,
        latency_ms: primary_resp.latency_ms,
        usage: primary_resp.usage,
        variants: p_ok,
        rejected_variants: p_rej,
    };

    let comparison = if let Some(cmp_route) = input.compare_route {
        let cmp_resp = router
            .complete(db, base_req, Some(cmp_route.into()))
            .await
            .map_err(crate::error::AppError::Llm)?;
        let (c_ok, c_rej) =
            parse_variants_and_filter(cmp_resp.structured_json.as_ref(), &cmp_resp.text, &guide);
        Some(BenchModelRun {
            model_id: cmp_resp.model_id,
            provider_type: cmp_resp.provider_type,
            provider_label: cmp_resp.provider_label,
            billing_lane: cmp_resp.billing_lane,
            used_fallback: cmp_resp.used_fallback,
            fallback_reason: cmp_resp.fallback_reason,
            latency_ms: cmp_resp.latency_ms,
            usage: cmp_resp.usage,
            variants: c_ok,
            rejected_variants: c_rej,
        })
    } else {
        None
    };

    Ok(StyleBenchResult {
        style_guide_version: guide.version,
        prompt_version: prompt.version,
        system_prompt: prompt.system_prompt,
        rendered_user_prompt: rendered_user,
        retrieved_examples: retrieved,
        primary,
        comparison,
    })
}

/// Parses pasted text (separated by blank lines or `---`) into candidate examples (STY-04).
pub fn parse_pasted_examples(raw: &str, kind: ExampleKind) -> Vec<ImportCandidate> {
    let normalized = raw.replace("\r\n", "\n");
    let mut blocks: Vec<String> = Vec::new();
    let mut cur = Vec::new();

    for line in normalized.lines() {
        let t = line.trim();
        if t == "---" || t.is_empty() {
            if !cur.is_empty() {
                blocks.push(cur.join(" ").trim().to_string());
                cur.clear();
            }
        } else {
            cur.push(t.to_string());
        }
    }
    if !cur.is_empty() {
        blocks.push(cur.join(" ").trim().to_string());
    }

    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for block in blocks {
        if block.len() < 8 {
            continue;
        }
        let key = block.to_lowercase();
        if !seen.insert(key) {
            continue;
        }
        out.push(ImportCandidate {
            kind,
            source_context_text: None,
            example_text: block,
            tags: Vec::new(),
            source: "pasted".into(),
            author_handle: None,
            external_id: None,
        });
    }
    out
}

/// Imports candidate examples from the currently open X browser window (STY-04).
/// If `handle` is provided, navigates to `https://x.com/<handle>/with_replies` (for replies)
/// or `https://x.com/<handle>` (for OGC) before reading visible tweets.
pub async fn import_examples_from_browser(
    browser: &BrowserService,
    handle: Option<&str>,
    kind: ExampleKind,
) -> AppResult<Vec<ImportCandidate>> {
    if !browser.is_running().await {
        return Err(crate::error::AppError::Validation(
            "Open the X browser window first (Settings -> Open X Window) before importing from X.".into(),
        ));
    }

    let clean_handle = handle
        .map(|h| h.trim().trim_start_matches('@').to_string())
        .filter(|h| !h.is_empty());

    if let Some(ref h) = clean_handle {
        let url = match kind {
            ExampleKind::Reply => format!("https://x.com/{h}/with_replies"),
            ExampleKind::Ogc => format!("https://x.com/{h}"),
        };
        browser.navigate(&url).await?;
        tokio::time::sleep(std::time::Duration::from_millis(1800)).await;
    }

    let tweets = browser.read_visible_tweets().await?;
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for t in tweets {
        let text = t.text.trim().to_string();
        if text.is_empty() {
            continue;
        }
        if let Some(ref h) = clean_handle {
            let author = t.handle.as_deref().unwrap_or("").trim_start_matches('@');
            if !author.is_empty() && !author.eq_ignore_ascii_case(h) {
                continue;
            }
        }
        if !seen.insert(text.to_lowercase()) {
            continue;
        }
        out.push(ImportCandidate {
            kind,
            source_context_text: None,
            example_text: text,
            tags: Vec::new(),
            source: "x_import".into(),
            author_handle: t.handle.clone().or_else(|| clean_handle.clone()),
            external_id: t.id,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::providers::mock::MockLlmProvider;
    use crate::llm::repo::{
        save_task_route, upsert_provider_account, ProviderAccount, ProviderAccountConfig,
        TaskModelRoutePatch,
    };
    use crate::llm::{BillingLane, ProviderCapabilities, ProviderType};
    use crate::storage::{open, repo as base_repo};
    use crate::style::repo::save_style_guide_version;
    use crate::style::StyleGuideInput;
    use std::sync::Arc;
    use tempfile::tempdir;

    #[test]
    fn constraint_checker_catches_banned_length_emoji_and_hashtags() {
        let guide = StyleGuide {
            id: "g1".into(),
            profile_id: "p1".into(),
            version: 1,
            free_text_guide: "Sharp".into(),
            learned_rules: vec![],
            banned_patterns: vec!["game changer".into()],
            max_reply_chars: 40,
            emoji_policy: EmojiPolicy::None,
            hashtag_policy: HashtagPolicy::None,
            note: None,
            is_active: true,
            created_at: String::new(),
        };

        let bad = check_reply_constraints("This is a total Game Changer 🚀 #ai #growth and way too long!", &guide);
        assert!(!bad.passed);
        assert_eq!(bad.banned_hits, vec!["game changer".to_string()]);
        assert_eq!(bad.emoji_count, 1);
        assert_eq!(bad.hashtag_count, 2);
        assert!(bad.violations.len() >= 4);

        let good = check_reply_constraints("Latency dropped 38% after batching.", &guide);
        assert!(good.passed);
        assert!(good.violations.is_empty());
    }

    #[test]
    fn paste_parser_splits_and_deduplicates_candidates() {
        let raw = "First crisp reply here.\n\n---\n\nSecond crisp reply\nacross two lines.\n\nFirst crisp reply here.\n\nhi";
        let out = parse_pasted_examples(raw, ExampleKind::Reply);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].example_text, "First crisp reply here.");
        assert_eq!(out[1].example_text, "Second crisp reply across two lines.");
    }

    #[tokio::test]
    async fn bench_post_filters_banned_phrases_and_returns_prompt_inspection() {
        let dir = tempdir().unwrap();
        let (conn, _) = open(&dir.path().join("a.db"), &dir.path().join("b")).unwrap();
        let db = Mutex::new(conn);
        let pid = {
            let conn = db.lock().unwrap();
            let pid = base_repo::ensure_default_profile(&conn, "/tmp/prof").unwrap();
            save_style_guide_version(
                &conn,
                &pid,
                StyleGuideInput {
                    free_text_guide: "Technical and concise.".into(),
                    learned_rules: vec![],
                    banned_patterns: vec!["game changer".into()],
                    max_reply_chars: 220,
                    emoji_policy: EmojiPolicy::None,
                    hashtag_policy: HashtagPolicy::None,
                    note: None,
                },
            )
            .unwrap();
            upsert_provider_account(
                &conn,
                &ProviderAccount {
                    id: "acc-1".into(),
                    provider_type: ProviderType::AnthropicApi,
                    label: "Mock".into(),
                    keychain_ref: None,
                    billing_lane: BillingLane::Api,
                    status: "connected".into(),
                    last_verified_at: None,
                    last_error: None,
                    config: ProviderAccountConfig::default(),
                },
            )
            .unwrap();
            save_task_route(
                &conn,
                TaskModelRoutePatch {
                    task_type: TaskType::ReplyDrafting,
                    provider_account_id: Some("acc-1".into()),
                    model_id: Some("claude-sonnet-4-5".into()),
                    reasoning_effort: None,
                    temperature: None,
                    fallback_account_id: None,
                    fallback_model_id: None,
                    max_calls_per_hour: None,
                },
            )
            .unwrap();
            pid
        };

        let router = LlmRouter::new();
        let mock = Arc::new(MockLlmProvider::new(
            "acc-1",
            ProviderType::AnthropicApi,
            ProviderCapabilities {
                vision: true,
                json_schema: true,
                streaming: false,
                thinking: false,
            },
        ));
        mock.push_ok(
            r#"{"variants":[
                {"tone_tag":"contrarian","text":"Batching queries cut our p99 from 140ms to 19ms."},
                {"tone_tag":"hype","text":"This feature is a total game changer for teams!"}
            ]}"#,
        );
        router.register_provider(mock);

        let res = run_style_bench(
            &db,
            &router,
            &pid,
            StyleBenchInput {
                sample_author: Some("@dev".into()),
                sample_post_text: "Why did your p99 drop?".into(),
                override_route: None,
                compare_route: None,
                example_limit: Some(5),
            },
        )
        .await
        .unwrap();

        assert_eq!(res.primary.variants.len(), 1);
        assert_eq!(res.primary.rejected_variants.len(), 1);
        assert!(res.primary.variants[0].text.contains("19ms"));
        assert_eq!(res.primary.rejected_variants[0].check.banned_hits, vec!["game changer"]);
        assert!(res.rendered_user_prompt.contains("Why did your p99 drop?"));
    }
}
