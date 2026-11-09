// SPDX-License-Identifier: Apache-2.0
//! Search terms of `dctx pack --for`: the names a task text mentions, in the order the text mentions them.
//! Code-like words come first (a single backticked word, snake_case, camelCase, PascalCase with an inner capital,
//! the last segment of `a::b`, file paths), then plain words of five or more ASCII letters that are not common card English.
//! Plain words keep their spelling, so `Session` finds the `Session` type before `session` functions.
//! A text without backticks and without any code-like word (`main`, `fn main`, `Session`) also keeps the shorter
//! plain words that are not common English or a keyword: such a text is a bare name, not prose.
//! Pure: no index, no IO. Used by the parent module, which runs `map find` on each term.

/// Most terms one task text yields.
pub(super) const MAX_TERMS: usize = 8;
/// Longest term kept; a longer word is not a name.
const MAX_LEN: usize = 64;
/// Shortest plain (not code-like) word kept when the text names something code-like.
const MIN_PLAIN: usize = 5;

/// Plain words that say what to do, not what to touch. Lower case.
const STOP: &[&str] = &[
    "about", "above", "after", "again", "against", "always", "before", "being", "below", "between",
    "build", "change", "changes", "check", "could", "doing", "during", "every", "exactly",
    "exists", "first", "follow", "goal", "gates", "never", "other", "output", "print", "prints",
    "should", "their", "there", "these", "those", "through", "under", "until", "using", "where",
    "which", "while", "within", "without", "would", "write", "written",
];

/// Common English words shorter than [`MIN_PLAIN`] letters. Lower case.
const SHORT_STOP: &[&str] = &[
    "add", "all", "also", "am", "an", "and", "any", "are", "as", "at", "be", "been", "both", "but",
    "by", "can", "did", "do", "does", "done", "each", "else", "fix", "for", "from", "had", "has",
    "have", "here", "how", "if", "in", "into", "is", "it", "its", "just", "keep", "like", "make",
    "many", "may", "me", "more", "most", "much", "must", "my", "need", "no", "not", "now", "of",
    "on", "once", "one", "only", "onto", "or", "our", "out", "over", "own", "same", "see", "so",
    "some", "such", "than", "that", "the", "them", "then", "they", "this", "to", "too", "two",
    "up", "us", "very", "via", "want", "was", "way", "we", "were", "what", "when", "who", "why",
    "will", "with", "work", "yes", "you", "your",
];

/// Short keywords that introduce a name (`fn main`) instead of being one. Lower case.
const KEYWORDS: &[&str] = &[
    "def", "enum", "fn", "fun", "func", "impl", "let", "mod", "pub", "type", "use", "var",
];

/// The search terms of one task text.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Terms {
    /// Deduplicated terms, code-like first, at most [`MAX_TERMS`].
    pub(super) list: Vec<String>,
    /// How many of `list` are code-like (the first ones).
    pub(super) code: usize,
    /// Name-shaped words dropped as common English.
    pub(super) common: usize,
}

impl Terms {
    /// True when the text reads as prose: no code-like name, at least one plain term, and common words around it.
    pub(super) fn prose_only(&self) -> bool {
        self.code == 0 && !self.list.is_empty() && self.common > 0
    }
}

/// The search terms of `text`, deduplicated, code-like first, at most [`MAX_TERMS`].
pub(super) fn extract(text: &str) -> Terms {
    let mut code: Vec<String> = Vec::new();
    // Plain words in text order, each with "shorter than MIN_PLAIN".
    let mut plain: Vec<(String, bool)> = Vec::new();
    let mut common = 0;
    // Odd pieces of a split on backticks are the backticked spans.
    for span in text.split('`').skip(1).step_by(2) {
        let inside = words(span);
        if let [(word, _)] = inside.as_slice()
            && is_name(word)
        {
            push_unique(&mut code, word);
        }
    }
    for (word, pathed) in words(text) {
        if !is_name(&word) {
            continue;
        }
        let lower = word.to_ascii_lowercase();
        if pathed || is_code_like(&word) {
            push_unique(&mut code, &word);
        } else if !word.chars().all(|ch| ch.is_ascii_alphabetic()) {
            // Numbers, versions and mixed words such as `v2` are not plain names.
            continue;
        } else if STOP.contains(&lower.as_str()) || SHORT_STOP.contains(&lower.as_str()) {
            common += 1;
        } else if !KEYWORDS.contains(&lower.as_str())
            && !plain
                .iter()
                .any(|(seen, _)| seen.eq_ignore_ascii_case(&word))
        {
            plain.push((word, lower.len() < MIN_PLAIN));
        }
    }
    // A short plain word is a name only in a text that marks nothing as code.
    let bare = code.is_empty() && !text.contains('`');
    let mut list = code;
    let code = list.len().min(MAX_TERMS);
    for (word, short) in plain {
        if (bare || !short) && !list.iter().any(|name| name.eq_ignore_ascii_case(&word)) {
            list.push(word);
        }
    }
    list.truncate(MAX_TERMS);
    Terms { list, code, common }
}

