// SPDX-License-Identifier: Apache-2.0
//! IO-free extraction vocabulary: facts, precision labels, limits and refusals.
//! Implements PARSERS-v1 (completion, diagnostics, limits) for Java source.
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
    let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
    match value.find(". ") {
        Some(end) => value[..=end].into(),
        None => value,
    }
}

/// First description sentence of a `/** */` comment, skipping tag lines.
pub(crate) fn javadoc_summary(comment: &str) -> Option<String> {
    let body = comment.strip_prefix("/**")?.strip_suffix("*/")?;
    body.lines()
        .map(|line| line.trim().trim_start_matches('*').trim())
        .find(|line| !line.is_empty() && !line.starts_with('@'))
        .map(sentence)
}

/// Collapses whitespace the way signatures are stored.
pub(crate) fn normalize(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
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
    }

    #[test]
    fn javadoc_keeps_the_first_description_sentence() {
        assert_eq!(javadoc_summary("/* plain */"), None);
        assert_eq!(javadoc_summary("/**/"), None);
        assert_eq!(
            javadoc_summary("/**\n * @param x\n * Does it. More.\n */"),
            Some("Does it.".into())
        );
        assert_eq!(normalize("a .\n b"), "a . b");
    }
}
