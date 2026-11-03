// SPDX-License-Identifier: Apache-2.0
//! Hand-written, zero-dependency Go structural extractor.
//! Implements decision 077, PARSERS-v1 and SPEC-MAP-MODE 5–6.2 for Go source.
//! Depends on dctx-map-model facts and dctx-hash; never on tree-sitter or git.
//! Emits the dctx-map-parse fact shape with a precision label on every fact.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod domain;
mod extract;
mod lexer;

pub use domain::{
    body_fingerprint, Completion, Diagnostic, DiagnosticReason, Dialect, Labeled, LexError,
    LexedFile, Limits, Precision, RawReference,
};

/// Parser identity recorded with every fact key (PARSERS-v1 provenance).
pub const PARSER_ID: &str = "dctx-lex-go";
/// Bumped whenever extraction output can change for the same bytes.
pub const PARSER_REVISION: u32 = 1;

/// Bounded structural extractor. Stateless apart from its limits.
#[derive(Debug, Clone, Copy, Default)]
pub struct GoLexer {
    limits: Limits,
}

impl GoLexer {
    /// Builds an extractor. Limits above the ceilings are narrowed to them.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn new(limits: Limits) -> Self {
        Self {
            limits: limits.narrowed(),
        }
    }

    /// Extracts facts from one place-relative path and its authorized bytes.
    ///
    /// # Errors
    /// Refuses unsupported paths, invalid UTF-8 or model identities, and
    /// input exceeding the byte, nesting or fact ceilings.
    pub fn extract(&self, path: &str, bytes: &[u8]) -> Result<LexedFile, LexError> {
        if Dialect::for_path(path).is_none() {
            return Err(LexError::UnsupportedDialect);
        }
        if bytes.len() > self.limits.max_bytes {
            return Err(LexError::TooLarge {
                bytes: bytes.len(),
                limit: self.limits.max_bytes,
            });
        }
        let source = std::str::from_utf8(bytes)?;
        let lexed = lexer::lex(source, &self.limits)?;
        extract::extract(path, source, &lexed, &self.limits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refusals_are_typed_with_positive_controls() {
        let lexer = GoLexer::default();
        assert!(matches!(lexer.extract("x.py", b""), Err(LexError::UnsupportedDialect)));
        assert!(matches!(lexer.extract("x.go", &[0xff]), Err(LexError::InvalidUtf8(_))));
        assert!(matches!(lexer.extract("../x.go", b""), Err(LexError::InvalidId(_))));
        let small = GoLexer::new(Limits {
            max_bytes: 4,
            ..Limits::default()
        });
        assert!(matches!(
            small.extract("x.go", b"package"),
            Err(LexError::TooLarge { bytes: 7, limit: 4 })
        ));
        assert!(small.extract("x.go", b"p").is_ok());
        assert!(lexer.extract("x.go", b"").is_ok());
        assert_eq!(lexer.extract("x.GO", b"package p\n").unwrap().dialect, Dialect::Go);
    }

    #[test]
    fn limits_never_widen_past_the_ceiling() {
        let wide = GoLexer::new(Limits {
            max_bytes: usize::MAX,
            max_depth: u32::MAX,
            max_facts: usize::MAX,
        });
        assert_eq!(wide.limits, Limits::CEILING);
        let tight = GoLexer::new(Limits {
            max_facts: 2,
            ..Limits::default()
        });
        assert!(tight.extract("p.go", b"package p\n").is_ok());
        assert!(matches!(
            tight.extract("p.go", b"package p\nfunc A() {}\n"),
            Err(LexError::TooManyFacts { .. })
        ));
        let deep = GoLexer::new(Limits {
            max_depth: 2,
            ..Limits::default()
        });
        assert!(deep.extract("p.go", b"func f() { {} }\n").is_ok());
        assert!(matches!(
            deep.extract("p.go", b"func f() { {{}} }\n"),
            Err(LexError::TooDeep { .. })
        ));
    }
}
