// SPDX-License-Identifier: Apache-2.0
//! Hand-written, zero-dependency Markdown structure extractor for dctx map mode.
//! Implements decision 077 and ARCHITECTURE-PROPOSAL §3 (first-party structural extractors).
//! Depends on std only; dctx-map-parse turns a `Document` into model facts.
//!
//! Extracted: ATX and setext headings with levels, byte spans and the heading tree;
//! fenced and indented code blocks (never scanned for headings or references); front
//! matter (skipped); code spans and inline link destinations as raw references.
//! Unsupported scopes, where the extractor abstains instead of guessing: reference-style
//! links and link reference definitions, inline raw HTML, and partial-tab columns.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod domain;
mod lexer;

pub use domain::{
    CodeBlock, CodeBlockKind, Document, Heading, HeadingStyle, LexError, Limits, Reference,
    ReferenceKind, Span,
};

/// Extracts the Markdown structure of `bytes` with the default [`Limits`].
///
/// Empty input returns an empty document with one line. Truncated Markdown is
/// accepted as ordinary text or an unfinished block. Extraction is stateless and
/// safe to call concurrently; identical bytes return identical source-ordered facts.
///
/// # Errors
/// Returns a typed refusal for input over the byte or container-nesting limits,
/// or for invalid UTF-8. Reduce the input size or container nesting, or re-encode
/// the input as UTF-8, then retry.
pub fn extract(bytes: &[u8]) -> Result<Document, LexError> {
    extract_with(bytes, &Limits::default())
}

/// Extracts the Markdown structure of `bytes`, refusing input beyond `limits`.
///
/// The byte limit is clamped to `u32::MAX - 1`; other limits are used as supplied.
/// The byte limit is checked before UTF-8 decoding. Exceeding `max_brackets` makes
/// additional open brackets literal text instead of refusing the document.
/// Like [`extract`], this function is stateless and safe to call concurrently.
///
/// # Errors
/// Returns a typed refusal for input over the byte or container-nesting limits,
/// or for invalid UTF-8. Reduce the input size or container nesting, or re-encode
/// the input as UTF-8, then retry.
pub fn extract_with(bytes: &[u8], limits: &Limits) -> Result<Document, LexError> {
    let max = limits.max_bytes.min(u32::MAX as usize - 1);
    if bytes.len() > max {
        return Err(LexError::TooLarge {
            len: bytes.len(),
            max,
        });
    }
    let source = std::str::from_utf8(bytes).map_err(|error| LexError::InvalidUtf8 {
        valid_up_to: error.valid_up_to(),
    })?;
    lexer::document(source, limits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oversized_input_is_refused_with_a_positive_control() {
        let limits = Limits {
            max_bytes: 8,
            ..Limits::default()
        };
        assert_eq!(
            extract_with(b"# Too long", &limits),
            Err(LexError::TooLarge { len: 10, max: 8 })
        );
        assert!(extract_with(b"# Short", &limits).is_ok());
    }

    #[test]
    fn invalid_utf8_is_refused_with_a_positive_control() {
        assert_eq!(
            extract(b"# A\xff"),
            Err(LexError::InvalidUtf8 { valid_up_to: 3 })
        );
        assert_eq!(extract("# Ä".as_bytes()).unwrap().headings[0].title, "Ä");
    }
}
