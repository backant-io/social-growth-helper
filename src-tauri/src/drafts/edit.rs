//! Edit distance (DRF-07) and pre-publish checks (DRF-10).

use super::bundle::{text_action, ActionKind};

/// Character-level Levenshtein distance.
pub fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur.push((prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    prev[b.len()]
}

pub const DEFAULT_MAX_CHARS: usize = 280;

fn norm(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// Text-only checks shared by replies, quotes and original posts: empty, too long, banned phrase, exact repeat.
pub fn text_problems(text: &str, max_chars: usize, banned: &[String], recent_published: &[String]) -> Vec<String> {
    let mut problems = Vec::new();
    let t = text.trim();
    if t.is_empty() {
        problems.push("The text is empty.".into());
        return problems;
    }
    let len = t.chars().count();
    if len > max_chars {
        problems.push(format!("The text is {len} characters; the limit is {max_chars}."));
    }
    for b in banned {
        if !b.trim().is_empty() && norm(t).contains(&norm(b)) {
            problems.push(format!("Contains a banned phrase: \"{b}\"."));
        }
    }
    if recent_published.iter().any(|r| norm(r) == norm(t)) {
        problems.push("You already posted exactly this text recently.".into());
    }
    problems
}

/// Returns a human-readable reason for every failed check; empty means the draft may be published.
/// "Post still exists" is verified by the executor when it opens the post.
pub fn pre_publish_problems(
    text: &str,
    bundle: &[ActionKind],
    max_chars: usize,
    banned: &[String],
    recent_published: &[String],
) -> Vec<String> {
    let mut problems = Vec::new();
    if bundle.is_empty() {
        problems.push("No action is selected.".to_string());
    }
    if text_action(bundle).is_none() {
        return problems;
    }
    problems.extend(text_problems(text, max_chars, banned, recent_published));
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levenshtein_known_values() {
        assert_eq!(edit_distance("kitten", "sitting"), 3);
        assert_eq!(edit_distance("", "abc"), 3);
        assert_eq!(edit_distance("same", "same"), 0);
        assert_eq!(edit_distance("héllo", "hello"), 1);
    }

    #[test]
    fn checks_length_banned_and_duplicates() {
        let b = [ActionKind::Like, ActionKind::Reply];
        assert!(pre_publish_problems("fine", &b, 280, &[], &[]).is_empty());
        assert_eq!(pre_publish_problems("   ", &b, 280, &[], &[]).len(), 1);
        assert!(pre_publish_problems(&"x".repeat(281), &b, 280, &[], &[])[0].contains("281"));
        assert!(pre_publish_problems("This is Game Changing", &b, 280, &["game changing".into()], &[])[0].contains("banned"));
        assert!(pre_publish_problems("Same  text", &b, 280, &[], &["same text".into()])[0].contains("already posted"));
    }

    #[test]
    fn no_text_bundles_need_no_text() {
        assert!(pre_publish_problems("", &[ActionKind::Like, ActionKind::Follow], 280, &[], &[]).is_empty());
        assert_eq!(pre_publish_problems("", &[], 280, &[], &[]).len(), 1);
    }
}
