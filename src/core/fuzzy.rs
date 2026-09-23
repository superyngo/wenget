//! Fuzzy matching for package/script search
//!
//! Scores a search term against a name (and optionally a description/repo)
//! in ranked tiers: glob, exact, prefix, word-boundary substring, substring,
//! subsequence, typo tolerance, then description/repo substring.
//! All comparisons are case-insensitive.

use glob::{MatchOptions, Pattern};

/// Score for an exact (or glob) name match
pub const SCORE_EXACT: u32 = 1000;
const SCORE_PREFIX: u32 = 800;
const SCORE_WORD: u32 = 700;
const SCORE_SUBSTRING: u32 = 600;
const SCORE_SUBSEQUENCE: u32 = 400;
const SCORE_TYPO: u32 = 300;
const SCORE_DESCRIPTION: u32 = 100;

/// Whether a term should be treated as a glob pattern
pub fn is_glob(term: &str) -> bool {
    term.contains(['*', '?', '['])
}

/// Score how well `term` matches `name`, ignoring description.
/// Returns `None` when there is no match.
pub fn score_name(term: &str, name: &str) -> Option<u32> {
    let term = term.to_lowercase();
    let name = name.to_lowercase();
    if term.is_empty() {
        return None;
    }

    if is_glob(&term) {
        let opts = MatchOptions {
            case_sensitive: false,
            ..MatchOptions::new()
        };
        return Pattern::new(&term)
            .ok()
            .filter(|p| p.matches_with(&name, opts))
            .map(|_| SCORE_EXACT);
    }

    let len_penalty = |base: u32| base - (name.chars().count().min(99) as u32);

    if name == term {
        return Some(SCORE_EXACT);
    }
    if name.starts_with(&term) {
        return Some(len_penalty(SCORE_PREFIX));
    }
    if let Some(pos) = name.find(&term) {
        let at_boundary = name[..pos].ends_with(['-', '_', '.', ' ']);
        let base = if at_boundary {
            SCORE_WORD
        } else {
            SCORE_SUBSTRING
        };
        return Some(len_penalty(base));
    }

    let term_len = term.chars().count();
    if term_len >= 3 {
        if let Some(span) = subsequence_span(&term, &name) {
            if span <= term_len * 3 {
                return Some(SCORE_SUBSEQUENCE - (span - term_len) as u32);
            }
        }
    }

    let max_dist = match term_len {
        0..=3 => 0,
        4..=5 => 1,
        _ => 2,
    };
    if max_dist > 0 {
        let name_chars: Vec<char> = name.chars().collect();
        let term_chars: Vec<char> = term.chars().collect();
        let full = osa_distance(&term_chars, &name_chars);
        // Also compare against the name prefix of same length (typo in a prefix query)
        let prefix_len = term_len.min(name_chars.len());
        let prefix = osa_distance(&term_chars, &name_chars[..prefix_len]);
        let dist = full.min(prefix);
        if dist <= max_dist {
            return Some(SCORE_TYPO - dist as u32 * 10);
        }
    }

    None
}

/// Score `term` against a name, falling back to a word-prefix match in description/repo
/// (e.g. `json` hits "JSON processor", but `rip` does not hit "JavaScript").
pub fn score(term: &str, name: &str, description: &str, repo: &str) -> Option<u32> {
    if let Some(s) = score_name(term, name) {
        return Some(s);
    }
    if is_glob(term) || term.chars().count() < 3 {
        return None;
    }
    let term = term.to_lowercase();
    let starts_word = |text: &str| {
        text.to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .any(|w| w.starts_with(&term))
    };
    if starts_word(description) || starts_word(repo) {
        return Some(SCORE_DESCRIPTION);
    }
    None
}

