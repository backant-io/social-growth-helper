//! X analytics extraction scripts & two-layer parsers (ANL-02, ANL-03, ANL-04, ANL-06, ANL-16, ANL-17).
//!
//! Layer 1: Passive GraphQL response bodies matched by operation name (`AccountOverviewQuery`,
//! `TweetActivityQuery`, `UserTweets`, `UserTweetsAndReplies`) — never by rotating query hash (SPK-04).
//! Layer 2: Read-only DOM extraction via `data-testid`, ARIA labels, and semantic links.
//! Layer 3: Non-Premium fallback via profile header + public tweet counters / view counts.

use crate::browser::CapturedNetworkResponse;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const ACCOUNT_ANALYTICS_URL: &str = "https://x.com/i/account_analytics";
pub const ACCOUNT_CONTENT_URL: &str = "https://x.com/i/account_analytics/content";

pub const ACCOUNT_GQL_FILTERS: &[&str] = &["AccountOverviewQuery", "Analytics", "UserByScreenName"];
pub const POST_GQL_FILTERS: &[&str] = &["TweetActivityQuery", "TweetDetail", "Analytics"];
pub const PROFILE_GQL_FILTERS: &[&str] = &["UserTweets", "UserTweetsAndReplies", "UserByScreenName"];

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct AccountMetrics {
    pub handle: Option<String>,
    pub impressions: Option<i64>,
    pub engagement_rate: Option<f64>,
    pub profile_visits: Option<i64>,
    pub followers_total: Option<i64>,
    pub is_premium_upsell: bool,
    pub is_logged_out: bool,
    pub source: String,
    pub raw_json: Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct PostMetrics {
    pub platform_post_id: String,
    pub text: Option<String>,
    pub posted_at: Option<String>,
    pub is_reply: bool,
    pub impressions: Option<i64>,
    pub likes: Option<i64>,
    pub replies: Option<i64>,
    pub reposts: Option<i64>,
    pub bookmarks: Option<i64>,
    pub profile_visits: Option<i64>,
    pub link_clicks: Option<i64>,
    pub engagement_rate: Option<f64>,
    pub source: String,
    pub raw_json: Value,
}

/// Read-only DOM probe for `x.com/i/account_analytics` (SPK-04, ANL-02, ANL-06).
pub const ACCOUNT_ANALYTICS_DOM_JS: &str = r#"(() => {
  const url = location.href || '';
  const bodyText = (document.body?.innerText || '').slice(0, 6000);
  const loggedOut = /\/i\/(flow\/login|jf\/onboarding\/web)|\/login\b/i.test(url);
  const metricCards = {};
  for (const el of document.querySelectorAll('[data-testid]')) {
    const id = el.getAttribute('data-testid') || '';
    if (/(impression|engagement|profile|follow|visit|metric|stat)/i.test(id)) {
      metricCards[id] = (el.innerText || '').trim().slice(0, 120);
    }
  }
  const hasMetrics = Object.keys(metricCards).length > 0;
  const isPremiumUpsell = !hasMetrics && /(subscribe|x premium|upgrade to premium|unlock analytics)/i.test(bodyText);
  const switcher = document.querySelector('[data-testid="SideNav_AccountSwitcher_Button"]')?.innerText || '';
  const handleMatch = switcher.match(/@([A-Za-z0-9_]{1,15})/);
  return {
    url,
    loggedOut,
    isPremiumUpsell,
    handle: handleMatch ? handleMatch[1] : null,
    metricCards,
    bodySnippet: bodyText.slice(0, 2000)
  };
})()"#;

/// Read-only DOM probe for `<post_url>/analytics` or status page fallback (ANL-03, ANL-04, ANL-06).
pub const POST_ANALYTICS_DOM_JS: &str = r#"(() => {
  const url = location.href || '';
  const bodyText = (document.body?.innerText || '').slice(0, 4000);
  const grab = (sel) => document.querySelector(sel)?.innerText?.trim() || null;
  const ariaCount = (sel) => {
    const el = document.querySelector(sel);
    return el ? (el.getAttribute('aria-label') || el.innerText || '').trim() : null;
  };
  const viewLink = document.querySelector('a[href*="/analytics"]');
  return {
    url,
    impressions: grab('[data-testid="impressions"]') || (viewLink?.getAttribute('aria-label') || viewLink?.innerText || null),
    engagements: grab('[data-testid="engagements"]'),
    likes: ariaCount('[data-testid="like"], [data-testid="unlike"]'),
    replies: ariaCount('[data-testid="reply"]'),
    reposts: ariaCount('[data-testid="retweet"], [data-testid="unretweet"]'),
    bookmarks: ariaCount('[data-testid="bookmark"], [data-testid="removeBookmark"]'),
    tweetText: grab('[data-testid="tweetText"]'),
    postedAt: document.querySelector('article[data-testid="tweet"] time')?.getAttribute('datetime') || null,
    bodySnippet: bodyText.slice(0, 1500)
  };
})()"#;

