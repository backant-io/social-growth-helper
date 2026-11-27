//! Outlier & breakout virality scoring for Scout and Creators.
//!
//! Combines X recommendation mechanics (heavy bookmark/repost/reply weighting, early-wave reply
//! headroom, small-account over-performance relative to follower count) with feed-visible signals
//! (engagement per 1k views, save ratio, share ratio) when follower counts are unavailable.

use super::model::Metrics;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct OutlierSignals<'a> {
    pub metrics: &'a Metrics,
    pub age_minutes: i64,
    pub author_followers: Option<i64>,
    pub author_verified: bool,
    pub is_repost: bool,
    pub has_media: bool,
    pub text: &'a str,
    pub has_context: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OutlierReport {
    /// 0..=100 score representing how strongly this post over-performs & invites high-leverage replies.
    pub outlier_score: f64,
    /// Machine-readable tags: `breakout`, `small_account`, `early_wave`, `high_saves`,
    /// `high_shares`, `high_view_efficiency`, `conversation_starter`.
    pub breakout_flags: Vec<String>,
    pub engagement: f64,
    pub reach_multiple: Option<f64>,
    pub views_multiple: Option<f64>,
    pub engagement_per_1k_views: Option<f64>,
    pub bookmark_ratio: f64,
    pub share_ratio: f64,
    /// 0..=1 estimate of whether an early reply can still earn visibility near the top.
    pub reply_headroom: f64,
    /// Short, factual explanation grounded strictly in observed metrics.
    pub why: String,
}

/// Weighted engagement emphasizing explicit intent signals (bookmarks & reposts > replies > likes).
pub fn weighted_engagement(m: &Metrics) -> f64 {
    m.likes as f64 + 2.0 * m.reposts as f64 + 1.5 * m.replies as f64 + 2.5 * m.bookmarks as f64
}

pub fn has_open_loop_cue(text: &str) -> bool {
    let lower = text.to_lowercase();
    text.contains('?')
        || lower.contains("how do you")
        || lower.contains("what's your")
        || lower.contains("whats your")
        || lower.contains("unpopular opinion")
        || lower.contains("hot take")
        || lower.contains("change my mind")
        || lower.contains("lesson")
        || lower.contains("framework")
}

fn looks_like_ratio_or_giveaway(m: &Metrics, text: &str) -> bool {
    let lower = text.to_lowercase();
    let giveaway = lower.contains("drop a") && (lower.contains("comment") || lower.contains("dm") || lower.contains("send you"))
        || lower.contains("like and rt")
        || lower.contains("giveaway");
    let ratioed = m.replies >= 25 && m.replies > m.likes.saturating_mul(2) && m.bookmarks <= m.likes / 20;
    giveaway || ratioed
}

