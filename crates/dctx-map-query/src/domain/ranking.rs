//! Integer ranking features layered over SQLite FTS5 BM25.
//! Implements SPEC-MAP-MODE section 6.4 without IO.
//! Used by find after lexical candidates are retrieved from the store.
use super::{Request, inside};
use dctx_map_model::{Node, SummarySource};

fn words(value: &str) -> Vec<String> {
    let chars: Vec<_> = value.chars().collect();
    let mut split = String::new();
    for (i, ch) in chars.iter().copied().enumerate() {
        if !ch.is_alphanumeric() {
            split.push(' ');
            continue;
        }
        if i > 0
            && ch.is_uppercase()
            && (chars[i - 1].is_lowercase()
                || chars[i - 1].is_numeric()
                || (chars[i - 1].is_uppercase()
                    && chars.get(i + 1).is_some_and(|c| c.is_lowercase())))
        {
            split.push(' ');
        }
        split.extend(ch.to_lowercase());
    }
    split.split_whitespace().map(str::to_owned).collect()
}

pub(crate) fn expression(query: &str) -> Option<String> {
    let stop = [
        "a", "an", "the", "is", "are", "of", "to", "in", "for", "and", "or", "how", "what",
        "which", "where", "does", "do", "can", "i", "it", "that", "with",
    ];
    let mut terms: Vec<_> = words(query)
        .into_iter()
        .filter(|w| !stop.contains(&w.as_str()))
        .take(32)
        .collect();
    terms.sort();
    terms.dedup();
    if terms.is_empty() {
        None
    } else {
        Some(
            terms
                .iter()
                .map(|w| match stem(w) {
                    Some(stem) => format!("(\"{w}\" OR \"{stem}\"*)"),
                    None => format!("\"{w}\""),
                })
                .collect::<Vec<_>>()
                .join(" OR "),
        )
    }
}

fn stem(word: &str) -> Option<&str> {
    match word {
        "application" => return Some("app"),
        "variables" | "variable" => return Some("var"),
        _ => {}
    }
    for suffix in ["ation", "ion", "ing", "ed", "es", "s"] {
        if let Some(base) = word.strip_suffix(suffix).filter(|s| s.len() >= 4) {
            return Some(base);
        }
    }
    None
}

#[allow(clippy::cast_possible_truncation, reason = "The fixed-point lexical score is rounded and clamped to 0..=1_000_000_000 before conversion; retain Rust float-to-integer NaN handling.")]
pub(crate) fn rank(node: &Node, bm25: f64, request: &Request, degree: usize, recent: bool) -> i64 {
    let lexical = (-bm25 * 10_000.0).round().clamp(0.0, 1_000_000_000.0) as i64;
    let exact = node
        .name
        .rsplit("::")
        .next()
        .is_some_and(|name| name.eq_ignore_ascii_case(request.target.trim()));
    let doc = matches!(
        node.summary.source,
        SummarySource::Doc | SummarySource::Heading
    ) && !node.summary.text.is_empty();
    lexical
        + i64::from(exact) * 2_000_000
        + i64::from(node.public) * 200_000
        + i64::from(doc) * 30_000
        + i64::from((degree + 1).ilog2()) * 15_000
        + i64::from(request.focus.iter().any(|p| inside(&node.path, p))) * 100_000
        + i64::from(recent) * 5_000
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identifiers_and_user_fts_operators_are_plain_terms() {
        assert_eq!(
            expression("HTTPServer read_file").unwrap(),
            "\"file\" OR \"http\" OR \"read\" OR \"server\""
        );
        assert_eq!(expression("\" OR * ()"), None);
        assert_eq!(expression("where is cache"), Some("\"cache\"".into()));
    }
}
