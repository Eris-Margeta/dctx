// SPDX-License-Identifier: Apache-2.0
//! IO-free extraction vocabulary: facts, precision labels, limits and refusals.
//! Implements PARSERS-v1 (completion, diagnostics, limits) for C# source.
//! Used by the lexer, the structural walker and callers of the public API.
mod facts;
mod limits;

pub use facts::{
    Completion, Diagnostic, DiagnosticReason, Dialect, Labeled, LexedFile, Precision, RawReference,
};
pub use limits::{LexError, Limits};

use dctx_hash::Sha256;

/// SHA-256 hex of the body with whitespace runs collapsed and edges trimmed.
#[allow(
    clippy::must_use_candidate,
    reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
)]
pub fn body_fingerprint(body: &str) -> String {
    // A long ASCII token or comment often needs no normalization. Preserve
    // Unicode whitespace semantics on the general path, including vertical tab.
    let normalized = if body.is_ascii()
        && ![' ', '\t', '\n', '\r', '\x0b', '\x0c']
            .into_iter()
            .any(|ch| body.contains(ch))
    {
        std::borrow::Cow::Borrowed(body)
    } else {
        std::borrow::Cow::Owned(body.split_whitespace().collect::<Vec<_>>().join(" "))
    };
    let mut hex = String::with_capacity(64);
    if let Ok(digest) = Sha256::digest(normalized.as_bytes()) {
        for byte in digest.as_bytes() {
            hex.push(char::from(HEX[usize::from(byte >> 4)]));
            hex.push(char::from(HEX[usize::from(byte & 15)]));
        }
    }
    hex
}

const HEX: &[u8; 16] = b"0123456789abcdef";

/// Whitespace-normalized text up to and including the first `". "`.
pub(crate) fn sentence(value: &str) -> String {
    let value = normalize(value);
    match value.find(". ") {
        Some(end) => value[..=end].to_string(),
        None => value,
    }
}

/// Collapses whitespace the way signatures are stored.
pub(crate) fn normalize(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Byte slice that is empty when the range is not a char boundary.
pub(crate) fn slice(src: &str, start: u32, end: u32) -> &str {
    let (start, end) = (start as usize, end as usize);
    if start <= end && end <= src.len() && src.is_char_boundary(start) && src.is_char_boundary(end)
    {
        &src[start..end]
    } else {
        ""
    }
}

/// First `<summary>` sentence of an XML doc comment, otherwise the first plain line.
pub(crate) fn xml_summary(parts: &[String]) -> Option<String> {
    let body = parts.join(" ");
    let text = summary_inner(&body).unwrap_or_else(|| plain_doc(&body));
    let text: String = sentence(&strip_tags(&text)).chars().take(100).collect();
    (!text.is_empty()).then_some(text)
}

/// Text of one `///` or `/** */` comment, markers removed.
pub(crate) fn doc_body(text: &str) -> String {
    if let Some(rest) = text.strip_prefix("///") {
        return rest.trim().to_string();
    }
    if let Some(inner) = text
        .strip_prefix("/*")
        .and_then(|body| body.strip_suffix("*/"))
    {
        return inner
            .lines()
            .map(|line| line.trim().trim_start_matches('*').trim())
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
    }
    text.trim().to_string()
}

fn summary_inner(body: &str) -> Option<String> {
    let start = body.find("<summary")?;
    let open = start + "<summary".len();
    let close = body[open..].find('>')? + open + 1;
    let end = body[close..].find("</summary>")? + close;
    Some(body[close..end].to_string())
}

fn plain_doc(body: &str) -> String {
    body.lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('<') && !line.starts_with('@'))
        .unwrap_or("")
        .to_string()
}

fn strip_tags(value: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for ch in value.chars() {
        match ch {
            '<' => inside = true,
            '>' => inside = false,
            _ if !inside => out.push(ch),
            _ => {}
        }
    }
    out
}

/// At most one newline, and no other non-space, between a doc comment and its declaration.
pub(crate) fn gap_ok(src: &str, from: u32, to: u32) -> bool {
    if from > to {
        return false;
    }
    let Some(gap) = src.get(from as usize..to as usize) else {
        return false;
    };
    gap.bytes().filter(|&byte| byte == b'\n').count() <= 1
        && gap
            .bytes()
            .all(|byte| matches!(byte, b' ' | b'\t' | b'\r' | b'\n'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprints_are_known_sha256_and_normalize_whitespace() {
        assert_eq!(
            body_fingerprint("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(body_fingerprint(" a\n\t b "), body_fingerprint("a b"));
        assert_ne!(body_fingerprint("a b"), body_fingerprint("a c"));
        assert_eq!(body_fingerprint("").len(), 64);
    }

    #[test]
    fn xml_summary_keeps_the_first_summary_sentence() {
        let parts = vec![
            "<summary>".into(),
            "A box of values. More.".into(),
            "</summary>".into(),
        ];
        assert_eq!(xml_summary(&parts).as_deref(), Some("A box of values."));
        assert_eq!(xml_summary(&["<param name=\"x\">no</param>".into()]), None);
        assert_eq!(normalize("a .\n b"), "a . b");
        assert!(gap_ok(" \n ", 0, 3));
        assert!(!gap_ok(" \n\n ", 0, 4));
    }
}
