//! Originality check (OGC-13): warns when a generated post is too close to the example it was inspired by.
//! The threshold is the user's to adjust; the default is moderate.

use serde::Serialize;
use std::collections::BTreeSet;

pub const DEFAULT_THRESHOLD: f64 = 0.5;
pub const MIN_THRESHOLD: f64 = 0.2;
pub const MAX_THRESHOLD: f64 = 0.95;

fn normalise(text: &str) -> Vec<String> {
    text.split_whitespace()
        .filter(|w| !w.starts_with("http://") && !w.starts_with("https://"))
        .flat_map(|w| {
            w.split(|c: char| !c.is_alphanumeric() && c != '\'')
                .filter(|p| !p.is_empty())
                .map(|p| p.to_lowercase())
                .collect::<Vec<_>>()
        })
        .collect()
}

fn shingles(words: &[String], n: usize) -> BTreeSet<String> {
    if words.len() < n {
        return std::iter::once(words.join(" ")).collect();
    }
    words.windows(n).map(|w| w.join(" ")).collect()
}

/// 0..=1. The larger of the overlap relative to both texts (Jaccard) and to the draft alone (containment),
/// so a draft that is a trimmed copy of its source scores high even when the source is much longer.
pub fn score(draft: &str, source: &str) -> f64 {
    let (a, b) = (normalise(draft), normalise(source));
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let (sa, sb) = (shingles(&a, 3), shingles(&b, 3));
    let inter = sa.intersection(&sb).count() as f64;
    let jaccard = inter / sa.union(&sb).count() as f64;
    let containment = inter / sa.len() as f64;
    jaccard.max(containment)
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SimilarityReport {
    pub score: f64,
    pub threshold: f64,
    pub flagged: bool,
    /// Index into the sources that was compared, for showing which example it resembles.
    pub closest: Option<usize>,
}

pub fn check(draft: &str, sources: &[String], threshold: f64) -> SimilarityReport {
    let best = sources
        .iter()
        .enumerate()
        .map(|(i, s)| (i, score(draft, s)))
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    let (closest, score) = match best {
        Some((i, s)) => (Some(i), s),
        None => (None, 0.0),
    };
    SimilarityReport { score: (score * 100.0).round() / 100.0, threshold, flagged: score >= threshold, closest }
}

pub fn validate_threshold(t: f64) -> Result<f64, String> {
    if (MIN_THRESHOLD..=MAX_THRESHOLD).contains(&t) {
        Ok(t)
    } else {
        Err(format!("the similarity threshold must be between {MIN_THRESHOLD} and {MAX_THRESHOLD}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = "Unpopular opinion: hustle culture is wrong. It burns people out before they find the work they love. Rest is a strategy.";

    #[test]
    fn pasting_the_source_is_flagged() {
        let r = check(SOURCE, &[SOURCE.to_string()], DEFAULT_THRESHOLD);
        assert!(r.flagged);
        assert!(r.score > 0.95);
    }

    #[test]
    fn a_trimmed_copy_is_flagged_even_though_the_source_is_longer() {
        let r = check("Unpopular opinion: hustle culture is wrong. It burns people out before they find the work they love.", &[SOURCE.to_string()], DEFAULT_THRESHOLD);
        assert!(r.flagged);
    }

    #[test]
    fn same_structure_new_subject_is_not_flagged() {
        let r = check(
            "Unpopular opinion: daily standups are wrong. They interrupt deep work for status theatre. Async updates win.",
            &[SOURCE.to_string()],
            DEFAULT_THRESHOLD,
        );
        assert!(!r.flagged, "score was {}", r.score);
    }

    #[test]
    fn moving_the_threshold_changes_what_triggers() {
        let partial = "Unpopular opinion: hustle culture is wrong. Nobody admits that sleep builds careers.";
        let s = score(partial, SOURCE);
        assert!(check(partial, &[SOURCE.to_string()], (s - 0.05).max(MIN_THRESHOLD)).flagged);
        assert!(!check(partial, &[SOURCE.to_string()], (s + 0.05).min(MAX_THRESHOLD)).flagged);
    }

    #[test]
    fn links_and_case_do_not_matter_and_no_sources_means_no_flag() {
        assert!(score(&format!("{SOURCE} https://t.co/abc"), &SOURCE.to_uppercase()) > 0.95);
        assert!(!check("anything", &[], 0.5).flagged);
        assert_eq!(score("", SOURCE), 0.0);
    }

    #[test]
    fn threshold_bounds_are_enforced() {
        assert!(validate_threshold(0.5).is_ok());
        assert!(validate_threshold(0.05).is_err());
        assert!(validate_threshold(1.0).is_err());
    }
}
