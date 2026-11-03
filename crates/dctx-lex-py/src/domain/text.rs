// SPDX-License-Identifier: Apache-2.0
//! Docstring decoding, comment summaries and body fingerprints.
//! Implements SPEC-MAP-MODE 5.2 (fingerprints) and 5.4 (summaries) for Python.
//! Uses dctx-hash SHA-256; the walker calls these when it labels a fact.
use dctx_hash::Sha256;

const HEX: &[u8; 16] = b"0123456789abcdef";

/// SHA-256 hex of the body with whitespace runs collapsed and edges trimmed.
#[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
pub fn body_fingerprint(body: &str) -> String {
    let normalized = collapse(body);
    let mut hex = String::with_capacity(64);
    if let Ok(digest) = Sha256::digest(normalized.as_bytes()) {
        for byte in digest.as_bytes() {
            hex.push(char::from(HEX[usize::from(byte >> 4)]));
            hex.push(char::from(HEX[usize::from(byte & 15)]));
        }
    }
    hex
}

pub(crate) fn collapse(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Whitespace-normalized text up to and including the first `". "`.
pub(crate) fn sentence(value: &str) -> String {
    let value = collapse(value);
    match value.find(". ") {
        Some(end) => value[..=end].to_owned(),
        None => value,
    }
}

pub(crate) fn comment_summary(source: &str, comments: &[(u32, u32)]) -> String {
    for &(start, end) in comments {
        let raw = source.get(start as usize..end as usize).unwrap_or("");
        let body = raw.trim().trim_start_matches('#').trim();
        if body.is_empty() || body.starts_with('!') {
            continue;
        }
        return sentence(body);
    }
    String::new()
}

/// Decodes one docstring token, quotes and recognised escapes removed.
pub(crate) fn decode_doc(source: &str, start: u32, end: u32) -> Option<String> {
    let raw = source.get(start as usize..end as usize)?;
    let rest = strip_string_prefix(raw);
    let (_, triple) = quote_kind(rest)?;
    let qlen = if triple { 3 } else { 1 };
    if rest.len() < qlen * 2 {
        return None;
    }
    let inner = &rest[qlen..rest.len() - qlen];
    let prefix = &raw[..raw.len() - rest.len()];
    let raw_prefix = prefix.bytes().any(|b| b == b'r' || b == b'R');
    let text = if raw_prefix {
        inner.to_owned()
    } else {
        unescape(inner)
    };
    Some(text)
}

fn strip_string_prefix(raw: &str) -> &str {
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_alphabetic() && i < 2 {
        i += 1;
    }
    raw.get(i..).unwrap_or(raw)
}

fn quote_kind(rest: &str) -> Option<(char, bool)> {
    let mut chars = rest.chars();
    let quote = chars.next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let triple = rest.starts_with(&format!("{quote}{quote}{quote}"));
    Some((quote, triple))
}

fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('\n') => {}
            Some('\r') => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
            }
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') | None => out.push('\\'),
            Some('\'') => out.push('\''),
            Some('"') => out.push('"'),
            Some('a') => out.push('\u{7}'),
            Some('b') => out.push('\u{8}'),
            Some('f') => out.push('\u{c}'),
            Some('v') => out.push('\u{b}'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_is_the_known_sha256_of_collapsed_text() {
        assert_eq!(
            body_fingerprint("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(body_fingerprint(" a\n\t b "), body_fingerprint("a b"));
    }
}
