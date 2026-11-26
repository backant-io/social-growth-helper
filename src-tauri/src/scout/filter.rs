//! Local pre-filter (SCT-05, SCT-15, SCT-21). Runs before any model call so cheap rules save quota.

use super::model::{parse_rfc3339_unix, ScrapedPost};
use serde::Serialize;

pub const DEFAULT_MAX_AGE_SECS: i64 = 6 * 3600;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dropped {
    Invalid,
    Promoted,
    Own,
    Seen,
    Stale,
    ReplyRestricted,
    Avoided,
    Language,
    Sensitive,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct FilterCounts {
    pub invalid: u32,
    pub promoted: u32,
    pub own: u32,
    pub seen: u32,
    pub stale: u32,
    pub reply_restricted: u32,
    pub avoided: u32,
    pub language: u32,
    pub sensitive: u32,
}

impl FilterCounts {
    pub fn record(&mut self, d: Dropped) {
        match d {
            Dropped::Invalid => self.invalid += 1,
            Dropped::Promoted => self.promoted += 1,
            Dropped::Own => self.own += 1,
            Dropped::Seen => self.seen += 1,
            Dropped::Stale => self.stale += 1,
            Dropped::ReplyRestricted => self.reply_restricted += 1,
            Dropped::Avoided => self.avoided += 1,
            Dropped::Language => self.language += 1,
            Dropped::Sensitive => self.sensitive += 1,
        }
    }

    pub fn total(&self) -> u32 {
        self.invalid + self.promoted + self.own + self.seen + self.stale + self.reply_restricted + self.avoided + self.language + self.sensitive
    }
}

pub struct FilterConfig {
    pub now_unix: i64,
    pub max_age_secs: i64,
    pub own_handle: Option<String>,
    pub avoid_accounts: Vec<String>,
    /// Lower-case language codes from onboarding. Empty means accept everything.
    pub languages: Vec<String>,
}

fn lang_matches(post_lang: &str, wanted: &[String]) -> bool {
    let l = post_lang.to_lowercase();
    let base = l.split('-').next().unwrap_or("");
    wanted.iter().any(|w| w == &l || w.split('-').next() == Some(base))
}

/// `None` keeps the post. `seen` answers whether this post id already has an evaluation.
pub fn classify(p: &ScrapedPost, cfg: &FilterConfig, seen: &dyn Fn(&str) -> bool) -> Option<Dropped> {
    let (Some(id), Some(handle)) = (p.id.as_deref(), p.handle.as_deref()) else { return Some(Dropped::Invalid) };
    if p.text.trim().is_empty() && !p.has_media {
        return Some(Dropped::Invalid);
    }
    if p.promoted {
        return Some(Dropped::Promoted);
    }
    if cfg.own_handle.as_deref().is_some_and(|me| me.eq_ignore_ascii_case(handle)) {
        return Some(Dropped::Own);
    }
    if cfg.avoid_accounts.iter().any(|a| a.eq_ignore_ascii_case(handle)) {
        return Some(Dropped::Avoided);
    }
    if seen(id) {
        return Some(Dropped::Seen);
    }
    if p.sensitive {
        return Some(Dropped::Sensitive);
    }
    if p.reply_restricted {
        return Some(Dropped::ReplyRestricted);
    }
    if let Some(t) = p.time.as_deref().and_then(parse_rfc3339_unix) {
        if cfg.now_unix - t > cfg.max_age_secs {
            return Some(Dropped::Stale);
        }
    }
    if !cfg.languages.is_empty() {
        if let Some(l) = p.lang.as_deref().filter(|l| !l.is_empty() && *l != "und" && *l != "zxx") {
            if !lang_matches(l, &cfg.languages) {
                return Some(Dropped::Language);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn post(id: &str, handle: &str) -> ScrapedPost {
        ScrapedPost {
            id: Some(id.into()),
            handle: Some(handle.into()),
            text: "hello world".into(),
            lang: Some("en".into()),
            time: Some("2026-11-26T10:00:00Z".into()),
            ..Default::default()
        }
    }

    fn cfg() -> FilterConfig {
        FilterConfig {
            now_unix: parse_rfc3339_unix("2026-11-26T11:00:00Z").unwrap(),
            max_age_secs: DEFAULT_MAX_AGE_SECS,
            own_handle: Some("me".into()),
            avoid_accounts: vec!["spammer".into()],
            languages: vec!["en".into()],
        }
    }

    fn never(_: &str) -> bool {
        false
    }

    #[test]
    fn keeps_a_normal_post() {
        assert_eq!(classify(&post("1", "naval"), &cfg(), &never), None);
    }

    #[test]
    fn drops_each_category() {
        let c = cfg();
        let mut p = post("1", "naval");
        p.promoted = true;
        assert_eq!(classify(&p, &c, &never), Some(Dropped::Promoted));
        assert_eq!(classify(&post("2", "ME"), &c, &never), Some(Dropped::Own));
        assert_eq!(classify(&post("3", "Spammer"), &c, &never), Some(Dropped::Avoided));
        assert_eq!(classify(&post("4", "a"), &c, &|id| id == "4"), Some(Dropped::Seen));
        let mut r = post("5", "a");
        r.reply_restricted = true;
        assert_eq!(classify(&r, &c, &never), Some(Dropped::ReplyRestricted));
        let mut old = post("6", "a");
        old.time = Some("2026-11-26T01:00:00Z".into());
        assert_eq!(classify(&old, &c, &never), Some(Dropped::Stale));
        let mut de = post("7", "a");
        de.lang = Some("de".into());
        assert_eq!(classify(&de, &c, &never), Some(Dropped::Language));
        assert_eq!(classify(&ScrapedPost::default(), &c, &never), Some(Dropped::Invalid));
    }

    #[test]
    fn language_match_ignores_region_and_unknown_tags() {
        let c = cfg();
        let mut p = post("1", "a");
        p.lang = Some("en-GB".into());
        assert_eq!(classify(&p, &c, &never), None);
        p.lang = Some("und".into());
        assert_eq!(classify(&p, &c, &never), None);
    }

    #[test]
    fn counts_add_up() {
        let mut c = FilterCounts::default();
        c.record(Dropped::Seen);
        c.record(Dropped::Stale);
        c.record(Dropped::Stale);
        assert_eq!((c.seen, c.stale, c.total()), (1, 2, 3));
    }
}
