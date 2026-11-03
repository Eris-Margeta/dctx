// SPDX-License-Identifier: Apache-2.0
//! Search terms of `dctx pack --for`: the names a task text mentions, in the order the text mentions them.
//! Code-like words come first (a single backticked word, snake_case, camelCase, PascalCase with an inner capital,
//! the last segment of `a::b`), then plain words of five or more ASCII letters that are not common card English.
//! Pure: no index, no IO. Used by the parent module, which runs `map find` on each term.

/// Most terms one task text yields.
pub(super) const MAX_TERMS: usize = 8;
/// Longest term kept; a longer word is not a name.
const MAX_LEN: usize = 64;
/// Shortest plain (not code-like) word kept.
const MIN_PLAIN: usize = 5;

/// Plain words that say what to do, not what to touch. Lower case.
const STOP: &[&str] = &[
    "about", "above", "after", "again", "against", "always", "before", "being", "below", "between", "build", "change",
    "changes", "check", "could", "doing", "during", "every", "exactly", "exists", "first", "follow", "goal", "gates",
    "never", "other", "output", "print", "prints", "should", "their", "there", "these", "those", "through", "under",
    "until", "using", "where", "which", "while", "within", "without", "would", "write", "written",
];

/// The search terms of `text`, deduplicated, code-like first, at most [`MAX_TERMS`].
pub(super) fn extract(text: &str) -> Vec<String> {
    let mut code: Vec<String> = Vec::new();
    let mut plain: Vec<String> = Vec::new();
    // Odd pieces of a split on backticks are the backticked spans.
    for span in text.split('`').skip(1).step_by(2) {
        let inside = words(span);
        if let [(word, _)] = inside.as_slice() {
            if is_name(word) {
                push_unique(&mut code, word);
            }
        }
    }
    for (word, pathed) in words(text) {
        if !is_name(&word) {
            continue;
        }
        if pathed || is_code_like(&word) {
            push_unique(&mut code, &word);
        } else if is_plain(&word) {
            push_unique(&mut plain, &word.to_ascii_lowercase());
        }
    }
    plain.retain(|word| !code.iter().any(|name| name.eq_ignore_ascii_case(word)));
    code.extend(plain);
    code.truncate(MAX_TERMS);
    code
}

/// Words of `text`: runs of ASCII letters, digits, `_` and `:`. A run holding `::` yields its last segment, flagged as pathed.
fn words(text: &str) -> Vec<(String, bool)> {
    text.split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_' || ch == ':'))
        .filter_map(|run| {
            let pathed = run.contains("::");
            let last = run.rsplit("::").find(|part| !part.trim_matches(':').is_empty())?;
            let word = last.trim_matches(':');
            (!word.contains(':')).then(|| (word.to_string(), pathed))
        })
        .collect()
}

/// A word that can name a symbol: 2..=[`MAX_LEN`] bytes, starting with a letter or `_`, holding a letter.
fn is_name(word: &str) -> bool {
    let first = word.chars().next();
    (2..=MAX_LEN).contains(&word.len())
        && first.is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '_')
        && word.chars().any(|ch| ch.is_ascii_alphabetic())
}

/// snake_case or SCREAMING_CASE (an inner `_`), or camelCase / PascalCase (a lower-case letter followed by a capital).
fn is_code_like(word: &str) -> bool {
    let inner_underscore = word.trim_matches('_').contains('_');
    let bytes = word.as_bytes();
    let inner_capital = bytes.windows(2).any(|pair| pair[0].is_ascii_lowercase() && pair[1].is_ascii_uppercase());
    inner_underscore || inner_capital
}

/// A plain word worth a find: at least [`MIN_PLAIN`] ASCII letters and not in [`STOP`].
fn is_plain(word: &str) -> bool {
    word.len() >= MIN_PLAIN
        && word.chars().all(|ch| ch.is_ascii_alphabetic())
        && !STOP.contains(&word.to_ascii_lowercase().as_str())
}

fn push_unique(list: &mut Vec<String>, word: &str) {
    if !list.iter().any(|seen| seen == word) {
        list.push(word.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_like_names_come_first_in_text_order_then_plain_words() {
        let terms = extract("Fix `alpha_body` so BetaStore reads the cache; see beta_helper and crate::pack::run.");
        assert_eq!(terms, ["alpha_body", "BetaStore", "beta_helper", "run", "reads", "cache"]);
    }

    #[test]
    fn a_single_backticked_word_is_a_name_even_when_plain() {
        assert_eq!(extract("wire `pack` into the CLI"), ["pack"]);
        assert_eq!(extract("run `dctx pack --json` now"), Vec::<String>::new());
    }

    #[test]
    fn stop_words_short_words_numbers_and_duplicates_are_dropped() {
        assert!(extract("Goal: never print before the check, 3000 and 42 a b").is_empty());
        assert_eq!(extract("alpha_body alpha_body ALPHA_BODY"), ["alpha_body", "ALPHA_BODY"]);
        assert_eq!(extract("Cache cache CACHE"), ["cache"]);
        assert_eq!(extract("DCTX_MAX_SOURCE_BYTES and parseArgs"), ["DCTX_MAX_SOURCE_BYTES", "parseArgs"]);
    }

    #[test]
    fn paths_split_into_words_and_ids_are_not_terms() {
        assert_eq!(extract("edit crates/dctx-cli/src/pack_for.rs"), ["pack_for", "crates"]);
        assert!(extract("s:a.rs").is_empty());
    }

    #[test]
    fn at_most_eight_terms() {
        let text = (0..20).map(|n| format!("name_{n}")).collect::<Vec<_>>().join(" ");
        let terms = extract(&text);
        assert_eq!(terms.len(), MAX_TERMS);
        assert_eq!(terms[0], "name_0");
        assert!(extract(&"x".repeat(MAX_LEN + 1)).is_empty());
    }
}
