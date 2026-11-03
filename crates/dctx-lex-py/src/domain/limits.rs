// SPDX-License-Identifier: Apache-2.0
//! Input ceilings for one Python extraction.
//! Implements PARSERS-v1 capabilities: caller limits can only narrow ceilings.
//! Used by the public extractor, the lexer and the structural walker.

/// Bounds on one extraction. Ceilings are in [`Limits::CEILING`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Largest accepted file, in bytes.
    pub max_bytes: usize,
    /// Deepest indent, bracket or f-string nesting.
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
    #[allow(clippy::must_use_candidate, clippy::return_self_not_must_use, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
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
