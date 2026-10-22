//! Hand-written, zero-dependency C and C++ structural extractor.
//! Implements decision 077, PARSERS-v1 and SPEC-MAP-MODE 5–6.2 for C and C++ source.
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
pub const PARSER_ID: &str = "dctx-lex-c";
/// Bumped whenever extraction output can change for the same bytes.
pub const PARSER_REVISION: u32 = 1;

/// Bounded structural extractor. Stateless apart from its limits.
#[derive(Debug, Clone, Copy, Default)]
pub struct CLexer {
    limits: Limits,
}

impl CLexer {
    /// Builds an extractor. Limits above the ceilings are narrowed to them.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn new(limits: Limits) -> Self {
        Self {
            limits: limits.narrowed(),
        }
    }

    /// Extracts facts from one place-relative path and its authorized bytes.
    ///
    /// Refuses a bad path, an unsupported extension, invalid UTF-8, or input past
    /// the byte, depth, or fact ceilings. Malformed source still returns facts
    /// plus diagnostics.
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
        let lexed = lexer::lex(source, &self.limits)?;
        extract::extract(path, source, dialect, &lexed, &self.limits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refusals_are_typed_with_positive_controls() {
        let lexer = CLexer::default();
        assert!(matches!(
            lexer.extract("x.py", b""),
            Err(LexError::UnsupportedDialect)
        ));
        assert!(matches!(
            lexer.extract("x.c", &[0xff]),
            Err(LexError::InvalidUtf8(_))
        ));
        assert!(matches!(
            lexer.extract("../x.c", b""),
            Err(LexError::InvalidId(_))
        ));
        let small = CLexer::new(Limits {
            max_bytes: 4,
            ..Limits::default()
        });
        assert!(matches!(
            small.extract("x.c", b"int x;"),
            Err(LexError::TooLarge { bytes: 6, limit: 4 })
        ));
        assert!(small.extract("x.c", b"int;").is_ok());
        assert!(lexer.extract("x.c", b"").is_ok());
        assert_eq!(
            lexer.extract("x.H", b"int x;").unwrap().dialect,
            Dialect::C
        );
        assert_eq!(PARSER_ID, "dctx-lex-c");
        assert_eq!(PARSER_REVISION, 1);
    }

    #[test]
    fn limits_never_widen_past_the_ceiling() {
        let wide = CLexer::new(Limits {
            max_bytes: usize::MAX,
            max_depth: u32::MAX,
            max_facts: usize::MAX,
        });
        assert_eq!(wide.limits, Limits::CEILING);
        let tight = CLexer::new(Limits {
            max_facts: 1,
            ..Limits::default()
        });
        assert!(tight.extract("p.c", b"").is_ok());
        assert!(matches!(
            tight.extract("p.c", b"int a;\n"),
            Err(LexError::TooManyFacts { .. })
        ));
        let deep = CLexer::new(Limits {
            max_depth: 2,
            ..Limits::default()
        });
        assert!(deep.extract("p.c", b"void f() { {} }\n").is_ok());
        assert!(matches!(
            deep.extract("p.c", b"void f() { {{{}}} }\n"),
            Err(LexError::TooDeep { .. })
        ));
        assert!(matches!(
            deep.extract("p.hpp", b"template<class A = B<C<int>>> struct S;\n"),
            Err(LexError::TooDeep { .. })
        ));
    }
}
