//! Scraped-post types plus the small parsers that turn page text into numbers and timestamps.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Metrics {
    pub replies: u64,
    pub reposts: u64,
    pub likes: u64,
    pub bookmarks: u64,
    pub views: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MediaItem {
    /// `"image"`, `"video"`, or `"gif"`.
    pub kind: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub poster_url: Option<String>,
    #[serde(default)]
    pub alt: Option<String>,
    #[serde(default)]
    pub duration: Option<String>,
}

/// A tweet card as read from the feed. Everything here is untrusted page content (SEC-07).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ScrapedPost {
    pub id: Option<String>,
    pub handle: Option<String>,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub author_verified: bool,
    #[serde(default)]
    pub is_repost: bool,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub lang: Option<String>,
    #[serde(default)]
    pub promoted: bool,
    #[serde(default)]
    pub reply_restricted: bool,
    #[serde(default)]
    pub has_media: bool,
    #[serde(default)]
    pub time: Option<String>,
    #[serde(default)]
    pub metrics_label: Option<String>,
    /// Text of the quoted post card, when this post quotes another (SCT-20).
    #[serde(default)]
    pub quoted: Option<String>,
    /// Text of the post this one replies to, when it is rendered right above in the thread (SCT-20).
    #[serde(default)]
    pub parent: Option<String>,
    #[serde(default)]
    pub replying_to: Option<String>,
    /// Up to four image URLs, for image-aware scoring and UI display (SCT-19).
    #[serde(default)]
    pub image_urls: Vec<String>,
    /// Structured images, videos and GIFs extracted from the post.
    #[serde(default)]
    pub media: Vec<MediaItem>,
    /// The card shows a "Show more" link: the text read here is cut off (BRW-19).
    #[serde(default)]
    pub truncated: bool,
    /// The media sits behind a sensitive-content wall (BRW-19).
    #[serde(default)]
    pub sensitive: bool,
}

impl ScrapedPost {
    /// Returns structured media items, falling back to `image_urls` if `media` is empty.
    pub fn resolved_media(&self) -> Vec<MediaItem> {
        if !self.media.is_empty() {
            return self.media.clone();
        }
        self.image_urls
            .iter()
            .map(|u| MediaItem { kind: "image".into(), url: Some(u.clone()), poster_url: None, alt: None, duration: None })
            .collect()
    }

    /// Quote and thread context as one block of untrusted text, or `None` when there is none.
    pub fn context_text(&self) -> Option<String> {
        let mut parts = vec![];
        if let Some(p) = self.parent.as_deref().filter(|t| !t.trim().is_empty()) {
            parts.push(format!("Replying to{}: {}", self.replying_to.as_deref().map(|h| format!(" @{h}")).unwrap_or_default(), clip(p, 400)));
        }
        if let Some(q) = self.quoted.as_deref().filter(|t| !t.trim().is_empty()) {
            parts.push(format!("Quoted post: {}", clip(q, 400)));
        }
        (!parts.is_empty()).then(|| parts.join("\n"))
    }
}

fn clip(s: &str, max: usize) -> String {
    let t: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    t.chars().take(max).collect()
}

#[derive(Debug, Clone, Deserialize)]
pub struct FeedRead {
    pub state: String,
    pub me: Option<String>,
    #[serde(default)]
    pub posts: Vec<ScrapedPost>,
}

/// Splits "1,2 Mio." style suffixes off a number token. Returns the multiplier and how many words it used.
fn multiplier(word: &str) -> Option<f64> {
    match word.trim_end_matches('.').to_lowercase().as_str() {
        "k" | "tsd" | "mil" => Some(1_000.0),
        "m" | "mio" | "mill" | "mln" => Some(1_000_000.0),
        "mrd" | "b" => Some(1_000_000_000.0),
        _ => None,
    }
}

