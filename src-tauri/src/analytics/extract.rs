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
    pub period_days: Option<u32>,
    pub period_label: Option<String>,
    pub impressions: Option<i64>,
    pub engagements: Option<i64>,
    pub engagement_rate: Option<f64>,
    pub profile_visits: Option<i64>,
    pub followers_total: Option<i64>,
    pub follower_delta: Option<i64>,
    pub new_follows: Option<i64>,
    pub unfollows: Option<i64>,
    pub likes: Option<i64>,
    pub replies: Option<i64>,
    pub reposts: Option<i64>,
    pub bookmarks: Option<i64>,
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

/// Read-only range-selector locator for `x.com/i/account_analytics`.
/// Finds the visible 7-day (`7D` / `7 days` / `1W`) or 30-day (`4W` / `30D` / `28D` / `1M`) range control
/// and returns its center coordinates without mutating the page.
pub fn account_analytics_range_locator_js(period_days: u32) -> String {
    let targets = if period_days <= 7 {
        r#"["7d","7 days","last 7 days","1w","1 week"]"#
    } else {
        r#"["4w","30d","28d","1m","30 days","28 days","last 30 days","last 28 days","4 weeks"]"#
    };
    format!(
        r#"(() => {{
  const targets = {targets};
  const rangeTokenRe = /^(7d|2w|4w|28d|30d|1w|1m|3m|1y|7 days|28 days|30 days|last 7 days|last 28 days|last 30 days|1 week|4 weeks)$/i;
  const norm = (s) => (s || '').replace(/\s+/g, ' ').trim().toLowerCase();
  const isVisible = (el) => {{
    const r = el.getBoundingClientRect();
    return r && r.width > 4 && r.height > 4 && r.bottom >= 0 && r.right >= 0;
  }};
  const candidates = [];
  for (const el of document.querySelectorAll('button, [role="tab"], [role="radio"], [role="button"], a[role="tab"]')) {{
    if (!isVisible(el)) continue;
    const text = norm(el.innerText || el.getAttribute('aria-label') || '');
    if (!rangeTokenRe.test(text)) continue;
    const r = el.getBoundingClientRect();
    const cs = window.getComputedStyle(el);
    const bg = cs ? cs.backgroundColor : '';
    const fw = cs ? parseInt(cs.fontWeight || '400', 10) : 400;
    const explicit = el.getAttribute('aria-selected') === 'true'
      || el.getAttribute('aria-pressed') === 'true'
      || el.getAttribute('aria-current') === 'true'
      || el.getAttribute('data-selected') === 'true'
      || el.getAttribute('data-state') === 'active';
    candidates.push({{ el, text, label: (el.innerText || el.getAttribute('aria-label') || '').trim(), rect: r, bg, fw, explicit }});
  }}
  const anyExplicit = candidates.some((c) => c.explicit);
  const bgCounts = {{}};
  for (const c of candidates) {{
    if (c.bg && c.bg !== 'rgba(0, 0, 0, 0)' && c.bg !== 'transparent') {{
      bgCounts[c.bg] = (bgCounts[c.bg] || 0) + 1;
    }}
  }}
  const isSelected = (c) => {{
    if (c.explicit) return true;
    if (anyExplicit) return false;
    if (c.bg && bgCounts[c.bg] === 1 && candidates.length >= 2) return true;
    return false;
  }};
  for (const want of targets) {{
    const hit = candidates.find((c) => c.text === want);
    if (hit) {{
      return {{
        found: true,
        alreadySelected: isSelected(hit),
        label: hit.label,
        x: Math.round((hit.rect.left + hit.rect.width / 2) * 10) / 10,
        y: Math.round((hit.rect.top + hit.rect.height / 2) * 10) / 10,
        available: candidates.map((c) => c.label)
      }};
    }}
  }}
  return {{ found: false, alreadySelected: false, label: null, x: null, y: null, available: candidates.map((c) => c.label) }};
}})()"#
    )
}