/// Read-only DOM probe for profile Posts / Replies tabs (ANL-06 non-Premium fallback, ANL-16 backfill, ANL-17 external matching).
pub const PROFILE_TIMELINE_DOM_JS: &str = r#"(() => {
  const url = location.href || '';
  const followerLink = document.querySelector('a[href$="/verified_followers"], a[href$="/followers"]');
  const followersText = followerLink ? (followerLink.innerText || followerLink.getAttribute('title') || '').trim() : null;
  const switcher = document.querySelector('[data-testid="SideNav_AccountSwitcher_Button"]')?.innerText || '';
  const handleMatch = switcher.match(/@([A-Za-z0-9_]{1,15})/);
  const posts = [];
  for (const a of document.querySelectorAll('article[data-testid="tweet"]')) {
    const link = a.querySelector('a[href*="/status/"] time')?.closest('a');
    const m = (link?.getAttribute('href') || '').match(/^\/([^/]+)\/status\/(\d+)/);
    if (!m) continue;
    const metric = (sel) => {
      const el = a.querySelector(sel);
      return el ? (el.getAttribute('aria-label') || el.innerText || '').trim() : null;
    };
    const viewsEl = a.querySelector('a[href*="/analytics"]');
    posts.push({
      id: m[2],
      handle: m[1],
      text: a.querySelector('[data-testid="tweetText"]')?.innerText || '',
      postedAt: a.querySelector('time')?.getAttribute('datetime') || null,
      isReply: /replying to/i.test(a.innerText || ''),
      likes: metric('[data-testid="like"], [data-testid="unlike"]'),
      replies: metric('[data-testid="reply"]'),
      reposts: metric('[data-testid="retweet"], [data-testid="unretweet"]'),
      bookmarks: metric('[data-testid="bookmark"], [data-testid="removeBookmark"]'),
      views: viewsEl ? (viewsEl.getAttribute('aria-label') || viewsEl.innerText || '').trim() : null
    });
  }
  return {
    url,
    handle: handleMatch ? handleMatch[1] : null,
    followersText,
    posts
  };
})()"#;

/// Parses X shorthand counters (`"1.2K"`, `"3.4M"`, `"1,234 Views"`, `"42 Likes"`).
pub fn parse_compact_count(raw: &str) -> Option<i64> {
    let cleaned = raw.replace(',', "");
    for token in cleaned.split_whitespace() {
        let t = token.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '.' && c != '-');
        if t.is_empty() {
            continue;
        }
        let lower = t.to_lowercase();
        let (num_part, mult) = if let Some(n) = lower.strip_suffix('k') {
            (n, 1_000.0)
        } else if let Some(n) = lower.strip_suffix('m') {
            (n, 1_000_000.0)
        } else if let Some(n) = lower.strip_suffix('b') {
            (n, 1_000_000_000.0)
        } else {
            (lower.as_str(), 1.0)
        };
        if let Ok(v) = num_part.parse::<f64>() {
            return Some((v * mult).round() as i64);
        }
    }
    None
}

/// Parses a percentage string like `"4.25%"` or `"4.25"`.
pub fn parse_percent(raw: &str) -> Option<f64> {
    for token in raw.split_whitespace() {
        let t = token.trim_end_matches('%').trim_matches(|c: char| !c.is_ascii_digit() && c != '.' && c != '-');
        if let Ok(v) = t.parse::<f64>() {
            return Some((v * 100.0).round() / 100.0);
        }
    }
    None
}