/// Parses a count in English, German, Spanish or French notation: `1,234`, `2.345`, `1.2K`, `1,2 Mio.`.
/// With a suffix the separator is a decimal point; without one it groups thousands.
fn parse_count(raw: &str, suffix: Option<&str>) -> Option<u64> {
    let t: String = raw.chars().filter(|c| !c.is_whitespace() && *c != '\u{a0}' && *c != '\u{202f}').collect();
    let (num, mult) = match suffix.and_then(multiplier) {
        Some(m) => (t.as_str(), m),
        None => match t.chars().last()? {
            'K' | 'k' => (&t[..t.len() - 1], 1_000.0),
            'M' | 'm' => (&t[..t.len() - 1], 1_000_000.0),
            _ => (t.as_str(), 1.0),
        },
    };
    let v: f64 = if mult > 1.0 {
        num.replace(',', ".").parse().ok()?
    } else {
        num.replace([',', '.'], "").parse().ok()?
    };
    Some((v * mult).round() as u64)
}

fn contains_any(s: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| s.contains(n))
}

/// Parses X's engagement group label, e.g. `"5 replies, 12 reposts, 87 likes, 3 bookmarks, 2,345 views"`.
/// German, Spanish and French labels are understood too (BRW-19).
pub fn parse_metrics_label(label: &str) -> Metrics {
    let mut m = Metrics::default();
    for part in label.split(", ") {
        let part = part.trim();
        let mut words = part.split_whitespace();
        let Some(first) = words.next() else { continue };
        let mut n = first.to_string();
        let mut rest: Vec<&str> = words.collect();
        while rest.first().is_some_and(|w| w.len() == 3 && w.chars().all(|c| c.is_ascii_digit())) && n.chars().all(|c| c.is_ascii_digit()) {
            n.push_str(rest.remove(0));
        }
        let (suffix, kind_words) = match rest.first().filter(|w| multiplier(w).is_some() && rest.len() > 1) {
            Some(w) => (Some(*w), &rest[1..]),
            None => (None, &rest[..]),
        };
        let Some(n) = parse_count(&n, suffix) else { continue };
        let kind = kind_words.join(" ").to_lowercase();
        if kind.starts_with("repl") || contains_any(&kind, &["antwort", "respuesta", "réponse", "reponse", "risposta"]) {
            m.replies = n;
        } else if kind.starts_with("repost") || contains_any(&kind, &["retweet", "retuit", "republi", "reposte"]) {
            m.reposts = n;
        } else if contains_any(&kind, &["like", "gusta", "aime", "gefällt", "mi piace"]) {
            m.likes = n;
        } else if contains_any(&kind, &["bookmark", "lesezeichen", "marcador", "signet", "favori", "segnalibr"]) {
            m.bookmarks = n;
        } else if contains_any(&kind, &["view", "aufruf", "impression", "visualiz", "vue", "ansicht"]) {
            m.views = n;
        }
    }
    m
}

/// Parses `YYYY-MM-DDTHH:MM:SS[.fff]Z` into unix seconds.
pub fn parse_rfc3339_unix(s: &str) -> Option<i64> {
    let s = s.trim();
    let (date, time) = s.split_once('T')?;
    let mut d = date.split('-');
    let (y, m, day): (i64, i64, i64) = (d.next()?.parse().ok()?, d.next()?.parse().ok()?, d.next()?.parse().ok()?);
    let time = time.trim_end_matches('Z');
    let time = time.split(['+', '.']).next()?;
    let mut t = time.split(':');
    let (h, mi, sec): (i64, i64, i64) = (t.next()?.parse().ok()?, t.next()?.parse().ok()?, t.next().unwrap_or("0").parse().ok()?);
    // days_from_civil (Hinnant)
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2.rem_euclid(400);
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + h * 3600 + mi * 60 + sec)
}

