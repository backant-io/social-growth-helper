//! Reading posts with their engagement numbers from a creator's profile page (OGC-03).
//! The page script only reads. Metric text is parsed in Rust so the parsing is testable without a browser.

use super::detect::PostMetrics;
use crate::scout::model::MediaItem;
use crate::browser::service::BrowserService;
use crate::browser::BrowserError;
use serde::Deserialize;
use std::time::Duration;

/// Selectors come from the Phase 0 catalogue (SPK-06); verify them against live X before relying on this.
pub const READ_POSTS_WITH_METRICS_JS: &str = r#"(() => {
  const out = [];
  for (const a of document.querySelectorAll('article[data-testid="tweet"]')) {
    const link = a.querySelector('a[href*="/status/"] time')?.closest('a');
    const m = (link?.getAttribute('href') || '').match(/^\/([^/]+)\/status\/(\d+)/);
    if (!m) continue;
    out.push({
      id: m[2],
      handle: m[1],
      text: a.querySelector('[data-testid="tweetText"]')?.innerText || '',
      promoted: !!a.querySelector('[data-testid="placementTracking"]'),
      repost: !!a.querySelector('[data-testid="socialContext"]'),
      metrics: a.querySelector('[role="group"][aria-label]')?.getAttribute('aria-label') || '',
      time: a.querySelector('time')?.getAttribute('datetime') || null,
      name: (a.querySelector('[data-testid="User-Name"]')?.innerText || '').split('\n')[0],
      avatar: (() => { const u = a.querySelector('[data-testid="Tweet-User-Avatar"] img')?.src || ''; return /^https:\/\/pbs\.twimg\.com\/profile_images\//.test(u) ? u.replace(/_(normal|mini|bigger)\./, '_bigger.') : null; })(),
      media: (() => {
        const quote = a.querySelector('[data-testid="quoteTweet"]');
        const items = [];
        for (const img of a.querySelectorAll('[data-testid="tweetPhoto"] img')) {
          if ((quote && quote.contains(img)) || !/^https:\/\/pbs\.twimg\.com\//.test(img.src || '')) continue;
          if (!items.some(x => x.url === img.src) && items.length < 4) items.push({ kind: 'image', url: img.src, poster_url: null, alt: img.alt || null, duration: null });
        }
        for (const vp of a.querySelectorAll('[data-testid="videoPlayer"]')) {
          if (quote && quote.contains(vp)) continue;
          const v = vp.querySelector('video');
          if (!v) continue;
          const src = v.currentSrc || v.src || '';
          const poster = /^https:\/\/pbs\.twimg\.com\//.test(v.poster || '') ? v.poster : null;
          const dur = (vp.innerText || '').match(/\b\d+:\d{2}\b/);
          if (poster || /^https:\/\//.test(src)) items.push({ kind: poster && poster.includes('/tweet_video_thumb/') ? 'gif' : 'video', url: /^https:\/\//.test(src) ? src : null, poster_url: poster, alt: null, duration: dur ? dur[0] : null });
        }
        return items;
      })()
    });
  }
  return out;
})()"#;

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct RawPost {
    pub id: String,
    pub handle: String,
    pub text: String,
    #[serde(default)]
    pub promoted: bool,
    #[serde(default)]
    pub repost: bool,
    #[serde(default)]
    pub metrics: String,
    pub time: Option<String>,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub avatar: Option<String>,
    #[serde(default)]
    pub media: Vec<MediaItem>,
}

fn parse_count(raw: &str) -> Option<u64> {
    let t = raw.trim().replace(',', "");
    let (num, mult) = match t.chars().last()? {
        'K' | 'k' => (&t[..t.len() - 1], 1_000.0),
        'M' | 'm' => (&t[..t.len() - 1], 1_000_000.0),
        'B' | 'b' => (&t[..t.len() - 1], 1_000_000_000.0),
        _ => (t.as_str(), 1.0),
    };
    num.trim().parse::<f64>().ok().filter(|v| *v >= 0.0).map(|v| (v * mult).round() as u64)
}

/// Parses X's action-bar label, e.g. `"12 replies, 5 reposts, 1.2K likes, 3 bookmarks, 20K views"`.
pub fn parse_metrics_label(label: &str) -> PostMetrics {
    let mut m = PostMetrics::default();
    for part in label.split(", ") {
        let part = part.trim().to_lowercase();
        let Some((count, kind)) = part.split_once(' ') else { continue };
        let Some(n) = parse_count(count) else { continue };
        match kind.trim_end_matches('.').trim() {
            k if k.starts_with("repl") => m.replies = n,
            k if k.starts_with("repost") || k.starts_with("retweet") => m.reposts = n,
            k if k.starts_with("like") => m.likes = n,
            k if k.starts_with("bookmark") => m.bookmarks = n,
            k if k.starts_with("view") => m.views = Some(n),
            _ => {}
        }
    }
    m
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScannedPost {
    pub id: String,
    pub handle: String,
    pub text: String,
    pub posted_at: Option<String>,
    pub metrics: PostMetrics,
    pub author_name: String,
    pub avatar_url: Option<String>,
    pub media: Vec<MediaItem>,
}

/// Keeps the creator's own original posts: no ads, no reposts, no posts by other authors, no empty text.
pub fn usable_posts(raw: Vec<RawPost>, creator: &str) -> Vec<ScannedPost> {
    raw.into_iter()
        .filter(|p| !p.promoted && !p.repost && p.handle.eq_ignore_ascii_case(creator) && !p.text.trim().is_empty())
        .map(|p| ScannedPost {
            metrics: parse_metrics_label(&p.metrics),
            id: p.id,
            handle: p.handle,
            text: p.text,
            posted_at: p.time,
            author_name: p.name,
            avatar_url: p.avatar,
            media: p.media,
        })
        .collect()
}

const SCROLL_STEPS: usize = 4;

/// Read-only extraction of the profile header's follower count label (e.g. `"1,234 Followers"` or `"42.1K"`).
pub const READ_PROFILE_FOLLOWERS_JS: &str = r#"(() => {
  const el = document.querySelector('a[href$="/followers"], a[href$="/verified_followers"]');
  return el ? (el.innerText || el.getAttribute('aria-label') || '') : null;
})()"#;

pub fn parse_followers_label(label: &str) -> Option<u64> {
    let token = label
        .split_whitespace()
        .find(|w| w.chars().next().is_some_and(|c| c.is_ascii_digit()))?;
    parse_count(token)
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProfileScan {
    pub followers: Option<u64>,
    pub posts: Vec<ScannedPost>,
}

/// Opens a creator's profile in the X window and reads their follower count plus recent posts. Read-only apart from scrolling.
pub async fn scan_profile_with_followers(browser: &BrowserService, handle: &str) -> Result<ProfileScan, BrowserError> {
    let d = browser.lock().await;
    if !d.is_running() {
        return Err(BrowserError::Disconnected);
    }
    d.navigate(&format!("https://x.com/{handle}")).await?;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let followers = d
        .eval_json(READ_PROFILE_FOLLOWERS_JS)
        .await
        .ok()
        .and_then(|v| v.as_str().and_then(parse_followers_label));
    let mut all: Vec<RawPost> = Vec::new();
    for step in 0..=SCROLL_STEPS {
        let v = d.eval_json(READ_POSTS_WITH_METRICS_JS).await?;
        let batch: Vec<RawPost> =
            serde_json::from_value(v).map_err(|e| BrowserError::Evaluation(format!("unexpected post payload: {e}")))?;
        for p in batch {
            if !all.iter().any(|x| x.id == p.id) {
                all.push(p);
            }
        }
        if step < SCROLL_STEPS {
            d.wheel_scroll(400.0, 500.0, 700.0).await?;
            tokio::time::sleep(Duration::from_millis(1200)).await;
        }
    }
    Ok(ProfileScan { followers, posts: usable_posts(all, handle) })
}

/// Opens a creator's profile in the X window and reads the posts it renders. Read-only apart from scrolling.
pub async fn scan_profile(browser: &BrowserService, handle: &str) -> Result<Vec<ScannedPost>, BrowserError> {
    Ok(scan_profile_with_followers(browser, handle).await?.posts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_label_with_abbreviations() {
        let m = parse_metrics_label("12 replies, 5 reposts, 1.2K likes, 3 bookmarks, 20K views");
        assert_eq!((m.replies, m.reposts, m.likes, m.bookmarks, m.views), (12, 5, 1_200, 3, Some(20_000)));
    }

    #[test]
    fn parses_singular_labels_commas_and_millions() {
        let m = parse_metrics_label("1 reply, 1 repost, 1,234 likes, 2.5M views");
        assert_eq!((m.replies, m.reposts, m.likes, m.views), (1, 1, 1_234, Some(2_500_000)));
    }

    #[test]
    fn missing_or_garbled_parts_stay_zero() {
        let m = parse_metrics_label("lots of likes, ?? views");
        assert_eq!(m, PostMetrics::default());
        assert_eq!(parse_metrics_label(""), PostMetrics::default());
    }

    #[test]
    fn filters_ads_reposts_other_authors_and_empty_text() {
        let raw = |id: &str, handle: &str, text: &str, promoted: bool, repost: bool| RawPost {
            id: id.into(),
            handle: handle.into(),
            text: text.into(),
            promoted,
            repost,
            metrics: "10 likes".into(),
            time: None,
            name: String::new(),
            avatar: None,
            media: vec![],
        };
        let posts = usable_posts(
            vec![
                raw("1", "Naval", "keep", false, false),
                raw("2", "Naval", "ad", true, false),
                raw("3", "Naval", "rt", false, true),
                raw("4", "other", "someone else", false, false),
                raw("5", "naval", "  ", false, false),
            ],
            "naval",
        );
        assert_eq!(posts.len(), 1);
        assert_eq!(posts[0].id, "1");
        assert_eq!(posts[0].metrics.likes, 10);
    }

    #[test]
    fn script_is_read_only() {
        for banned in ["click(", ".submit", "dispatchEvent", "fetch(", "XMLHttpRequest", "innerHTML ="] {
            assert!(!READ_POSTS_WITH_METRICS_JS.contains(banned), "must not contain {banned}");
        }
    }
}
