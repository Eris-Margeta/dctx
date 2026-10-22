//! Hand-written, zero-dependency C# structural extractor.
//! Implements decision 077, PARSERS-v1 and SPEC-MAP-MODE 5–6.2 for `.cs` source.
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
pub const PARSER_ID: &str = "dctx-lex-cs";
/// Bumped whenever extraction output can change for the same bytes.
pub const PARSER_REVISION: u32 = 1;

/// Bounded structural extractor. Stateless apart from its limits.
#[derive(Debug, Clone, Copy, Default)]
pub struct CsLexer {
    limits: Limits,
}

impl CsLexer {
    /// Builds an extractor. Limits above the ceilings are narrowed to them.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn new(limits: Limits) -> Self {
        Self {
            limits: limits.narrowed(),
        }
    }

    /// Extracts facts from one place-relative path and its authorized bytes.
    ///
    /// Refuses a bad path, a non-C# extension, invalid UTF-8, or input past
    /// the byte, depth, or fact ceilings. Malformed source still returns facts
    /// plus diagnostics.
    ///
    /// # Errors
    /// Refuses unsupported paths, invalid UTF-8 or model identities, and
    /// input exceeding the byte, nesting or fact ceilings.
    pub fn extract(&self, path: &str, bytes: &[u8]) -> Result<LexedFile, LexError> {
        Dialect::for_path(path).ok_or(LexError::UnsupportedDialect)?;
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
        let lexer = CsLexer::default();
        assert!(matches!(
            lexer.extract("x.py", b""),
            Err(LexError::UnsupportedDialect)
        ));
        assert!(matches!(
            lexer.extract("x.cs", &[0xff]),
            Err(LexError::InvalidUtf8(_))
        ));
        assert!(matches!(
            lexer.extract("../x.cs", b""),
            Err(LexError::InvalidId(_))
        ));
        let small = CsLexer::new(Limits {
            max_bytes: 4,
            ..Limits::default()
        });
        assert!(matches!(
            small.extract("x.cs", b"run();"),
            Err(LexError::TooLarge { bytes: 6, limit: 4 })
        ));
        assert!(small.extract("x.cs", b"f();").is_ok());
        assert!(lexer.extract("x.cs", b"").is_ok());
        assert_eq!(
            lexer.extract("App.CS", b"class A {}").unwrap().dialect,
            Dialect::CSharp
        );
        assert_eq!(PARSER_ID, "dctx-lex-cs");
        assert_eq!(PARSER_REVISION, 1);
    }

    #[test]
    fn limits_never_widen_past_the_ceiling() {
        let wide = CsLexer::new(Limits {
            max_bytes: usize::MAX,
            max_depth: u32::MAX,
            max_facts: usize::MAX,
        });
        assert_eq!(wide.limits, Limits::CEILING);
        let tight = CsLexer::new(Limits {
            max_facts: 1,
            ..Limits::default()
        });
        assert!(tight.extract("p.cs", b"").is_ok());
        assert!(matches!(
            tight.extract("p.cs", b"class A {}"),
            Err(LexError::TooManyFacts { .. })
        ));
        let zero = CsLexer::new(Limits {
            max_facts: 0,
            ..Limits::default()
        });
        assert!(matches!(
            zero.extract("p.cs", b""),
            Err(LexError::TooManyFacts { limit: 0 })
        ));
        let deep = CsLexer::new(Limits {
            max_depth: 2,
            ..Limits::default()
        });
        assert!(deep.extract("p.cs", b"class A { void M() { } }").is_ok());
        assert!(matches!(
            deep.extract("p.cs", b"class A { void M() { { } } }"),
            Err(LexError::TooDeep { .. })
        ));
    }
}