/// Engagement per minute, weighted toward the signals that predict reply headroom. Age is floored at
/// five minutes so a seconds-old post does not look infinitely fast.
pub fn velocity(m: &Metrics, age_secs: i64) -> f64 {
    let weighted = m.likes as f64 + 2.0 * m.reposts as f64 + 1.5 * m.replies as f64 + 2.0 * m.bookmarks as f64;
    weighted / (age_secs.max(300) as f64 / 60.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_metric_labels() {
        let m = parse_metrics_label("5 replies, 12 reposts, 87 likes, 3 bookmarks, 2,345 views");
        assert_eq!(m, Metrics { replies: 5, reposts: 12, likes: 87, bookmarks: 3, views: 2345 });
        assert_eq!(parse_metrics_label("1 reply, 1.2K likes").likes, 1200);
        assert_eq!(parse_metrics_label("garbage"), Metrics::default());
    }

    #[test]
    fn parses_german_spanish_and_french_labels() {
        let de = parse_metrics_label("5 Antworten, 12 Reposts, 87 Likes, 3 Lesezeichen, 2.345 Impressionen");
        assert_eq!(de, Metrics { replies: 5, reposts: 12, likes: 87, bookmarks: 3, views: 2345 });
        assert_eq!(parse_metrics_label("1,2 Mio. Impressionen").views, 1_200_000);
        let es = parse_metrics_label("2 respuestas, 4 retuits, 1,2 mil Me gusta, 7 marcadores, 900 visualizaciones");
        assert_eq!((es.replies, es.reposts, es.likes, es.bookmarks, es.views), (2, 4, 1200, 7, 900));
        let fr = parse_metrics_label("3 réponses, 8 reposts, 40 J’aime, 2 signets, 1 234 vues");
        assert_eq!((fr.replies, fr.reposts, fr.likes, fr.bookmarks, fr.views), (3, 8, 40, 2, 1234));
    }

    fn sample(v: serde_json::Value) -> ScrapedPost {
        serde_json::from_value(v).unwrap()
    }

    /// Five edge-case cards in the shape `extract_feed_js` returns (BRW-19).
    #[test]
    fn edge_case_cards_deserialize_and_carry_their_flags() {
        use serde_json::json;
        let base = |id: &str| json!({ "id": id, "handle": "a", "author": "A", "text": "hello", "lang": "en", "time": "2026-11-26T10:00:00Z" });
        let mut truncated = base("1");
        truncated["truncated"] = json!(true);
        let mut sensitive = base("2");
        sensitive["sensitive"] = json!(true);
        sensitive["has_media"] = json!(true);
        let mut quote = base("3");
        quote["quoted"] = json!("the original take");
        let mut reply = base("4");
        reply["parent"] = json!("what do you think about rust?");
        reply["replying_to"] = json!("naval");
        let mut images = base("5");
        images["image_urls"] = json!(["https://pbs.twimg.com/media/x.jpg?name=small"]);
        images["metrics_label"] = json!("3 Antworten, 1,5 Tsd. Likes");

        assert!(sample(truncated).truncated);
        assert!(sample(sensitive).sensitive);
        assert_eq!(sample(quote).context_text().as_deref(), Some("Quoted post: the original take"));
        assert_eq!(sample(reply).context_text().as_deref(), Some("Replying to @naval: what do you think about rust?"));
        let im = sample(images);
        assert_eq!(im.image_urls.len(), 1);
        assert_eq!(parse_metrics_label(im.metrics_label.as_deref().unwrap()).likes, 1500);
        assert_eq!(sample(base("6")).context_text(), None, "a plain post has no context");
    }

    #[test]
    fn parses_timestamps_consistently_with_util() {
        assert_eq!(parse_rfc3339_unix("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_rfc3339_unix("2023-11-14T22:13:20.000Z"), Some(1_700_000_000));
        assert_eq!(parse_rfc3339_unix(&crate::util::format_unix(951_782_400)), Some(951_782_400));
        assert_eq!(parse_rfc3339_unix("nope"), None);
    }

    #[test]
    fn velocity_rises_with_engagement_and_falls_with_age() {
        let m = Metrics { likes: 100, ..Default::default() };
        assert!(velocity(&m, 600) > velocity(&m, 6000));
        assert_eq!(velocity(&m, 10), velocity(&m, 300));
    }
}
