//! Budgets and limits: estimated tokens, output budgets, pack limits and cache reservations.
//! Implements `estimated_tokens = ceil(2 * utf8_bytes / 7)` and the byte ceiling `floor(7B/2)`
//! of contracts/API-PORTS-v1.md ("Requests, budgets and results"); an estimate, not a tokenizer.
//! Depends on [`crate::error`]. Used by dctx-ports (`Cache`) and dctx-pack (snapshot limits).

#![deny(missing_docs)]

use crate::error::{DctxError, Resource};

/// Estimated tokens of `bytes` UTF-8 bytes: `ceil(2 * bytes / 7)`, without overflow.
#[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
pub fn estimate_tokens(bytes: u64) -> u64 {
    let tokens = (u128::from(bytes) * 2).div_ceil(7);
    u64::try_from(tokens).unwrap_or(u64::MAX)
}

/// Largest byte count whose estimate stays within `tokens`: `floor(7 * tokens / 2)`, saturating.
#[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
pub fn byte_ceiling(tokens: u64) -> u64 {
    u64::try_from(u128::from(tokens) * 7 / 2).unwrap_or(u64::MAX)
}

/// An output budget; `None` means that dimension is unbounded by this party.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OutputBudget {
    /// Final encoded bytes.
    pub encoded_bytes: Option<u64>,
    /// Estimated tokens of the final encoded bytes.
    pub estimated_tokens: Option<u64>,
    /// Whole records.
    pub records: Option<u64>,
}

impl OutputBudget {
    /// The tighter of two budgets in every dimension (host ∩ policy ∩ session).
    #[allow(clippy::must_use_candidate, clippy::return_self_not_must_use, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn intersect(self, other: Self) -> Self {
        Self {
            encoded_bytes: min_opt(self.encoded_bytes, other.encoded_bytes),
            estimated_tokens: min_opt(self.estimated_tokens, other.estimated_tokens),
            records: min_opt(self.records, other.records),
        }
    }

    /// Effective byte ceiling: the explicit byte cap intersected with the token-derived ceiling.
    pub fn effective_bytes(self) -> Option<u64> {
        min_opt(self.encoded_bytes, self.estimated_tokens.map(byte_ceiling))
    }

    /// True if an answer of `bytes` encoded bytes and `records` records fits.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn admits(self, bytes: u64, records: u64) -> bool {
        self.effective_bytes().is_none_or(|cap| bytes <= cap)
            && self.records.is_none_or(|cap| records <= cap)
    }
}

fn min_opt(a: Option<u64>, b: Option<u64>) -> Option<u64> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.min(y)),
        (x, None) => x,
        (None, y) => y,
    }
}

/// Bounds on one pack snapshot. The default is unbounded, matching the legacy walker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackLimits {
    /// Most tree entries (files and directories, root included).
    pub max_entries: u64,
    /// Deepest directory nesting below the root.
    pub max_depth: u64,
    /// Largest single file whose content is read; larger files are withheld.
    pub max_file_bytes: u64,
    /// Most content bytes read from the source in total.
    pub max_source_bytes: u64,
}

impl Default for PackLimits {
    fn default() -> Self {
        Self::unbounded()
    }
}

impl PackLimits {
    /// No limits (legacy behaviour).
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub const fn unbounded() -> Self {
        Self {
            max_entries: u64::MAX,
            max_depth: u64::MAX,
            max_file_bytes: u64::MAX,
            max_source_bytes: u64::MAX,
        }
    }

    /// Refuses when `count` entries exceed [`Self::max_entries`].
    /// # Errors
    /// Returns `DctxError::ResourceLimit` when the entry count exceeds the configured ceiling.
    pub fn check_entries(&self, count: u64) -> Result<(), DctxError> {
        check(Resource::Entries, count, self.max_entries)
    }

    /// Refuses when `depth` exceeds [`Self::max_depth`].
    /// # Errors
    /// Returns `DctxError::ResourceLimit` when the depth exceeds the configured ceiling.
    pub fn check_depth(&self, depth: u64) -> Result<(), DctxError> {
        check(Resource::Depth, depth, self.max_depth)
    }

    /// Refuses when `total` content bytes exceed [`Self::max_source_bytes`].
    /// # Errors
    /// Returns `DctxError::ResourceLimit` when source bytes exceed the configured ceiling.
    pub fn check_source_bytes(&self, total: u64) -> Result<(), DctxError> {
        check(Resource::SourceBytes, total, self.max_source_bytes)
    }
}

fn check(resource: Resource, value: u64, limit: u64) -> Result<(), DctxError> {
    if value > limit {
        Err(DctxError::ResourceLimit { resource, limit })
    } else {
        Ok(())
    }
}

/// Bytes admitted by a cache before they are written; settled by commit or abort.
#[derive(Debug, PartialEq, Eq)]
pub struct Reservation {
    id: u64,
    bytes: u64,
}

impl Reservation {
    /// Issued by a cache adapter for `bytes` under its own `id`.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn new(id: u64, bytes: u64) -> Self {
        Self { id, bytes }
    }

    /// Adapter-scoped identity of this reservation.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Reserved bytes.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn bytes(&self) -> u64 {
        self.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_estimate_rounds_up_and_never_overflows() {
        assert_eq!(estimate_tokens(0), 0);
        assert_eq!(estimate_tokens(1), 1);
        assert_eq!(estimate_tokens(7), 2);
        assert_eq!(estimate_tokens(8), 3);
        assert!(estimate_tokens(u64::MAX) < u64::MAX);
    }

    #[test]
    fn byte_ceiling_is_the_inverse_bound() {
        for tokens in [0, 1, 2, 3, 500, 2000] {
            let cap = byte_ceiling(tokens);
            assert!(estimate_tokens(cap) <= tokens);
            assert!(estimate_tokens(cap + 1) > tokens || cap == u64::MAX);
        }
        assert_eq!(byte_ceiling(u64::MAX), u64::MAX);
    }

    #[test]
    fn budgets_intersect_to_the_tighter_bound() {
        let host = OutputBudget { encoded_bytes: Some(10_000), estimated_tokens: None, records: Some(60) };
        let session = OutputBudget { encoded_bytes: None, estimated_tokens: Some(500), records: Some(100) };
        let both = host.intersect(session);
        assert_eq!(both.effective_bytes(), Some(1750));
        assert_eq!(both.records, Some(60));
        assert!(both.admits(1750, 60));
        assert!(!both.admits(1751, 60));
        assert!(!both.admits(10, 61));
        assert!(OutputBudget::default().admits(u64::MAX, u64::MAX));
    }

    #[test]
    fn pack_limits_refuse_past_the_limit_only() {
        let limits = PackLimits { max_entries: 2, max_depth: 1, ..PackLimits::unbounded() };
        assert!(limits.check_entries(2).is_ok());
        assert_eq!(
            limits.check_entries(3),
            Err(DctxError::ResourceLimit { resource: Resource::Entries, limit: 2 })
        );
        assert!(limits.check_depth(1).is_ok());
        assert!(limits.check_depth(2).is_err());
        assert!(PackLimits::default().check_source_bytes(u64::MAX).is_ok());
    }
}
