//! Format clustering (OGC-04): decides whether a new blueprint is the same format as one already stored,
//! so the library tracks one template with a count instead of many near-duplicates.

use super::blueprint::{placeholders, FormatBlueprint};
use std::collections::BTreeSet;

pub const MERGE_THRESHOLD: f64 = 0.55;

const STOPWORDS: &[&str] = &[
    "a", "an", "the", "of", "to", "and", "or", "in", "on", "for", "with", "then", "is", "it", "that", "this", "by", "at", "as", "be",
];

fn tokens(bp: &FormatBlueprint) -> BTreeSet<String> {
    // Slot names are folded to one marker so two formats with differently named slots still match.
    let mut skeleton = bp.skeleton.clone();
    for p in placeholders(&skeleton) {
        skeleton = skeleton.replace(&format!("{{{p}}}"), " slotmark ");
    }
    let text = format!("{} {} {}", bp.hook_formula, bp.structural_pattern, skeleton);
    text.split(|c: char| !c.is_alphanumeric())
        .map(|w| w.to_lowercase())
        .filter(|w| w.len() > 1 && !STOPWORDS.contains(&w.as_str()))
        .collect()
}

pub fn similarity(a: &FormatBlueprint, b: &FormatBlueprint) -> f64 {
    let (ta, tb) = (tokens(a), tokens(b));
    if ta.is_empty() || tb.is_empty() {
        return 0.0;
    }
    let inter = ta.intersection(&tb).count() as f64;
    let union = ta.union(&tb).count() as f64;
    let mut score = inter / union;
    if a.archetype == b.archetype {
        score += 0.1;
    }
    let (sa, sb) = (a.slots.len() as f64, b.slots.len() as f64);
    if (sa - sb).abs() <= 1.0 {
        score += 0.05;
    }
    score.min(1.0)
}

/// Best existing match at or above `threshold`, as `(index, score)`.
pub fn find_match(existing: &[FormatBlueprint], new: &FormatBlueprint, threshold: f64) -> Option<(usize, f64)> {
    existing
        .iter()
        .enumerate()
        .map(|(i, e)| (i, similarity(e, new)))
        .filter(|(_, s)| *s >= threshold)
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formats::blueprint::Slot;

    fn bp(archetype: &str, hook: &str, pattern: &str, skeleton: &str, slots: &[&str]) -> FormatBlueprint {
        FormatBlueprint {
            name: "x format".into(),
            archetype: archetype.into(),
            hook_formula: hook.into(),
            structural_pattern: pattern.into(),
            skeleton: skeleton.into(),
            slots: slots.iter().map(|k| Slot { key: k.to_string(), label: k.to_string(), hint: String::new() }).collect(),
            media_hint: "text_only".into(),
        }
    }

    fn contrarian() -> FormatBlueprint {
        bp(
            "contrarian_take",
            "State a common belief then say it is wrong",
            "One line hook, three short proof lines, one line twist",
            "Unpopular opinion: {belief} is wrong.\n{p1}\n{p2}\n{p3}\n{twist}",
            &["belief", "p1", "p2", "p3", "twist"],
        )
    }

    #[test]
    fn rephrased_equivalent_format_merges() {
        let again = bp(
            "contrarian_take",
            "Name a belief most people hold and say it is wrong",
            "A one line hook, then three short lines of proof, then a one line twist",
            "Hot take: {thing_everyone_believes} is wrong.\n{a}\n{b}\n{c}\n{punchline}",
            &["thing_everyone_believes", "a", "b", "c", "punchline"],
        );
        assert!(similarity(&contrarian(), &again) >= MERGE_THRESHOLD);
        assert_eq!(find_match(&[contrarian()], &again, MERGE_THRESHOLD).unwrap().0, 0);
    }

    #[test]
    fn different_format_stays_separate() {
        let listicle = bp(
            "listicle",
            "Promise a number of lessons learned from one experience",
            "Hook line, numbered list of five lessons, closing call to follow",
            "I {experience} for {duration}. Here are 5 lessons:\n1. {l1}\n2. {l2}\n3. {l3}\n4. {l4}\n5. {l5}\n\n{cta}",
            &["experience", "duration", "l1", "l2", "l3", "l4", "l5", "cta"],
        );
        assert!(find_match(&[contrarian()], &listicle, MERGE_THRESHOLD).is_none());
    }

    #[test]
    fn picks_the_closest_of_several() {
        let other = bp("listicle", "Promise lessons", "Numbered list", "{a}\n1. {b}\n2. {c}", &["a", "b", "c"]);
        let (i, _) = find_match(&[other, contrarian()], &contrarian(), MERGE_THRESHOLD).unwrap();
        assert_eq!(i, 1);
    }
}
