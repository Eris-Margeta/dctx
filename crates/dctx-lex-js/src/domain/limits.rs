// SPDX-License-Identifier: Apache-2.0
//! Input ceilings and the typed refusals returned instead of partial facts.
//! Implements PARSERS-v1 "Capabilities": caller limits can only narrow ceilings.
//! Used by the public extractor, the lexer and the structural walker.
use std::fmt;

/// Bounds on one extraction. Ceilings are in [`Limits::CEILING`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Largest accepted file, in bytes.
    pub max_bytes: usize,
    /// Deepest bracket, template, JSX or type nesting.
    pub max_depth: u32,
    /// Most nodes plus references emitted for one file, including the module node.
    /// Zero refuses every file; containment edges do not consume this budget.
    pub max_facts: usize,
}

impl Limits {
    /// The widest limits this extractor supports.
    pub const CEILING: Self = Self {
        max_bytes: 8 << 20,
        max_depth: 256,
        max_facts: 250_000,
    };

    /// Clamps every limit to its ceiling.
    #[allow(
        clippy::must_use_candidate,
        clippy::return_self_not_must_use,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn narrowed(self) -> Self {
        Self {
            max_bytes: self.max_bytes.min(Self::CEILING.max_bytes),
            max_depth: self.max_depth.min(Self::CEILING.max_depth),
            max_facts: self.max_facts.min(Self::CEILING.max_facts),
        }
    }
}

impl Default for Limits {
    fn default() -> Self {
        Self::CEILING
    }
}

/// Extraction refusals; no facts are returned with any of these.
#[derive(Debug)]
pub enum LexError {
    /// The path is not a JavaScript or TypeScript dialect.
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
            Self::UnsupportedDialect => f.write_str(
                "path is not a JavaScript or TypeScript file; use a supported extension such as .js or .ts",
            ),
            Self::InvalidId(error) => write!(
                f,
                "{error}; use a place-relative path and names without traversal or reserved identity characters"
            ),
            Self::InvalidUtf8(error) => write!(
                f,
                "{error}; decode or transcode the source to UTF-8 and retry"
            ),
            Self::TooLarge { bytes, limit } => {
                write!(f, "file of {bytes} bytes exceeds the {limit}-byte limit; split the file or reduce the input and retry")
            }
            Self::TooDeep { line, limit } => {
                write!(f, "nesting deeper than {limit} at line {line}; reduce syntactic nesting and retry")
            }
            Self::TooManyFacts { limit } => write!(f, "file yields more than {limit} facts; split the file or increase max_facts up to Limits::CEILING and retry"),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn narrowing_keeps_smaller_limits_and_clamps_larger_ones() {
        let small = Limits {
            max_bytes: 10,
            max_depth: 3,
            max_facts: 7,
        };
        assert_eq!(small.narrowed(), small);
        let large = Limits {
            max_bytes: usize::MAX,
            ..small
        };
        assert_eq!(large.narrowed().max_bytes, Limits::CEILING.max_bytes);
        assert!(
            LexError::TooDeep { line: 2, limit: 3 }
                .to_string()
                .contains("line 2")
        );
    }
}