pub fn evaluate_outlier(sig: &OutlierSignals<'_>) -> OutlierReport {
    let m = sig.metrics;
    let engagement = weighted_engagement(m);
    let age = sig.age_minutes.max(1);

    let followers = sig.author_followers.filter(|&f| f > 0);
    let reach_multiple = followers.map(|f| engagement / (f.max(200) as f64));
    let views_multiple = followers.and_then(|f| (m.views > 0).then(|| m.views as f64 / (f.max(200) as f64)));
    let engagement_per_1k_views = (m.views >= 100).then(|| engagement / (m.views as f64 / 1_000.0));

    let bookmark_ratio = if m.likes >= 5 { m.bookmarks as f64 / m.likes as f64 } else { 0.0 };
    let share_ratio = if m.likes >= 5 { m.reposts as f64 / m.likes as f64 } else { 0.0 };

    // Reply headroom: high when fresh and not buried under hundreds of replies.
    let age_factor: f64 = if age <= 45 {
        1.0
    } else if age <= 120 {
        0.8
    } else if age <= 240 {
        0.5
    } else {
        0.25
    };
    let reply_crowding: f64 = if m.replies <= 15 {
        1.0
    } else if m.replies <= 45 {
        0.8
    } else if m.replies <= 120 {
        0.45
    } else {
        0.15
    };
    let reply_headroom = ((age_factor * 0.55 + reply_crowding * 0.45) * 100.0).round() / 100.0;

    // 1. Follower-normalized breakout component (0..42 when followers known).
    let follower_pts = match (followers, reach_multiple, views_multiple) {
        (Some(f), Some(rm), vm) => {
            let reach_score = (rm / 0.35).min(1.0) * 26.0;
            let view_score = vm.map(|v| (v / 2.5).min(1.0) * 10.0).unwrap_or(0.0);
            let small_account_bonus = if f <= 50_000 && rm >= 0.12 { 6.0 } else { 0.0 };
            reach_score + view_score + small_account_bonus
        }
        _ => 0.0,
    };

    // 2. View efficiency + depth signals (saves & shares per 1k views / likes).
    let view_eff_pts = engagement_per_1k_views
        .map(|epk| (epk / 45.0).min(1.0) * 26.0)
        .unwrap_or_else(|| ((engagement + 1.0).log10() * 8.0).min(20.0));
    let save_pts = if m.bookmarks >= 5 { (bookmark_ratio / 0.25).min(1.0) * 18.0 } else { 0.0 };
    let share_pts = if m.reposts >= 5 { (share_ratio / 0.25).min(1.0) * 12.0 } else { 0.0 };

    // 3. Velocity & reply headroom (0..28).
    let vel_per_min = engagement / (age.max(5) as f64);
    let vel_pts = ((vel_per_min + 1.0).log10() * 14.0).min(18.0);
    let headroom_pts = reply_headroom * 12.0;

    // 4. Content cues & context.
    let cue_pts = if has_open_loop_cue(sig.text) { 6.0 } else { 0.0 }
        + if sig.has_media { 3.0 } else { 0.0 }
        + if sig.has_context { 2.0 } else { 0.0 };

    // Combine: when follower count is missing, redistribute weight cleanly across view efficiency & depth
    // so feed-only outlier detection stays well-calibrated in 0..100.
    let mut raw = if followers.is_some() {
        follower_pts + view_eff_pts * 0.7 + save_pts + share_pts + vel_pts * 0.7 + headroom_pts + cue_pts
    } else {
        view_eff_pts * 1.35 + save_pts * 1.15 + share_pts + vel_pts + headroom_pts + cue_pts
    };

    // Dampen noisy signals: reposts in feed, tiny engagement, or ratio/giveaway spam.
    if engagement < 10.0 {
        raw *= 0.45;
    } else if engagement < 25.0 && age > 60 {
        raw *= 0.7;
    }
    if sig.is_repost {
        raw -= 6.0;
    }
    if looks_like_ratio_or_giveaway(m, sig.text) {
        raw -= 28.0;
    }

    let outlier_score = (raw.clamp(0.0, 100.0) * 10.0).round() / 10.0;

    let mut flags = Vec::new();
    let is_small_breakout = followers.is_some_and(|f| f <= 50_000) && reach_multiple.is_some_and(|rm| rm >= 0.15) && engagement >= 25.0;
    if is_small_breakout {
        flags.push("small_account".into());
    }
    if outlier_score >= 68.0 {
        flags.push("breakout".into());
    }
    if age <= 90 && m.replies <= 35 && engagement >= 15.0 {
        flags.push("early_wave".into());
    }
    if m.bookmarks >= 8 && bookmark_ratio >= 0.15 {
        flags.push("high_saves".into());
    }
    if m.reposts >= 8 && share_ratio >= 0.15 {
        flags.push("high_shares".into());
    }
    if engagement_per_1k_views.is_some_and(|epk| epk >= 28.0) && m.views >= 200 {
        flags.push("high_view_efficiency".into());
    }
    if has_open_loop_cue(sig.text) && reply_headroom >= 0.6 {
        flags.push("conversation_starter".into());
    }

    let why = build_why(sig, engagement, reach_multiple, views_multiple, engagement_per_1k_views, bookmark_ratio);

    OutlierReport {
        outlier_score,
        breakout_flags: flags,
        engagement: (engagement * 10.0).round() / 10.0,
        reach_multiple,
        views_multiple,
        engagement_per_1k_views,
        bookmark_ratio,
        share_ratio,
        reply_headroom,
        why,
    }
}

