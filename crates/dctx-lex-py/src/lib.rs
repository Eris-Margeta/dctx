// SPDX-License-Identifier: Apache-2.0
//! Hand-written, zero-dependency Python structural extractor.
//! Implements decision 077, PARSERS-v1 and SPEC-MAP-MODE 5–6.2 for Python 3.
//! Depends on dctx-map-model facts and dctx-hash; never on tree-sitter.
//! Emits the dctx-map-parse fact shape with a precision label on every fact.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod domain;
mod error;
mod extract;
mod lexer;

pub use domain::{
    Completion, Diagnostic, DiagnosticReason, Dialect, Labeled, LexError, LexedFile, Limits,
    Precision, RawReference, body_fingerprint,
};

/// Parser identity recorded with every fact key (PARSERS-v1 provenance).
pub const PARSER_ID: &str = "dctx-lex-py";
/// Bumped whenever extraction output can change for the same bytes.
pub const PARSER_REVISION: u32 = 1;

/// Bounded structural extractor; stateless apart from its limits.
#[derive(Debug, Clone, Copy, Default)]
pub struct PyLexer {
    limits: Limits,
}

impl PyLexer {
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
        let lexed = lexer::lex(source, self.limits.max_depth)?;
        extract::extract(path, source, dialect, &lexed, &self.limits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refusals_are_typed_and_have_positive_controls() {
        let lexer = PyLexer::default();
        assert!(matches!(
            lexer.extract("x.js", b""),
            Err(LexError::UnsupportedDialect)
        ));
        assert!(matches!(
            lexer.extract("x.py", &[0xff]),
            Err(LexError::InvalidUtf8(_))
        ));
        assert!(matches!(
            lexer.extract("../x.py", b""),
            Err(LexError::InvalidId(_))
        ));
        let small = PyLexer::new(Limits {
            max_bytes: 4,
            ..Limits::default()
        });
        assert!(matches!(
            small.extract("x.py", b"hello"),
            Err(LexError::TooLarge { bytes: 5, limit: 4 })
        ));
        assert!(small.extract("x.py", b"a=1").is_ok());
        assert!(lexer.extract("x.py", b"").is_ok());
        assert_eq!(Dialect::for_path("pkg/app.PYW"), Some(Dialect::Python));
        assert_eq!(Dialect::for_path("types.pyi"), Some(Dialect::Stub));
        assert_eq!(Dialect::for_path("notes.md"), None);
    }

    #[test]
    fn limits_never_widen_past_the_ceiling() {
        let wide = PyLexer::new(Limits {
            max_bytes: usize::MAX,
            max_depth: u32::MAX,
            max_facts: usize::MAX,
        });
        assert_eq!(wide.limits, Limits::CEILING);
        let deep = "(".repeat(300);
        assert!(matches!(
            PyLexer::default().extract("d.py", deep.as_bytes()),
            Err(LexError::TooDeep { .. })
        ));
        let shallow = "(".repeat(8) + &")".repeat(8);
        assert!(
            PyLexer::default()
                .extract("d.py", shallow.as_bytes())
                .is_ok()
        );
        let narrow = PyLexer::new(Limits {
            max_depth: 4,
            ..Limits::default()
        });
        assert!(narrow.extract("d.py", b"f(g(h(i())))").is_ok());
        assert!(matches!(
            narrow.extract("d.py", "(((((x)))))".as_bytes()),
            Err(LexError::TooDeep { limit: 4, .. })
        ));
        let few = PyLexer::new(Limits {
            max_facts: 1,
            ..Limits::default()
        });
        assert!(matches!(
            few.extract("d.py", b"def f():\n    pass\n"),
            Err(LexError::TooManyFacts { limit: 1 })
        ));
        assert!(few.extract("d.py", b"x = 1\n").is_ok());
        let room = PyLexer::new(Limits {
            max_facts: 2,
            ..Limits::default()
        });
        assert!(room.extract("d.py", b"def f():\n    pass\n").is_ok());
        assert!(
            LexError::TooDeep { line: 4, limit: 8 }
                .to_string()
                .contains("line 4")
        );
    }

    #[test]
    fn strings_and_comments_do_not_invent_declarations() {
        let source = "# def fake():\ns = \"def fake():\"\ns2 = b\"def fake():\"\ndef real():\n    return 1\n";
        let facts = PyLexer::default()
            .extract("m.py", source.as_bytes())
            .unwrap();
        let names: Vec<_> = facts
            .nodes
            .iter()
            .skip(1)
            .map(|node| node.fact.name.as_str())
            .collect();
        assert_eq!(names, ["real"]);
        assert_eq!(facts.completion, Completion::Complete);
        assert_eq!(facts.references, []);
    }
}
