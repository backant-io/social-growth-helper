//! Text helpers for the edit-diff analyzer (LRN-02): character edit distance and a word-level diff.

/// Levenshtein distance in characters (what `reply_drafts.edit_distance` stores).
pub fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// Lower-cased words with surrounding punctuation trimmed. Hashtags and mentions keep their marker.
pub fn tokens(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|w| {
            w.trim_matches(|c: char| !(c.is_alphanumeric() || c == '#' || c == '@' || is_emoji(c)))
                .to_lowercase()
        })
        .filter(|w| !w.is_empty())
        .collect()
}

pub fn is_emoji(c: char) -> bool {
    matches!(c as u32,
        0x1F300..=0x1FAFF | 0x2600..=0x27BF | 0x1F000..=0x1F2FF | 0x2B00..=0x2BFF | 0xFE0F)
        && c as u32 != 0xFE0F
}

pub fn emoji_count(text: &str) -> usize {
    text.chars().filter(|c| is_emoji(*c)).count()
}

pub fn hashtag_count(text: &str) -> usize {
    text.split_whitespace().filter(|w| w.starts_with('#') && w.len() > 1).count()
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WordDiff {
    /// Contiguous runs of words present in the original but not in the edit.
    pub removed: Vec<Vec<String>>,
    /// Contiguous runs of words present only in the edit.
    pub added: Vec<Vec<String>>,
}

/// Word-level diff via longest common subsequence.
pub fn word_diff(original: &str, edited: &str) -> WordDiff {
    let a = tokens(original);
    let b = tokens(edited);
    let (n, m) = (a.len(), b.len());
    let mut lcs = vec![vec![0u16; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if a[i] == b[j] { lcs[i + 1][j + 1] + 1 } else { lcs[i + 1][j].max(lcs[i][j + 1]) };
        }
    }
    let mut diff = WordDiff::default();
    let (mut i, mut j) = (0, 0);
    let mut rem: Vec<String> = vec![];
    let mut add: Vec<String> = vec![];
    let flush = |rem: &mut Vec<String>, add: &mut Vec<String>, diff: &mut WordDiff| {
        if !rem.is_empty() {
            diff.removed.push(std::mem::take(rem));
        }
        if !add.is_empty() {
            diff.added.push(std::mem::take(add));
        }
    };
    while i < n && j < m {
        if a[i] == b[j] {
            flush(&mut rem, &mut add, &mut diff);
            i += 1;
            j += 1;
        } else if lcs[i + 1][j] >= lcs[i][j + 1] {
            rem.push(a[i].clone());
            i += 1;
        } else {
            add.push(b[j].clone());
            j += 1;
        }
    }
    rem.extend(a[i..].iter().cloned());
    add.extend(b[j..].iter().cloned());
    flush(&mut rem, &mut add, &mut diff);
    diff
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct DiffLine {
    /// "add", "remove" or "same".
    pub kind: &'static str,
    pub text: String,
}

/// Line-level diff used for patch previews and version comparison.
pub fn line_diff(before: &[String], after: &[String]) -> Vec<DiffLine> {
    let (n, m) = (before.len(), after.len());
    let mut lcs = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if before[i] == after[j] { lcs[i + 1][j + 1] + 1 } else { lcs[i + 1][j].max(lcs[i][j + 1]) };
        }
    }
    let mut out = vec![];
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if before[i] == after[j] {
            out.push(DiffLine { kind: "same", text: before[i].clone() });
            i += 1;
            j += 1;
        } else if lcs[i + 1][j] >= lcs[i][j + 1] {
            out.push(DiffLine { kind: "remove", text: before[i].clone() });
            i += 1;
        } else {
            out.push(DiffLine { kind: "add", text: after[j].clone() });
            j += 1;
        }
    }
    out.extend(before[i..].iter().map(|t| DiffLine { kind: "remove", text: t.clone() }));
    out.extend(after[j..].iter().map(|t| DiffLine { kind: "add", text: t.clone() }));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levenshtein_basics() {
        assert_eq!(levenshtein("kitten", "sitting"), 3);
        assert_eq!(levenshtein("", "abc"), 3);
        assert_eq!(levenshtein("same", "same"), 0);
        assert_eq!(levenshtein("héllo", "hello"), 1, "counts characters, not bytes");
    }

    #[test]
    fn word_diff_finds_removed_and_added_runs() {
        let d = word_diff("Great point! I totally agree with this take", "Great point, agree with this");
        assert_eq!(d.removed, vec![vec!["i".to_string(), "totally".into()], vec!["take".to_string()]]);
        assert!(d.added.is_empty());
        let d = word_diff("nice thread", "nice thread, honestly super useful");
        assert_eq!(d.added, vec![vec!["honestly".to_string(), "super".into(), "useful".into()]]);
    }

    #[test]
    fn emoji_and_hashtags_are_counted() {
        assert_eq!(emoji_count("love it \u{1F525}\u{1F680}"), 2);
        assert_eq!(emoji_count("plain text"), 0);
        assert_eq!(hashtag_count("so #rust and #tauri # alone"), 2);
    }

    #[test]
    fn line_diff_marks_changes() {
        let a = vec!["a".to_string(), "b".into(), "c".into()];
        let b = vec!["a".to_string(), "c".into(), "d".into()];
        let d = line_diff(&a, &b);
        let kinds: Vec<_> = d.iter().map(|l| (l.kind, l.text.as_str())).collect();
        assert_eq!(kinds, vec![("same", "a"), ("remove", "b"), ("same", "c"), ("add", "d")]);
    }
}