/// Length of the shortest window of `name` containing `term` as a subsequence
fn subsequence_span(term: &str, name: &str) -> Option<usize> {
    let name: Vec<char> = name.chars().collect();
    let term: Vec<char> = term.chars().collect();
    let first = *term.first()?;
    let mut best: Option<usize> = None;
    for start in (0..name.len()).filter(|&i| name[i] == first) {
        let mut idx = start + 1;
        let mut ok = true;
        for &c in &term[1..] {
            match name[idx..].iter().position(|&n| n == c) {
                Some(p) => idx += p + 1,
                None => {
                    ok = false;
                    break;
                }
            }
        }
        if !ok {
            break;
        }
        let span = idx - start;
        best = Some(best.map_or(span, |b| b.min(span)));
    }
    best
}

/// Optimal string alignment (Damerau-Levenshtein with adjacent transpositions)
fn osa_distance(a: &[char], b: &[char]) -> usize {
    let (n, m) = (a.len(), b.len());
    let mut d = vec![vec![0usize; m + 1]; n + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    d[0] = (0..=m).collect();
    for i in 1..=n {
        for j in 1..=m {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut v = (d[i - 1][j] + 1)
                .min(d[i][j - 1] + 1)
                .min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                v = v.min(d[i - 2][j - 2] + 1);
            }
            d[i][j] = v;
        }
    }
    d[n][m]
}

/// Suggest up to `limit` names close to `term`, best first (name-only matching).
pub fn suggest<'a, I>(term: &str, names: I, limit: usize) -> Vec<&'a str>
where
    I: IntoIterator<Item = &'a str>,
{
    let mut scored: Vec<(u32, &str)> = names
        .into_iter()
        .filter_map(|n| score_name(term, n).map(|s| (s, n)))
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));
    scored.dedup_by(|a, b| a.1 == b.1);
    scored.into_iter().take(limit).map(|(_, n)| n).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tier_ordering() {
        let exact = score_name("rg", "rg").unwrap();
        let prefix = score_name("rip", "ripgrep").unwrap();
        let word = score_name("grep", "rip-grep").unwrap();
        let sub = score_name("grep", "ripgrep").unwrap();
        let subseq = score_name("rgp", "ripgrep").unwrap();
        let typo = score_name("ripgerp", "ripgrep").unwrap();
        let desc = score("finder", "fzf", "A command-line fuzzy finder", "").unwrap();
        assert!(exact > prefix && prefix > word && word > sub);
        assert!(sub > subseq && subseq > typo && typo > desc);
    }

    #[test]
    fn test_case_insensitive() {
        assert_eq!(score_name("RIPGREP", "ripgrep"), Some(SCORE_EXACT));
        assert_eq!(score_name("RIP*", "ripgrep"), Some(SCORE_EXACT));
    }

    #[test]
    fn test_glob_anchored() {
        assert!(score_name("*grep*", "ripgrep").is_some());
        assert!(score_name("grep*", "ripgrep").is_none());
    }

    #[test]
    fn test_typo_tolerance() {
        assert!(score_name("ripgerp", "ripgrep").is_some()); // transposition
        assert!(score_name("fzff", "fzf").is_some());
        assert!(score_name("bat", "bot").is_none()); // 3-char terms: no typo tolerance
        assert!(score_name("xyz", "fzf").is_none());
        assert!(score_name("fd", "fz").is_none()); // short terms need exact/substring
    }

    #[test]
    fn test_subsequence_span_limit() {
        assert!(score_name("rgp", "ripgrep").is_some());
        assert!(score_name("abc", "a-very-long-b-name-c").is_none());
    }

    #[test]
    fn test_description_and_repo() {
        assert!(score("json", "jq", "Command-line JSON processor", "").is_some());
        assert!(score(
            "burntsushi",
            "ripgrep",
            "",
            "https://github.com/BurntSushi/ripgrep"
        )
        .is_some());
        assert!(score("js", "jq", "JSON", "").is_none()); // too short
    }

    #[test]
    fn test_suggest() {
        let names = ["ripgrep", "fzf", "fd", "bat"];
        assert_eq!(suggest("ripgrp", names, 3), vec!["ripgrep"]);
        assert!(suggest("zzzz", names, 3).is_empty());
    }
}
