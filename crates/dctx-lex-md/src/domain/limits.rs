//! Input bounds and the typed refusals returned when a file exceeds them.
//! Implements the bounded-parser rule of ARCHITECTURE-PROPOSAL §3 (adversarial limits).
//! Pure data; checked by `extract_with` and the block scanner.
use std::fmt;

/// Bounds on the work done for one file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Largest accepted input in bytes; never above `u32::MAX - 1`.
    pub max_bytes: usize,
    /// Deepest accepted nesting of block quotes and list items.
    pub max_depth: usize,
    /// Open `[` per paragraph that may still form links; later ones are literal text.
    pub max_brackets: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_bytes: 32 << 20,
            max_depth: 32,
            max_brackets: 1024,
        }
    }
}

/// Refusal to extract; no partial document is returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LexError {
    /// Input is larger than `Limits::max_bytes`.
    TooLarge {
        /// Input length in bytes.
        len: usize,
        /// Accepted maximum.
        max: usize,
    },
    /// Containers nest deeper than `Limits::max_depth`.
    TooDeep {
        /// One-based line where the limit was crossed.
        line: u32,
        /// Accepted maximum depth.
        max: usize,
    },
    /// Input is not UTF-8.
    InvalidUtf8 {
        /// Length of the valid prefix.
        valid_up_to: usize,
    },
}

impl fmt::Display for LexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge { len, max } => {
                write!(f, "markdown input of {len} bytes exceeds {max} bytes")
            }
            Self::TooDeep { line, max } => {
                write!(f, "markdown nesting deeper than {max} at line {line}")
            }
            Self::InvalidUtf8 { valid_up_to } => {
                write!(f, "markdown input is not UTF-8 after byte {valid_up_to}")
            }
        }
    }
}

impl std::error::Error for LexError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refusals_explain_themselves() {
        assert_eq!(
            LexError::TooDeep { line: 3, max: 2 }.to_string(),
            "markdown nesting deeper than 2 at line 3"
        );
        assert!(Limits::default().max_bytes < u32::MAX as usize);
    }
}
