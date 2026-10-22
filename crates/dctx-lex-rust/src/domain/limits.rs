//! Extraction ceilings and the typed refusals they produce.
//! Implements PARSERS-v1 "Caller limits can only narrow these ceilings".
//! Pure values; the extract pipeline enforces them before allocating a syntax tree.

/// Bounds on one extraction. Caller values can only narrow [`Limits::CEILING`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::struct_field_names, reason = "The max prefix distinguishes extraction ceilings from observed byte, nesting and record counts.")]
pub struct Limits {
    max_bytes: usize,
    max_nesting: u32,
    max_records: usize,
}

impl Limits {
    /// Largest supported input, nesting cost and record count.
    pub const CEILING: Self = Self {
        max_bytes: 8 << 20,
        max_nesting: 2048,
        max_records: 200_000,
    };

    /// Limits clamped to [`Limits::CEILING`].
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior for the limits constructor.")]
    pub fn narrowed(max_bytes: usize, max_nesting: u32, max_records: usize) -> Self {
        Self {
            max_bytes: max_bytes.min(Self::CEILING.max_bytes),
            max_nesting: max_nesting.min(Self::CEILING.max_nesting),
            max_records: max_records.min(Self::CEILING.max_records),
        }
    }

    /// Largest accepted input in bytes.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior for the limits accessor.")]
    pub fn max_bytes(&self) -> usize {
        self.max_bytes
    }

    /// Largest nesting cost: tokens on the open syntactic path (see the nesting adapter).
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior for the limits accessor.")]
    pub fn max_nesting(&self) -> u32 {
        self.max_nesting
    }

    /// Largest number of declarations, imports and references together.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior for the limits accessor.")]
    pub fn max_records(&self) -> usize {
        self.max_records
    }
}

impl Default for Limits {
    fn default() -> Self {
        Self::CEILING
    }
}

/// Why a file was refused outright or only partially extracted.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    /// Input exceeds the byte limit; nothing was read.
    #[error("input of {bytes} bytes exceeds the {limit}-byte limit")]
    TooLarge {
        /// Input length.
        bytes: usize,
        /// Applied limit.
        limit: usize,
    },
    /// Input is not UTF-8.
    #[error("input is not UTF-8 after byte {valid_up_to}")]
    InvalidUtf8 {
        /// Length of the valid prefix.
        valid_up_to: usize,
    },
    /// Nesting cost exceeds the limit; syn was not run.
    #[error("nesting exceeds {limit} at line {line}")]
    TooDeep {
        /// First line where the limit was crossed.
        line: u32,
        /// Applied limit.
        limit: u32,
    },
    /// Unterminated literal, comment or delimiter.
    #[error("unlexable input at line {line}")]
    Lex {
        /// Reported line, 1-based; 0 when unknown.
        line: u32,
    },
    /// Tokens do not form a supported Rust file.
    #[error("unsupported or invalid syntax at line {line}")]
    Syntax {
        /// First error line, 1-based.
        line: u32,
    },
    /// The record limit was reached; later facts were dropped.
    #[error("more than {limit} records")]
    TooManyRecords {
        /// Applied limit.
        limit: usize,
    },
    /// The bounded parse worker could not start or did not finish.
    #[error("parse worker failed")]
    Worker,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caller_limits_only_narrow_the_ceiling() {
        let wide = Limits::narrowed(usize::MAX, u32::MAX, usize::MAX);
        assert_eq!(wide, Limits::CEILING);
        let narrow = Limits::narrowed(10, 3, 1);
        assert_eq!(
            (narrow.max_bytes(), narrow.max_nesting(), narrow.max_records()),
            (10, 3, 1)
        );
        assert_eq!(Limits::default(), Limits::CEILING);
    }

    #[test]
    fn refusals_render_their_reason() {
        let refusal = Refusal::TooLarge { bytes: 9, limit: 8 };
        assert_eq!(
            refusal.to_string(),
            "input of 9 bytes exceeds the 8-byte limit"
        );
    }
}