fn find_number_by_keys(val: &Value, keys: &[&str]) -> Option<f64> {
    match val {
        Value::Object(map) => {
            for k in keys {
                if let Some(v) = map.get(*k) {
                    if let Some(n) = v.as_f64() {
                        return Some(n);
                    }
                    if let Some(s) = v.as_str() {
                        if let Some(p) = parse_percent(s) {
                            return Some(p);
                        }
                        if let Some(c) = parse_compact_count(s) {
                            return Some(c as f64);
                        }
                    }
                }
            }
            for v in map.values() {
                if let Some(found) = find_number_by_keys(v, keys) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(arr) => {
            for v in arr {
                if let Some(found) = find_number_by_keys(v, keys) {
                    return Some(found);
                }
            }
            None
        }
        _ => None,
    }
}

fn find_int_by_keys(val: &Value, keys: &[&str]) -> Option<i64> {
    find_number_by_keys(val, keys).map(|f| f.round() as i64)
}

fn extract_labeled_metric(text: &str, label: &str) -> Option<String> {
    let lower = text.to_lowercase();
    let idx = lower.find(&label.to_lowercase())?;
    let after = &text[idx + label.len()..];
    for line in after.lines().take(3) {
        let trimmed = line.trim();
        if trimmed.chars().any(|c| c.is_ascii_digit()) {
            return Some(trimmed.to_string());
        }
    }
    None
}

/// Parses account overview metrics using passive GraphQL responses first, falling back to DOM (ANL-02, ANL-04, ANL-06).
pub fn parse_account_overview(network: &[CapturedNetworkResponse], dom: &Value) -> AccountMetrics {
    let mut out = AccountMetrics {
        handle: dom.get("handle").and_then(Value::as_str).map(ToString::to_string),
        is_logged_out: dom.get("loggedOut").and_then(Value::as_bool).unwrap_or(false),
        is_premium_upsell: dom.get("isPremiumUpsell").and_then(Value::as_bool).unwrap_or(false),
        raw_json: dom.clone(),
        ..Default::default()
    };

    // Layer 1: GraphQL passive capture (`AccountOverviewQuery`)
    for resp in network {
        if resp.status != 200 {
            continue;
        }
        let lower_url = resp.url.to_lowercase();
        if !lower_url.contains("accountoverview") && !lower_url.contains("analytics") {
            continue;
        }
        let Ok(parsed) = serde_json::from_str::<Value>(&resp.body) else { continue };
        let imp = find_int_by_keys(&parsed, &["impressions", "Impressions", "total_impressions"]);
        let eng_rate = find_number_by_keys(&parsed, &["engagement_rate", "engagementRate"]);
        let visits = find_int_by_keys(&parsed, &["profile_visits", "profileVisits", "ProfileVisits"]);
        let followers = find_int_by_keys(&parsed, &["followers_count", "followersCount", "followers", "followers_total"]);
        if imp.is_some() || eng_rate.is_some() || visits.is_some() || followers.is_some() {
            out.impressions = imp;
            out.engagement_rate = eng_rate.map(|v| if v > 0.0 && v < 1.0 { (v * 10000.0).round() / 100.0 } else { v });
            out.profile_visits = visits;
            out.followers_total = followers;
            out.is_premium_upsell = false;
            out.source = "graphql".into();
            out.raw_json = parsed;
            return out;
        }
    }

    // Layer 2: DOM fallback
    let cards = dom.get("metricCards").and_then(Value::as_object);
    let snippet = dom.get("bodySnippet").and_then(Value::as_str).unwrap_or_default();

    if let Some(map) = cards {
        for (k, v) in map {
            let kl = k.to_lowercase();
            let text = v.as_str().unwrap_or_default();
            if kl.contains("impression") && out.impressions.is_none() {
                out.impressions = parse_compact_count(text);
            } else if kl.contains("engagement") && out.engagement_rate.is_none() {
                out.engagement_rate = parse_percent(text);
            } else if (kl.contains("visit") || kl.contains("profile")) && out.profile_visits.is_none() {
                out.profile_visits = parse_compact_count(text);
            } else if kl.contains("follow") && out.followers_total.is_none() {
                out.followers_total = parse_compact_count(text);
            }
        }
    }

    if out.impressions.is_none() {
        out.impressions = extract_labeled_metric(snippet, "Impressions").and_then(|s| parse_compact_count(&s));
    }
    if out.engagement_rate.is_none() {
        out.engagement_rate = extract_labeled_metric(snippet, "Engagement rate").and_then(|s| parse_percent(&s));
    }
    if out.profile_visits.is_none() {
        out.profile_visits = extract_labeled_metric(snippet, "Profile visits").and_then(|s| parse_compact_count(&s));
    }
    if out.followers_total.is_none() {
        out.followers_total = extract_labeled_metric(snippet, "Followers").and_then(|s| parse_compact_count(&s));
    }

    if out.impressions.is_some()
        || out.engagement_rate.is_some()
        || out.profile_visits.is_some()
        || out.followers_total.is_some()
    {
        out.source = "dom_fallback".into();
        out.is_premium_upsell = false;
    } else if out.is_premium_upsell {
        out.source = "non_premium_upsell".into();
    } else {
        out.source = "empty".into();
    }

    out
}

/// Parses per-post metrics using passive `TweetActivityQuery` first, falling back to DOM / public counters (ANL-03, ANL-04, ANL-06).
pub fn parse_post_metrics(
    platform_post_id: &str,
    network: &[CapturedNetworkResponse],
    dom: &Value,
) -> PostMetrics {
    let mut out = PostMetrics {
        platform_post_id: platform_post_id.to_string(),
        text: dom.get("tweetText").and_then(Value::as_str).map(ToString::to_string),
        posted_at: dom.get("postedAt").and_then(Value::as_str).map(ToString::to_string),
        raw_json: dom.clone(),
        ..Default::default()
    };

    // Layer 1: GraphQL passive capture (`TweetActivityQuery`)
    for resp in network {
        if resp.status != 200 {
            continue;
        }
        let lower_url = resp.url.to_lowercase();
        if !lower_url.contains("tweetactivity") && !lower_url.contains("tweetdetail") && !lower_url.contains("analytics") {
            continue;
        }
        let Ok(parsed) = serde_json::from_str::<Value>(&resp.body) else { continue };
        let imp = find_int_by_keys(&parsed, &["Impressions", "impressions", "impression_count", "views"]);
        let likes = find_int_by_keys(&parsed, &["Likes", "likes", "favorite_count"]);
        let replies = find_int_by_keys(&parsed, &["Replies", "replies", "reply_count"]);
        let reposts = find_int_by_keys(&parsed, &["Retweets", "Reposts", "reposts", "retweet_count"]);
        let bookmarks = find_int_by_keys(&parsed, &["Bookmarks", "bookmarks", "bookmark_count"]);
        let visits = find_int_by_keys(&parsed, &["ProfileVisits", "profile_visits", "user_profile_clicks"]);
        let clicks = find_int_by_keys(&parsed, &["LinkClicks", "link_clicks", "url_link_clicks"]);
        let engagements = find_int_by_keys(&parsed, &["Engagements", "engagements"]);

        if imp.is_some() || likes.is_some() || engagements.is_some() {
            out.impressions = imp;
            out.likes = likes;
            out.replies = replies;
            out.reposts = reposts;
            out.bookmarks = bookmarks;
            out.profile_visits = visits;
            out.link_clicks = clicks;
            out.engagement_rate = compute_engagement_rate(imp, engagements, likes, replies, reposts, bookmarks);
            out.source = "graphql".into();
            out.raw_json = parsed;
            return out;
        }
    }

    // Layer 2: DOM fallback / public counters
    let str_field = |k: &str| dom.get(k).and_then(Value::as_str);
    out.impressions = str_field("impressions").and_then(parse_compact_count);
    out.likes = str_field("likes").and_then(parse_compact_count);
    out.replies = str_field("replies").and_then(parse_compact_count);
    out.reposts = str_field("reposts").and_then(parse_compact_count);
    out.bookmarks = str_field("bookmarks").and_then(parse_compact_count);
    let dom_eng = str_field("engagements").and_then(parse_compact_count);

    let snippet = str_field("bodySnippet").unwrap_or_default();
    if out.impressions.is_none() {
        out.impressions = extract_labeled_metric(snippet, "Impressions").and_then(|s| parse_compact_count(&s));
    }
    if out.profile_visits.is_none() {
        out.profile_visits = extract_labeled_metric(snippet, "Profile visits").and_then(|s| parse_compact_count(&s));
    }
    if out.link_clicks.is_none() {
        out.link_clicks = extract_labeled_metric(snippet, "Link clicks").and_then(|s| parse_compact_count(&s));
    }

    out.engagement_rate = compute_engagement_rate(out.impressions, dom_eng, out.likes, out.replies, out.reposts, out.bookmarks);
    out.source = if out.impressions.is_some() || out.likes.is_some() || out.replies.is_some() {
        "dom_fallback".into()
    } else {
        "empty".into()
    };
    out
}

pub fn compute_engagement_rate(
    impressions: Option<i64>,
    explicit_engagements: Option<i64>,
    likes: Option<i64>,
    replies: Option<i64>,
    reposts: Option<i64>,
    bookmarks: Option<i64>,
) -> Option<f64> {
    let imp = impressions?;
    if imp <= 0 {
        return Some(0.0);
    }
    let eng = explicit_engagements.unwrap_or_else(|| {
        likes.unwrap_or(0) + replies.unwrap_or(0) + reposts.unwrap_or(0) + bookmarks.unwrap_or(0)
    });
    Some(((eng as f64 / imp as f64) * 10000.0).round() / 100.0)
}

#[derive(Debug, Clone, Default)]
pub struct ProfileTimelineExtract {
    pub handle: Option<String>,
    pub followers_total: Option<i64>,
    pub posts: Vec<PostMetrics>,
}

/// Parses own posts and follower count from profile Posts/Replies tabs (ANL-06, ANL-16, ANL-17).
pub fn parse_profile_timeline(dom: &Value) -> ProfileTimelineExtract {
    let handle = dom.get("handle").and_then(Value::as_str).map(ToString::to_string);
    let followers_total = dom
        .get("followersText")
        .and_then(Value::as_str)
        .and_then(parse_compact_count);
    let mut posts = Vec::new();

    if let Some(arr) = dom.get("posts").and_then(Value::as_array) {
        for item in arr {
            let Some(id) = item.get("id").and_then(Value::as_str) else { continue };
            let impressions = item.get("views").and_then(Value::as_str).and_then(parse_compact_count);
            let likes = item.get("likes").and_then(Value::as_str).and_then(parse_compact_count);
            let replies = item.get("replies").and_then(Value::as_str).and_then(parse_compact_count);
            let reposts = item.get("reposts").and_then(Value::as_str).and_then(parse_compact_count);
            let bookmarks = item.get("bookmarks").and_then(Value::as_str).and_then(parse_compact_count);
            let engagement_rate = compute_engagement_rate(impressions, None, likes, replies, reposts, bookmarks);
            posts.push(PostMetrics {
                platform_post_id: id.to_string(),
                text: item.get("text").and_then(Value::as_str).map(ToString::to_string),
                posted_at: item.get("postedAt").and_then(Value::as_str).map(ToString::to_string),
                is_reply: item.get("isReply").and_then(Value::as_bool).unwrap_or(false),
                impressions,
                likes,
                replies,
                reposts,
                bookmarks,
                profile_visits: None,
                link_clicks: None,
                engagement_rate,
                source: "public_counters".into(),
                raw_json: item.clone(),
            });
        }
    }

    ProfileTimelineExtract {
        handle,
        followers_total,
        posts,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_compact_counts_and_percentages() {
        assert_eq!(parse_compact_count("1.2K Views"), Some(1200));
        assert_eq!(parse_compact_count("3.4M"), Some(3_400_000));
        assert_eq!(parse_compact_count("1,234 Likes"), Some(1234));
        assert_eq!(parse_percent("4.25%"), Some(4.25));
    }

    #[test]
    fn prefers_graphql_then_falls_back_to_dom() {
        let gql = vec![CapturedNetworkResponse {
            url: "https://x.com/i/api/graphql/xyz/AccountOverviewQuery".into(),
            status: 200,
            body: json!({
                "data": { "overview": { "impressions": 45200, "engagement_rate": 0.042, "profile_visits": 610, "followers_total": 3120 } }
            })
            .to_string(),
        }];
        let m = parse_account_overview(&gql, &json!({}));
        assert_eq!(m.source, "graphql");
        assert_eq!(m.impressions, Some(45200));
        assert_eq!(m.engagement_rate, Some(4.2));
        assert_eq!(m.followers_total, Some(3120));

        // When GraphQL fails/absent, DOM fallback extracts metrics
        let dom = json!({
            "metricCards": { "impressions": "18.5K", "engagement_rate": "3.8%", "profile_visits": "210" },
            "bodySnippet": "Followers\n2,980"
        });
        let fallback = parse_account_overview(&[], &dom);
        assert_eq!(fallback.source, "dom_fallback");
        assert_eq!(fallback.impressions, Some(18500));
        assert_eq!(fallback.engagement_rate, Some(3.8));
        assert_eq!(fallback.followers_total, Some(2980));
    }

    #[test]
    fn detects_non_premium_upsell() {
        let dom = json!({
            "isPremiumUpsell": true,
            "metricCards": {},
            "bodySnippet": "Subscribe to X Premium to unlock analytics"
        });
        let m = parse_account_overview(&[], &dom);
        assert!(m.is_premium_upsell);
        assert_eq!(m.source, "non_premium_upsell");
    }

    #[test]
    fn extraction_scripts_are_strictly_read_only() {
        for script in [ACCOUNT_ANALYTICS_DOM_JS, POST_ANALYTICS_DOM_JS, PROFILE_TIMELINE_DOM_JS] {
            for banned in ["click(", ".submit", "dispatchEvent", "fetch(", "XMLHttpRequest", "innerHTML ="] {
                assert!(!script.contains(banned), "analytics script must not contain {banned}");
            }
        }
    }
}
