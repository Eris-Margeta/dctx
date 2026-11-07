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
    /// Deepest bracket, interpolation or block nesting.
    pub max_depth: u32,
    /// Most nodes plus references emitted for one file.
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

/// Extraction refusals. No facts are returned with any of these.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LexError {
    /// The path is not Ruby (`.rb`, `.rake`, `.gemspec`, `Rakefile`, `Gemfile`).
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
            Self::UnsupportedDialect => f.write_str("path is not a Ruby file"),
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
            max_depth: u32::MAX,
            max_facts: usize::MAX,
        };
        assert_eq!(large.narrowed(), Limits::CEILING);
        assert!(
            LexError::TooDeep { line: 2, limit: 3 }
                .to_string()
                .contains("line 2")
        );
    }
}