/// Read-only DOM probe for `x.com/i/account_analytics` (SPK-04, ANL-02, ANL-06).
pub const ACCOUNT_ANALYTICS_DOM_JS: &str = r#"(() => {
  const url = location.href || '';
  const bodyText = (document.body?.innerText || '').slice(0, 10000);
  const lowerBody = bodyText.toLowerCase();
  const loggedOut = /\/i\/(flow\/login|jf\/onboarding\/web)|\/login\b/i.test(url);
  let state = 'ok';
  if (loggedOut) state = 'logged_out';
  else if (/\/account\/access|arkose|challenge/i.test(url) || /verify your identity|confirm you('|’)re human|unusual login/i.test(lowerBody)) state = 'challenge';
  else if (/account is locked/i.test(lowerBody)) state = 'locked';
  else if (/account is suspended/i.test(lowerBody)) state = 'suspended';

  const rangeTokenRe = /^(7d|2w|4w|28d|30d|1w|1m|3m|1y|7 days|28 days|30 days|last 7 days|last 28 days|last 30 days|1 week|4 weeks)$/i;
  const rangeToDays = (t) => {
    const s = (t || '').replace(/\s+/g, ' ').trim().toLowerCase();
    if (s === '7d' || s === '7 days' || s === 'last 7 days' || s === '1w' || s === '1 week') return 7;
    if (s === '2w') return 14;
    if (s === '4w' || s === '28d' || s === '28 days' || s === 'last 28 days' || s === '4 weeks') return 28;
    if (s === '30d' || s === '30 days' || s === 'last 30 days' || s === '1m') return 30;
    if (s === '3m') return 90;
    if (s === '1y') return 365;
    return null;
  };

  const rangeBtns = [];
  for (const el of document.querySelectorAll('button, [role="tab"], [role="radio"], [role="button"], a[role="tab"]')) {
    const r = el.getBoundingClientRect();
    if (!r || r.width <= 4 || r.height <= 4) continue;
    const rawLabel = (el.innerText || el.getAttribute('aria-label') || '').replace(/\s+/g, ' ').trim();
    if (!rangeTokenRe.test(rawLabel)) continue;
    const cs = window.getComputedStyle(el);
    const bg = cs ? cs.backgroundColor : '';
    const explicit = el.getAttribute('aria-selected') === 'true'
      || el.getAttribute('aria-pressed') === 'true'
      || el.getAttribute('aria-current') === 'true'
      || el.getAttribute('data-selected') === 'true'
      || el.getAttribute('data-state') === 'active';
    rangeBtns.push({ label: rawLabel, days: rangeToDays(rawLabel), bg, explicit });
  }
  const anyExplicit = rangeBtns.some((b) => b.explicit);
  const bgCounts = {};
  for (const b of rangeBtns) {
    if (b.bg && b.bg !== 'rgba(0, 0, 0, 0)' && b.bg !== 'transparent') {
      bgCounts[b.bg] = (bgCounts[b.bg] || 0) + 1;
    }
  }
  const activeRange = rangeBtns.find((b) => b.explicit)
    || (!anyExplicit && rangeBtns.length >= 2 ? rangeBtns.find((b) => b.bg && bgCounts[b.bg] === 1) : null);

  const metricCards = {};
  for (const el of document.querySelectorAll('[data-testid]')) {
    const id = el.getAttribute('data-testid') || '';
    if (/(impression|engagement|profile|follow|visit|metric|stat|like|repl|repost|bookmark)/i.test(id)) {
      metricCards[id] = (el.innerText || '').trim().slice(0, 160);
    }
  }

  const knownLabels = [
    'impressions', 'engagement rate', 'engagements', 'profile visits',
    'new follows', 'unfollows', 'likes', 'replies', 'reposts', 'bookmarks', 'followers'
  ];
  const labeledMetrics = {};
  for (const el of document.querySelectorAll('div, section, article, a, button, li')) {
    if (el.children.length > 8) continue;
    const text = (el.innerText || '').trim();
    if (!text || text.length > 140 || !/\d/.test(text)) continue;
    const lines = text.split(/\n+/).map((l) => l.trim()).filter(Boolean);
    if (lines.length === 0 || lines.length > 5) continue;
    const first = lines[0].toLowerCase();
    if (first === 'verified followers') continue;
    if (knownLabels.includes(first) && !labeledMetrics[first]) {
      labeledMetrics[first] = lines.slice(1).join(' | ');
    }
  }

  const hasMetrics = Object.keys(metricCards).length > 0 || Object.keys(labeledMetrics).length > 0;
  const isPremiumUpsell = !hasMetrics && /(subscribe|x premium|upgrade to premium|unlock analytics|get premium)/i.test(bodyText);
  const switcher = document.querySelector('[data-testid="SideNav_AccountSwitcher_Button"]')?.innerText || '';
  const handleMatch = switcher.match(/@([A-Za-z0-9_]{1,15})/);
  return {
    url,
    state,
    loggedOut,
    isPremiumUpsell,
    handle: handleMatch ? handleMatch[1] : null,
    selectedRangeLabel: activeRange ? activeRange.label : null,
    selectedRangeDays: activeRange ? activeRange.days : null,
    availableRanges: rangeBtns.map((b) => b.label),
    metricCards,
    labeledMetrics,
    bodySnippet: bodyText.slice(0, 3500)
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

/// Parses X shorthand counters (`"1.2K"`, `"3.4M"`, `"1,234 Views"`, `"42 Likes"`), skipping percentage deltas (`"+18%"`).
pub fn parse_compact_count(raw: &str) -> Option<i64> {
    let cleaned = raw.replace(',', "");
    for token in cleaned.split_whitespace() {
        if token.contains('%') {
            continue;
        }
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

/// Parses a signed shorthand counter (`"+127"`, `"-14"`, `"+1.2K"`), skipping percentage tokens.
pub fn parse_signed_compact_count(raw: &str) -> Option<i64> {
    let cleaned = raw.replace(',', "");
    for token in cleaned.split_whitespace() {
        if token.contains('%') {
            continue;
        }
        let t = token.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '.' && c != '-' && c != '+');
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

/// Parses a percentage string like `"4.25%"` or `"4.25"`, preferring unsigned rate values over delta badges (`"+0.5%"`).
pub fn parse_percent(raw: &str) -> Option<f64> {
    let mut fallback: Option<f64> = None;
    for token in raw.split_whitespace() {
        let stripped = token.trim_matches(|c: char| !c.is_ascii_digit() && c != '.' && c != '-' && c != '+' && c != '%');
        if stripped.is_empty() {
            continue;
        }
        let is_signed_delta = stripped.starts_with('+') || stripped.starts_with('-');
        let num = stripped.trim_end_matches('%');
        if let Ok(v) = num.parse::<f64>() {
            let rounded = (v * 100.0).round() / 100.0;
            if !is_signed_delta {
                return Some(rounded);
            }
            if fallback.is_none() {
                fallback = Some(rounded);
            }
        }
    }
    fallback
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
    let want = label.to_lowercase();
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    // Prefer an exact line match (e.g. "Engagements" does not match "Engagement rate").
    for (idx, line) in lines.iter().enumerate() {
        if line.to_lowercase() == want {
            let collected: Vec<&str> = lines
                .iter()
                .skip(idx + 1)
                .take(3)
                .copied()
                .filter(|l| l.chars().any(|c| c.is_ascii_digit()))
                .collect();
            if !collected.is_empty() {
                return Some(collected.join(" "));
            }
        }
    }
    let lower = text.to_lowercase();
    let idx = lower.find(&want)?;
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
        period_days: dom.get("selectedRangeDays").and_then(Value::as_u64).map(|n| n as u32),
        period_label: dom.get("selectedRangeLabel").and_then(Value::as_str).map(ToString::to_string),
        is_logged_out: dom.get("loggedOut").and_then(Value::as_bool).unwrap_or(false)
            || dom.get("state").and_then(Value::as_str) == Some("logged_out"),
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
        let eng = find_int_by_keys(&parsed, &["engagements", "Engagements", "total_engagements"]);
        let eng_rate = find_number_by_keys(&parsed, &["engagement_rate", "engagementRate"]);
        let visits = find_int_by_keys(&parsed, &["profile_visits", "profileVisits", "ProfileVisits"]);
        let followers = find_int_by_keys(&parsed, &["followers_count", "followersCount", "followers", "followers_total"]);
        let new_follows = find_int_by_keys(&parsed, &["new_follows", "newFollows", "follows"]);
        let unfollows = find_int_by_keys(&parsed, &["unfollows", "Unfollows"]);
        let delta = find_int_by_keys(&parsed, &["follower_delta", "followerDelta", "net_follows", "netFollows"])
            .or_else(|| match (new_follows, unfollows) {
                (Some(n), Some(u)) => Some(n - u),
                (Some(n), None) => Some(n),
                _ => None,
            });
        let likes = find_int_by_keys(&parsed, &["likes", "Likes"]);
        let replies = find_int_by_keys(&parsed, &["replies", "Replies"]);
        let reposts = find_int_by_keys(&parsed, &["reposts", "Reposts", "retweets", "Retweets"]);
        let bookmarks = find_int_by_keys(&parsed, &["bookmarks", "Bookmarks"]);

        if imp.is_some() || eng.is_some() || eng_rate.is_some() || visits.is_some() || followers.is_some() {
            out.impressions = imp;
            out.engagements = eng;
            out.engagement_rate = eng_rate
                .map(|v| if v > 0.0 && v < 1.0 { (v * 10000.0).round() / 100.0 } else { v })
                .or_else(|| compute_engagement_rate(imp, eng, likes, replies, reposts, bookmarks));
            out.profile_visits = visits;
            out.followers_total = followers;
            out.new_follows = new_follows;
            out.unfollows = unfollows;
            out.follower_delta = delta;
            out.likes = likes;
            out.replies = replies;
            out.reposts = reposts;
            out.bookmarks = bookmarks;
            out.is_premium_upsell = false;
            out.source = "graphql".into();
            out.raw_json = parsed;
            return out;
        }
    }

    // Layer 2: DOM fallback (explicit semantic labeled cards first, then data-testid cards, then body text)
    if let Some(labeled) = dom.get("labeledMetrics").and_then(Value::as_object) {
        let get = |k: &str| labeled.get(k).and_then(Value::as_str);
        if let Some(t) = get("impressions") {
            out.impressions = parse_compact_count(t);
        }
        if let Some(t) = get("engagement rate") {
            out.engagement_rate = parse_percent(t);
        }
        if let Some(t) = get("engagements") {
            out.engagements = parse_compact_count(t);
        }
        if let Some(t) = get("profile visits") {
            out.profile_visits = parse_compact_count(t);
        }
        if let Some(t) = get("new follows") {
            out.new_follows = parse_compact_count(t);
        }
        if let Some(t) = get("unfollows") {
            out.unfollows = parse_compact_count(t);
        }
        if let Some(t) = get("likes") {
            out.likes = parse_compact_count(t);
        }
        if let Some(t) = get("replies") {
            out.replies = parse_compact_count(t);
        }
        if let Some(t) = get("reposts") {
            out.reposts = parse_compact_count(t);
        }
        if let Some(t) = get("bookmarks") {
            out.bookmarks = parse_compact_count(t);
        }
        if let Some(t) = get("followers") {
            out.followers_total = parse_compact_count(t);
        }
    }

    let cards = dom.get("metricCards").and_then(Value::as_object);
    let snippet = dom.get("bodySnippet").and_then(Value::as_str).unwrap_or_default();

    if let Some(map) = cards {
        for (k, v) in map {
            let kl = k.to_lowercase();
            let text = v.as_str().unwrap_or_default();
            if kl.contains("impression") && out.impressions.is_none() {
                out.impressions = parse_compact_count(text);
            } else if kl.contains("engagement_rate") || (kl.contains("engagement") && text.contains('%')) {
                if out.engagement_rate.is_none() {
                    out.engagement_rate = parse_percent(text);
                }
            } else if kl.contains("engagement") && out.engagements.is_none() {
                out.engagements = parse_compact_count(text);
            } else if (kl.contains("visit") || kl.contains("profile")) && out.profile_visits.is_none() {
                out.profile_visits = parse_compact_count(text);
            } else if kl.contains("new_follow") && out.new_follows.is_none() {
                out.new_follows = parse_compact_count(text);
            } else if kl.contains("unfollow") && out.unfollows.is_none() {
                out.unfollows = parse_compact_count(text);
            } else if kl.contains("follow") && !kl.contains("verified") && out.followers_total.is_none() {
                out.followers_total = parse_compact_count(text);
            } else if kl.contains("like") && out.likes.is_none() {
                out.likes = parse_compact_count(text);
            } else if kl.contains("repl") && out.replies.is_none() {
                out.replies = parse_compact_count(text);
            } else if kl.contains("repost") && out.reposts.is_none() {
                out.reposts = parse_compact_count(text);
            } else if kl.contains("bookmark") && out.bookmarks.is_none() {
                out.bookmarks = parse_compact_count(text);
            }
        }
    }

    if out.impressions.is_none() {
        out.impressions = extract_labeled_metric(snippet, "Impressions").and_then(|s| parse_compact_count(&s));
    }
    if out.engagement_rate.is_none() {
        out.engagement_rate = extract_labeled_metric(snippet, "Engagement rate").and_then(|s| parse_percent(&s));
    }
    if out.engagements.is_none() {
        out.engagements = extract_labeled_metric(snippet, "Engagements").and_then(|s| parse_compact_count(&s));
    }
    if out.profile_visits.is_none() {
        out.profile_visits = extract_labeled_metric(snippet, "Profile visits").and_then(|s| parse_compact_count(&s));
    }
    if out.new_follows.is_none() {
        out.new_follows = extract_labeled_metric(snippet, "New follows").and_then(|s| parse_compact_count(&s));
    }
    if out.unfollows.is_none() {
        out.unfollows = extract_labeled_metric(snippet, "Unfollows").and_then(|s| parse_compact_count(&s));
    }
    if out.likes.is_none() {
        out.likes = extract_labeled_metric(snippet, "Likes").and_then(|s| parse_compact_count(&s));
    }
    if out.replies.is_none() {
        out.replies = extract_labeled_metric(snippet, "Replies").and_then(|s| parse_compact_count(&s));
    }
    if out.reposts.is_none() {
        out.reposts = extract_labeled_metric(snippet, "Reposts").and_then(|s| parse_compact_count(&s));
    }
    if out.bookmarks.is_none() {
        out.bookmarks = extract_labeled_metric(snippet, "Bookmarks").and_then(|s| parse_compact_count(&s));
    }
    if out.followers_total.is_none() {
        out.followers_total = extract_labeled_metric(snippet, "Followers").and_then(|s| parse_compact_count(&s));
    }
    if out.follower_delta.is_none() {
        out.follower_delta = extract_labeled_metric(snippet, "Net follows")
            .and_then(|s| parse_signed_compact_count(&s))
            .or_else(|| match (out.new_follows, out.unfollows) {
                (Some(n), Some(u)) => Some(n - u),
                (Some(n), None) => Some(n),
                _ => None,
            });
    }
    if out.engagement_rate.is_none() {
        out.engagement_rate = compute_engagement_rate(
            out.impressions,
            out.engagements,
            out.likes,
            out.replies,
            out.reposts,
            out.bookmarks,
        );
    }

    if out.impressions.is_some()
        || out.engagements.is_some()
        || out.engagement_rate.is_some()
        || out.profile_visits.is_some()
        || out.followers_total.is_some()
        || out.new_follows.is_some()
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
        assert_eq!(parse_compact_count("+18% 142.3K"), Some(142_300));
        assert_eq!(parse_signed_compact_count("+127"), Some(127));
        assert_eq!(parse_signed_compact_count("-14"), Some(-14));
        assert_eq!(parse_percent("4.25%"), Some(4.25));
        assert_eq!(parse_percent("+0.5% 4.25%"), Some(4.25));
    }

    #[test]
    fn parses_creator_analytics_labeled_cards_and_computes_follower_delta() {
        let dom = json!({
            "state": "ok",
            "handle": "pavel",
            "selectedRangeLabel": "7D",
            "selectedRangeDays": 7,
            "labeledMetrics": {
                "impressions": "+24% | 54.2K",
                "engagement rate": "+0.4% | 4.8%",
                "engagements": "2.6K",
                "profile visits": "720",
                "new follows": "48",
                "unfollows": "11",
                "likes": "910",
                "replies": "180",
                "reposts": "65",
                "bookmarks": "140"
            },
            "metricCards": {},
            "bodySnippet": ""
        });
        let m = parse_account_overview(&[], &dom);
        assert_eq!(m.source, "dom_fallback");
        assert_eq!(m.period_days, Some(7));
        assert_eq!(m.period_label.as_deref(), Some("7D"));
        assert_eq!(m.impressions, Some(54_200));
        assert_eq!(m.engagement_rate, Some(4.8));
        assert_eq!(m.engagements, Some(2_600));
        assert_eq!(m.profile_visits, Some(720));
        assert_eq!(m.new_follows, Some(48));
        assert_eq!(m.unfollows, Some(11));
        assert_eq!(m.follower_delta, Some(37));
        assert_eq!(m.likes, Some(910));
        assert_eq!(m.replies, Some(180));
        assert_eq!(m.reposts, Some(65));
        assert_eq!(m.bookmarks, Some(140));
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
        let loc7 = account_analytics_range_locator_js(7);
        let loc30 = account_analytics_range_locator_js(30);
        for script in [
            ACCOUNT_ANALYTICS_DOM_JS,
            POST_ANALYTICS_DOM_JS,
            PROFILE_TIMELINE_DOM_JS,
            loc7.as_str(),
            loc30.as_str(),
        ] {
            for banned in ["click(", ".submit", "dispatchEvent", "fetch(", "XMLHttpRequest", "innerHTML ="] {
                assert!(!script.contains(banned), "analytics script must not contain {banned}");
            }
        }
    }
}