fn build_why(
    sig: &OutlierSignals<'_>,
    engagement: f64,
    reach_multiple: Option<f64>,
    views_multiple: Option<f64>,
    epk: Option<f64>,
    bookmark_ratio: f64,
) -> String {
    let m = sig.metrics;
    let mut parts = Vec::new();
    if let (Some(f), Some(rm)) = (sig.author_followers.filter(|&x| x > 0), reach_multiple) {
        if let Some(vm) = views_multiple {
            parts.push(format!("{} views on {} followers ({:.1}x views, {:.0}% engagement reach)", m.views, f, vm, rm * 100.0));
        } else {
            parts.push(format!("{:.0} weighted engagement on {} followers ({:.0}% reach)", engagement, f, rm * 100.0));
        }
    } else if let Some(rate) = epk {
        parts.push(format!("{:.1} weighted eng per 1K views ({} views)", rate, m.views));
    } else {
        parts.push(format!("{:.0} weighted engagement ({} likes, {} replies)", engagement, m.likes, m.replies));
    }

    if m.bookmarks >= 8 && bookmark_ratio >= 0.15 {
        parts.push(format!("{} bookmarks ({:.0}% save-to-like)", m.bookmarks, bookmark_ratio * 100.0));
    }
    if sig.age_minutes <= 90 && m.replies <= 35 {
        parts.push(format!("early wave ({}m old, {} replies)", sig.age_minutes.max(1), m.replies));
    } else if m.replies > 120 {
        parts.push(format!("crowded thread ({} replies)", m.replies));
    }
    parts.join("; ") + "."
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_account_breakout_beats_large_account_routine_post() {
        let small = evaluate_outlier(&OutlierSignals {
            metrics: &Metrics { likes: 110, reposts: 22, replies: 14, bookmarks: 35, views: 3_200 },
            age_minutes: 35,
            author_followers: Some(700),
            author_verified: false,
            is_repost: false,
            has_media: false,
            text: "How do you decide when to split a monolith?",
            has_context: false,
        });
        let big = evaluate_outlier(&OutlierSignals {
            metrics: &Metrics { likes: 400, reposts: 20, replies: 190, bookmarks: 15, views: 95_000 },
            age_minutes: 300,
            author_followers: Some(500_000),
            author_verified: true,
            is_repost: false,
            has_media: false,
            text: "Shipped a small update today.",
            has_context: false,
        });

        assert!(small.outlier_score > big.outlier_score + 20.0, "small={} big={}", small.outlier_score, big.outlier_score);
        assert!(small.breakout_flags.contains(&"small_account".to_string()));
        assert!(small.breakout_flags.contains(&"breakout".to_string()));
        assert!(small.breakout_flags.contains(&"early_wave".to_string()));
        assert!(small.breakout_flags.contains(&"high_saves".to_string()));
        assert!(small.why.contains("700 followers"));
    }

    #[test]
    fn unknown_followers_relies_on_view_efficiency_and_never_invents_followers() {
        let rep = evaluate_outlier(&OutlierSignals {
            metrics: &Metrics { likes: 80, reposts: 16, replies: 10, bookmarks: 28, views: 3_500 },
            age_minutes: 40,
            author_followers: None,
            author_verified: false,
            is_repost: false,
            has_media: true,
            text: "Why local-first tools feel twice as fast?",
            has_context: false,
        });
        assert!(rep.outlier_score >= 68.0, "score={}", rep.outlier_score);
        assert!(rep.breakout_flags.contains(&"high_view_efficiency".to_string()));
        assert!(rep.breakout_flags.contains(&"high_saves".to_string()));
        assert!(rep.reach_multiple.is_none());
        assert!(!rep.why.contains("followers"));
    }

    #[test]
    fn ratioed_or_giveaway_spam_is_penalized() {
        let spam = evaluate_outlier(&OutlierSignals {
            metrics: &Metrics { likes: 12, reposts: 1, replies: 90, bookmarks: 0, views: 5_000 },
            age_minutes: 30,
            author_followers: Some(1_000),
            author_verified: false,
            is_repost: false,
            has_media: false,
            text: "Drop a comment and I'll DM you my checklist!",
            has_context: false,
        });
        assert!(spam.outlier_score < 45.0, "spam score={}", spam.outlier_score);
    }
}
