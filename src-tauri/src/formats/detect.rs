//! Virality detection (OGC-01): flags posts that over-perform for the size of their author or that
//! readers save at an unusual rate. Pure arithmetic, so it runs on every scout and watchlist scan for free.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PostMetrics {
    pub likes: u64,
    pub reposts: u64,
    pub replies: u64,
    pub bookmarks: u64,
    pub views: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PostSignals {
    pub author_followers: Option<u64>,
    pub metrics: PostMetrics,
}

/// User-tunable thresholds. The defaults are a starting point that needs tuning against real feeds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ViralThresholds {
    /// Weighted engagement below this is never flagged, whatever the ratios say.
    pub min_engagement: f64,
    /// Weighted engagement divided by follower count must reach this to count as over-performing.
    pub reach_multiple: f64,
    /// Bookmarks divided by likes must reach this to count as unusually saved.
    pub bookmark_ratio: f64,
    /// Bookmarks needed before the ratio is trusted.
    pub min_bookmarks: u64,
    /// Follower count assumed when the feed does not show one.
    pub assumed_followers: u64,
}

impl Default for ViralThresholds {
    fn default() -> Self {
        Self { min_engagement: 150.0, reach_multiple: 0.25, bookmark_ratio: 0.15, min_bookmarks: 15, assumed_followers: 5_000 }
    }
}

impl ViralThresholds {
    pub fn validate(&self) -> Result<(), String> {
        if !(self.min_engagement >= 1.0 && self.min_engagement <= 1_000_000.0) {
            return Err("minimum engagement must be between 1 and 1,000,000".into());
        }
        if !(self.reach_multiple > 0.0 && self.reach_multiple <= 100.0) {
            return Err("reach multiple must be above 0 and at most 100".into());
        }
        if !(self.bookmark_ratio > 0.0 && self.bookmark_ratio <= 5.0) {
            return Err("bookmark ratio must be above 0 and at most 5".into());
        }
        if self.assumed_followers == 0 {
            return Err("assumed followers must be at least 1".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ViralVerdict {
    pub flagged: bool,
    /// 0..=100, how strongly the post over-performs.
    pub score: f64,
    pub engagement: f64,
    pub reach_multiple: f64,
    pub bookmark_ratio: f64,
    pub reasons: Vec<String>,
}

/// Replies and reposts show intent to join in or share; bookmarks show lasting value.
pub fn weighted_engagement(m: &PostMetrics) -> f64 {
    m.likes as f64 + 2.0 * m.reposts as f64 + m.replies as f64 + 2.0 * m.bookmarks as f64
}

pub fn evaluate(signals: &PostSignals, t: &ViralThresholds) -> ViralVerdict {
    let m = &signals.metrics;
    let engagement = weighted_engagement(m);
    let (followers, assumed) = match signals.author_followers {
        Some(f) if f > 0 => (f, false),
        _ => (t.assumed_followers, true),
    };
    // A floor keeps brand-new accounts from flagging on a handful of likes.
    let reach_multiple = engagement / followers.max(200) as f64;
    let bookmark_ratio = if m.likes > 0 { m.bookmarks as f64 / m.likes as f64 } else { 0.0 };

    let mut reasons = Vec::new();
    let enough = engagement >= t.min_engagement;
    let overperforms = enough && reach_multiple >= t.reach_multiple;
    if overperforms {
        let basis = if assumed { format!("an assumed {followers} followers") } else { format!("{followers} followers") };
        reasons.push(format!("Engagement is {:.0}% of the author's reach ({basis}).", reach_multiple * 100.0));
    }
    let saved = enough && m.bookmarks >= t.min_bookmarks && bookmark_ratio >= t.bookmark_ratio;
    if saved {
        reasons.push(format!("{:.0}% as many bookmarks as likes, so people are saving it.", bookmark_ratio * 100.0));
    }

    let reach_pts = 60.0 * (reach_multiple / (t.reach_multiple * 4.0)).min(1.0);
    let save_pts = 40.0 * (bookmark_ratio / (t.bookmark_ratio * 2.0)).min(1.0);
    let score = if enough { (reach_pts + save_pts).clamp(0.0, 100.0) } else { 0.0 };

    ViralVerdict {
        flagged: overperforms || saved,
        score: (score * 10.0).round() / 10.0,
        engagement,
        reach_multiple,
        bookmark_ratio,
        reasons,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sig(followers: Option<u64>, likes: u64, reposts: u64, replies: u64, bookmarks: u64) -> PostSignals {
        PostSignals { author_followers: followers, metrics: PostMetrics { likes, reposts, replies, bookmarks, views: None } }
    }

    #[test]
    fn small_author_with_big_engagement_is_flagged() {
        let v = evaluate(&sig(Some(2_000), 900, 120, 60, 80), &ViralThresholds::default());
        assert!(v.flagged);
        assert!(v.score > 40.0);
        assert!(v.reasons[0].contains("reach"));
    }

    #[test]
    fn large_author_with_ordinary_engagement_is_not() {
        let v = evaluate(&sig(Some(1_000_000), 800, 100, 50, 40), &ViralThresholds::default());
        assert!(!v.flagged);
        assert!(v.reasons.is_empty());
    }

    #[test]
    fn heavily_bookmarked_post_is_flagged_even_for_a_big_author() {
        let v = evaluate(&sig(Some(500_000), 300, 10, 10, 120), &ViralThresholds::default());
        assert!(v.flagged);
        assert!(v.reasons.iter().any(|r| r.contains("saving")));
    }

    #[test]
    fn engagement_floor_blocks_tiny_numbers() {
        let v = evaluate(&sig(Some(10), 12, 3, 2, 2), &ViralThresholds::default());
        assert!(!v.flagged);
        assert_eq!(v.score, 0.0);
    }

    #[test]
    fn unknown_followers_use_the_assumed_size_and_say_so() {
        let v = evaluate(&sig(None, 2_000, 200, 100, 150), &ViralThresholds::default());
        assert!(v.flagged);
        assert!(v.reasons[0].contains("assumed 5000 followers"));
    }

    #[test]
    fn loosening_a_threshold_flags_more() {
        let s = sig(Some(10_000), 700, 40, 30, 20);
        assert!(!evaluate(&s, &ViralThresholds::default()).flagged);
        let loose = ViralThresholds { reach_multiple: 0.05, ..Default::default() };
        assert!(evaluate(&s, &loose).flagged);
    }

    #[test]
    fn thresholds_reject_nonsense() {
        assert!(ViralThresholds { reach_multiple: 0.0, ..Default::default() }.validate().is_err());
        assert!(ViralThresholds::default().validate().is_ok());
    }
}
