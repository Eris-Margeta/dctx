//! Typed refusals for Python extraction. No facts are returned with any of these.
//! Implements PARSERS-v1 refusal rows: dialect, identity, UTF-8, size, depth and facts.
//! Returned by the lexer and the walker; `PyLexer::extract` surfaces them unchanged.
use std::fmt;

/// Extraction refusals. No facts are returned with any of these.
#[derive(Debug)]
pub enum LexError {
    /// The path is not a Python dialect.
    UnsupportedDialect,
    /// The path or a generated name cannot form a model identity.
    InvalidId(dctx_map_model::InvalidId),
    /// Supplied bytes are not UTF-8.
    InvalidUtf8(std::str::Utf8Error),
    /// The file exceeds the byte limit.
    TooLarge {
        /// Size of the refused input.
        bytes: usize,
        /// Limit in force.
        limit: usize,
    },
    /// Nesting exceeds the depth limit.
    TooDeep {
        /// One-based line where the limit was crossed.
        line: u32,
        /// Limit in force.
        limit: u32,
    },
    /// More facts than the fact limit.
    TooManyFacts {
        /// Limit in force.
        limit: usize,
    },
}

impl fmt::Display for LexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedDialect => f.write_str("path is not a Python file"),
            Self::InvalidId(error) => error.fmt(f),
            Self::InvalidUtf8(error) => error.fmt(f),
            Self::TooLarge { bytes, limit } => {
                write!(f, "file of {bytes} bytes exceeds the {limit}-byte limit")
            }
            Self::TooDeep { line, limit } => {
                write!(f, "nesting deeper than {limit} at line {line}")
            }
            Self::TooManyFacts { limit } => write!(f, "file yields more than {limit} facts"),
        }
    }
}

impl std::error::Error for LexError {}

impl From<dctx_map_model::InvalidId> for LexError {
    fn from(error: dctx_map_model::InvalidId) -> Self {
        Self::InvalidId(error)
    }
}

impl From<std::str::Utf8Error> for LexError {
    fn from(error: std::str::Utf8Error) -> Self {
        Self::InvalidUtf8(error)
    }
}
