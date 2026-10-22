//! Hand-written, zero-dependency Kotlin structural extractor.
//! Implements decision 077, PARSERS-v1 and SPEC-MAP-MODE 5–6.2 for `.kt` and `.kts`.
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
pub const PARSER_ID: &str = "dctx-lex-kt";
/// Bumped whenever extraction output can change for the same bytes.
pub const PARSER_REVISION: u32 = 1;

/// Bounded structural extractor. Stateless apart from its limits.
#[derive(Debug, Clone, Copy, Default)]
pub struct KotlinLexer {
    limits: Limits,
}

impl KotlinLexer {
    /// Builds an extractor. Limits above the ceilings are narrowed to them.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn new(limits: Limits) -> Self {
        Self {
            limits: limits.narrowed(),
        }
    }

    /// Extracts facts from one place-relative path and its authorized bytes.
    ///
    /// Refuses a bad path, a non-Kotlin extension, invalid UTF-8, or input past
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
    fn escaped_non_ascii_characters_preserve_following_declarations() {
        // Invalid Kotlin escapes still need bounded recovery without losing facts.
        // ASCII escapes are the positive control for every UTF-8 character width.
        for escaped in ["n", "é", "€", "😀"] {
            for quote in ['"', '\''] {
                for separator in ["; ", "\n"] {
                    let source = format!(
                        "fun earlier() = 1\nval x = {quote}\\{escaped}{quote}{separator}fun after() {{}}\n"
                    );
                    let facts = KotlinLexer::default()
                        .extract("Escape.kt", source.as_bytes())
                        .expect("extract escaped literal");
                    let names: Vec<_> = facts.nodes.iter().map(|node| node.fact.name.as_str()).collect();
                    assert_eq!(names, ["Escape.kt", "earlier", "x", "after"], "{source:?}");
                    assert_eq!(facts.completion, Completion::Complete, "{source:?}");
                    assert!(facts.diagnostics.is_empty(), "{source:?}: {:?}", facts.diagnostics);
                }
            }
        }
    }

    #[test]
    fn refusals_are_typed_with_positive_controls() {
        let lexer = KotlinLexer::default();
        assert!(matches!(
            lexer.extract("x.py", b""),
            Err(LexError::UnsupportedDialect)
        ));
        assert!(matches!(
            lexer.extract("x.kt", &[0xff]),
            Err(LexError::InvalidUtf8(_))
        ));
        assert!(matches!(
            lexer.extract("../x.kt", b""),
            Err(LexError::InvalidId(_))
        ));
        let small = KotlinLexer::new(Limits {
            max_bytes: 4,
            ..Limits::default()
        });
        assert!(matches!(
            small.extract("x.kt", b"class"),
            Err(LexError::TooLarge { bytes: 5, limit: 4 })
        ));
        assert!(small.extract("x.kt", b"v").is_ok());
        assert!(lexer.extract("x.kt", b"").is_ok());
        assert!(lexer.extract("App.KTS", b"fun main() {}").is_ok());
        let bare = lexer.extract("P.kt", b"package p").expect("package");
        assert_eq!(bare.nodes[0].fact.signature, "p");
        assert_eq!(bare.nodes[0].fact.summary.source.as_str(), "sig");
        assert_eq!(bare.nodes[0].fact.summary.text, "p");
        assert_eq!(PARSER_ID, "dctx-lex-kt");
        assert_eq!(PARSER_REVISION, 1);
        let script = lexer
            .extract("a.kts", b"#!/usr/bin/env kotlin\nfun main() {}\n")
            .expect("script");
        assert_eq!(script.nodes[1].fact.name, "main");
    }

    #[test]
    fn limits_never_widen_past_the_ceiling() {
        let wide = KotlinLexer::new(Limits {
            max_bytes: usize::MAX,
            max_depth: u32::MAX,
            max_facts: usize::MAX,
        });
        assert_eq!(wide.limits, Limits::CEILING);
        let deep = KotlinLexer::new(Limits {
            max_depth: 2,
            ..Limits::default()
        });
        assert!(deep.extract("a.kt", b"{{}}").is_ok());
        assert!(matches!(
            deep.extract("a.kt", b"{{{"),
            Err(LexError::TooDeep { limit: 2, .. })
        ));
        let two = KotlinLexer::new(Limits {
            max_facts: 2,
            ..Limits::default()
        });
        assert!(two.extract("a.kt", b"fun a() {}").is_ok());
        assert!(matches!(
            two.extract("a.kt", b"fun a() {}\nfun b() {}"),
            Err(LexError::TooManyFacts { limit: 2 })
        ));
        let zero = KotlinLexer::new(Limits {
            max_facts: 0,
            ..Limits::default()
        });
        assert!(matches!(
            zero.extract("a.kt", b""),
            Err(LexError::TooManyFacts { limit: 0 })
        ));
    }
}
