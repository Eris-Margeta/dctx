//! Hand-written, zero-dependency PHP structural extractor.
//! Implements decision 077, PARSERS-v1 and SPEC-MAP-MODE 5–6.2 for PHP source.
//! Depends on dctx-map-model facts and dctx-hash; never on tree-sitter or git.
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
pub const PARSER_ID: &str = "dctx-lex-php";
/// Bumped whenever extraction output can change for the same bytes.
pub const PARSER_REVISION: u32 = 1;

/// Bounded structural extractor. Stateless apart from its limits.
#[derive(Debug, Clone, Copy, Default)]
pub struct PhpLexer {
    limits: Limits,
}

impl PhpLexer {
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
    /// Refuses unsupported extensions, invalid UTF-8 or model identities, and
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