/// Keep file names intact, including short stems such as `lib.rs`. Other words
/// split at punctuation; a Rust `::` path yields its last segment. A dctx id or a URL
/// (a single `:` inside the word, such as `s:a.rs`) yields nothing.
fn words(text: &str) -> Vec<(String, bool)> {
    text.split(|ch: char| !(ch.is_ascii_alphanumeric() || "_:/.-".contains(ch)))
        .flat_map(|run| {
            let run = run.trim_end_matches('.').trim_start_matches("./");
            if run.trim_end_matches(':').replace("::", "").contains(':') {
                return Vec::new();
            }
            let filename = run.rsplit('/').next().unwrap_or(run);
            if !run.contains(':')
                && filename.rsplit_once('.').is_some_and(|(stem, ext)| {
                    !stem.is_empty()
                        && !ext.is_empty()
                        && ext.chars().all(|ch| ch.is_ascii_alphanumeric())
                })
            {
                vec![(run.to_string(), true)]
            } else {
                symbol_words(run)
            }
        })
        .collect()
}

fn symbol_words(text: &str) -> Vec<(String, bool)> {
    text.split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_' || ch == ':'))
        .filter_map(|run| {
            let pathed = run.contains("::");
            let last = run
                .rsplit("::")
                .find(|part| !part.trim_matches(':').is_empty())?;
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
    let inner_capital = bytes
        .windows(2)
        .any(|pair| pair[0].is_ascii_lowercase() && pair[1].is_ascii_uppercase());
    inner_underscore || inner_capital
}

fn push_unique(list: &mut Vec<String>, word: &str) {
    if !list.iter().any(|seen| seen == word) {
        list.push(word.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(text: &str) -> Vec<String> {
        extract(text).list
    }

    #[test]
    fn code_like_names_come_first_in_text_order_then_plain_words() {
        let terms = extract(
            "Fix `alpha_body` so BetaStore reads the cache; see beta_helper and crate::pack::run.",
        );
        assert_eq!(
            terms.list,
            [
                "alpha_body",
                "BetaStore",
                "beta_helper",
                "run",
                "reads",
                "cache"
            ]
        );
        assert_eq!(terms.code, 4);
        assert!(!terms.prose_only());
    }

    #[test]
    fn a_single_backticked_word_is_a_name_even_when_plain() {
        assert_eq!(list("wire `pack` into the CLI"), ["pack"]);
        assert_eq!(list("run `dctx pack --json` now"), Vec::<String>::new());
    }

    #[test]
    fn stop_words_short_words_numbers_and_duplicates_are_dropped() {
        assert!(list("Goal: never print before the check, 3000 and 42 a b").is_empty());
        assert_eq!(
            list("alpha_body alpha_body ALPHA_BODY"),
            ["alpha_body", "ALPHA_BODY"]
        );
        assert_eq!(
            list("DCTX_MAX_SOURCE_BYTES and parseArgs"),
            ["DCTX_MAX_SOURCE_BYTES", "parseArgs"]
        );
        // Beside a code-like name, a short plain word is card English.
        assert_eq!(list("fix alpha_body in main"), ["alpha_body"]);
    }

    #[test]
    fn bare_short_names_are_kept_when_nothing_is_code_like() {
        assert_eq!(list("main"), ["main"]);
        assert_eq!(list("run"), ["run"]);
        assert_eq!(list("test"), ["test"]);
        assert_eq!(list("fn main"), ["main"]);
        assert_eq!(list("main loop and session"), ["main", "loop", "session"]);
        let bare = extract("main");
        assert_eq!(bare.code, 0);
        assert!(!bare.prose_only(), "a bare name is not prose");
    }

    #[test]
    fn plain_words_keep_their_spelling() {
        assert_eq!(list("Session"), ["Session"]);
        assert_eq!(list("Cache cache CACHE"), ["Cache"]);
        assert_eq!(
            list("fix `alpha_body` in Session"),
            ["alpha_body", "Session"]
        );
    }

    #[test]
    fn a_sentence_without_identifiers_is_prose() {
        let terms = extract("how does caching work");
        assert_eq!(terms.list, ["caching"]);
        assert!(terms.prose_only());
        assert!(!extract("how does `cache_get` work").prose_only());
        let empty = extract("how does it work");
        assert!(empty.list.is_empty());
        assert!(!empty.prose_only(), "no terms is reported as no terms");
    }

    #[test]
    fn paths_stay_whole_and_ids_are_not_terms() {
        assert_eq!(
            list("edit crates/dctx-cli/src/pack_for.rs"),
            ["crates/dctx-cli/src/pack_for.rs"]
        );
        assert!(list("s:a.rs").is_empty());
        assert!(list("d:docs/guides/README.md").is_empty());
    }

    #[test]
    fn file_targets_keep_the_filename_including_short_stems() {
        assert_eq!(list("src/lib.rs"), ["src/lib.rs"]);
        assert_eq!(list("lib.rs"), ["lib.rs"]);
    }

    #[test]
    fn at_most_eight_terms() {
        let text = (0..20)
            .map(|n| format!("name_{n}"))
            .collect::<Vec<_>>()
            .join(" ");
        let terms = extract(&text);
        assert_eq!(terms.list.len(), MAX_TERMS);
        assert_eq!(terms.code, MAX_TERMS);
        assert_eq!(terms.list[0], "name_0");
        assert!(list(&"x".repeat(MAX_LEN + 1)).is_empty());
    }
}
