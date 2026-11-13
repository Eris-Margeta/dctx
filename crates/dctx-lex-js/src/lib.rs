// SPDX-License-Identifier: Apache-2.0
//! Hand-written JavaScript and TypeScript structural extractor.
//! Implements decision 077, PARSERS-v1 (JavaScript/TypeScript rows) and SPEC-MAP-MODE 5–6.2.
//! Depends on dctx-map-model facts and dctx-hash; never on tree-sitter.
//! Emits the dctx-map-parse fact shape with a precision label on every fact.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod domain;
mod extract;
mod lexer;

pub use domain::{
    Completion, Diagnostic, DiagnosticReason, Dialect, Labeled, LexError, LexedFile, Limits,
    Precision, RawReference, body_fingerprint,
};

/// Parser identity recorded with every fact key (PARSERS-v1 provenance).
pub const PARSER_ID: &str = "dctx-lex-js";
/// Bumped whenever extraction output can change for the same bytes.
pub const PARSER_REVISION: u32 = 2;

/// Bounded structural extractor; stateless apart from its limits.
///
/// Share one instance across threads or reuse it across files: extraction keeps
/// no mutable per-file state in the extractor.
#[derive(Debug, Clone, Copy, Default)]
pub struct JsLexer {
    limits: Limits,
}

impl JsLexer {
    /// Builds an extractor; limits above the ceilings are narrowed to them.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn new(limits: Limits) -> Self {
        Self {
            limits: limits.narrowed(),
        }
    }

    /// Extracts facts from one place-relative path and its authorized bytes.
    ///
    /// The path is an identity only; this method performs no filesystem IO.
    /// Bytes must be UTF-8. Limits are inclusive, and the fact budget includes
    /// the module node even for empty input. Unsupported syntax returns partial
    /// facts with diagnostics; a refusal returns no facts. Output preserves source
    /// traversal order, including distinct identities for duplicate declarations.
    ///
    /// # Errors
    /// Refuses unsupported paths, invalid UTF-8 or model identities, and
    /// input exceeding the byte, nesting or fact ceilings.
    pub fn extract(&self, path: &str, bytes: &[u8]) -> Result<LexedFile, LexError> {
        let dialect = Dialect::for_path(path).ok_or(LexError::UnsupportedDialect)?;
        if bytes.len() > self.limits.max_bytes {
            return Err(LexError::TooLarge {
                bytes: bytes.len(),
                limit: self.limits.max_bytes,
            });
        }
        let source = std::str::from_utf8(bytes)?;
        let lexed = lexer::lex(source, dialect, &self.limits)?;
        extract::extract(path, source, dialect, &lexed, &self.limits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refusals_are_typed_with_positive_controls() {
        let lexer = JsLexer::default();
        assert!(matches!(
            lexer.extract("x.py", b""),
            Err(LexError::UnsupportedDialect)
        ));
        assert!(matches!(
            lexer.extract("x.js", &[0xff]),
            Err(LexError::InvalidUtf8(_))
        ));
        assert!(matches!(
            lexer.extract("../x.js", b""),
            Err(LexError::InvalidId(_))
        ));
        let small = JsLexer::new(Limits {
            max_bytes: 4,
            ..Limits::default()
        });
        assert!(matches!(
            small.extract("x.js", b"run();"),
            Err(LexError::TooLarge { bytes: 6, limit: 4 })
        ));
        assert!(small.extract("x.js", b"f();").is_ok());
        assert!(lexer.extract("x.js", b"").is_ok());
    }

    #[test]
    fn limits_never_widen_past_the_ceiling() {
        let wide = JsLexer::new(Limits {
            max_bytes: usize::MAX,
            max_depth: u32::MAX,
            max_facts: usize::MAX,
        });
        assert_eq!(wide.limits, Limits::CEILING);
    }
}
