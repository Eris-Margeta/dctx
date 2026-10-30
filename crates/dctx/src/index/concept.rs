//! Multi-word concept matching for find: weighted word overlap on top of the lexical rank.
//! A query of two or more content words keeps a candidate only when at least half of them match.
//! Fewer than two content words never reach this module's filter, so single-word find stays as it was.

/// Words that carry no meaning in a concept query.
const STOP: [&str; 24] = [
    "a", "an", "the", "is", "are", "of", "to", "in", "on", "for", "and", "or", "how", "what",
    "which", "where", "does", "do", "can", "i", "it", "that", "with", "by",
];

/// Score per weighted point; a three-word match scores 300 to 900, against 3000 for an exact name.
const POINT: i64 = 100;

/// Lowercase words of `value`, split at non-alphanumerics and at camelCase boundaries (`HTTPServer` is `http`, `server`).
fn words(value: &str) -> Vec<String> {
    let chars: Vec<char> = value.chars().collect();
    let mut split = String::with_capacity(value.len() + 8);
    for (i, ch) in chars.iter().copied().enumerate() {
        if !ch.is_alphanumeric() {
            split.push(' ');
            continue;
        }
        let prev = i.checked_sub(1).and_then(|p| chars.get(p)).copied();
        let next_lower = chars.get(i + 1).is_some_and(|c| c.is_lowercase());
        if ch.is_uppercase() && prev.is_some_and(|p| p.is_lowercase() || p.is_numeric() || (p.is_uppercase() && next_lower)) {
            split.push(' ');
        }
        split.extend(ch.to_lowercase());
    }
    split.split_whitespace().map(str::to_owned).collect()
}

/// Content words of a query: lowercase, split like identifiers, stop words dropped, duplicates removed, order kept.
/// A query without whitespace (one token, however it is spelled) has none, so it keeps the legacy substring scoring.
pub(crate) fn terms(query: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if query.split_whitespace().nth(1).is_none() {
        return out;
    }
    for word in words(query).into_iter().take(32) {
        if !STOP.contains(&word.as_str()) && !out.contains(&word) {
            out.push(word);
        }
    }
    out
}

/// A word and its simple stems: trailing `s`, `es`, `ed`, `ing` stripped (with `e` restored), `ies` and `ied` as `y`.
fn forms(word: &str) -> Vec<String> {
    let mut out = vec![word.to_owned()];
    for suffix in ["ies", "ied"] {
        if let Some(base) = word.strip_suffix(suffix).filter(|b| b.len() >= 2) {
            out.push(format!("{base}y"));
        }
    }
    for (suffix, least) in [("ing", 4), ("ed", 4), ("es", 3), ("s", 3)] {
        let Some(base) = word.strip_suffix(suffix).filter(|b| b.len() >= least) else {
            continue;
        };
        if suffix == "s" && base.ends_with('s') {
            continue;
        }
        out.push(base.to_owned());
        if suffix != "s" {
            out.push(format!("{base}e"));
        }
    }
    out
}

/// True when `term` equals `candidate` or shares a stem with it.
fn same(term: &[String], candidate: &str) -> bool {
    let other = forms(candidate);
    term.iter().any(|t| other.contains(t))
}

/// Weighted overlap of the query `terms` with a candidate, or `None` when fewer than half of them match.
/// A term counts 3 when it appears in the name, else 2 in the summary, else 1 in the path.
/// With fewer than two terms the candidate is never filtered and the score is 0.
pub(crate) fn score(terms: &[String], name: &str, summary: &str, path: &str) -> Option<i64> {
    if terms.len() < 2 {
        return Some(0);
    }
    let tiers = [(3, words(name)), (2, words(summary)), (1, words(path))];
    let mut matched = 0;
    let mut points = 0_i64;
    for term in terms {
        let term = forms(term);
        if let Some((weight, _)) = tiers
            .iter()
            .find(|(_, tier)| tier.iter().any(|word| same(&term, word)))
        {
            matched += 1;
            points += weight;
        }
    }
    (matched * 2 >= terms.len()).then_some(points * POINT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stop_words_and_single_terms_leave_find_alone() {
        assert_eq!(terms("where is the cache of it"), ["cache"]);
        assert_eq!(score(&terms("cache"), "x", "", "y.rs"), Some(0));
        assert!(terms("parse_config").is_empty() && terms("HTTPServer").is_empty() && terms("src/lib").is_empty());
        assert_eq!(terms("read settings file"), ["read", "settings", "file"]);
    }

    #[test]
    fn concept_words_match_by_stem_and_weigh_name_over_summary_over_path() {
        let query = terms("read settings file");
        let found = score(&query, "parse_config_file", "Reads the settings file from disk.", "loader.py");
        assert_eq!(found, Some((3 + 2 + 2) * POINT));
        let poor = score(&query, "unrelated", "Something else.", "config/settings.py");
        assert_eq!(poor, None);
        let half = score(&terms("parse config"), "parse_config_file", "", "");
        assert_eq!(half, Some(6 * POINT));
        assert_eq!(score(&terms("parse config"), "unrelated", "", ""), None);
        assert_eq!(score(&terms("retry backoff"), "retry-policy", "Requests are retried.", "docs/guide.md"), Some(3 * POINT));
    }
}
