// SPDX-License-Identifier: Apache-2.0
//! Refusals for admission, maintenance and legacy adoption.
//! Implements CACHE-v1 `CacheLimit {needed, available, pinned, pending}` and the typed
//! disk-full failure. Depends on `dctx_core::error::DctxError` for the `Cache` port.
//! `Lifecycle` returns this type; the port adapter maps it onto `DctxError`.

use dctx_core::error::DctxError;

/// A lifecycle refusal. Quota failures keep the pinned and pending totals.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CacheError {
    /// `admitted + requested` would pass the hard cap or a root ceiling.
    #[error(
        "cache limit: requested {requested} bytes, {available} available, {pinned} pinned, {pending} pending"
    )]
    CacheLimit {
        /// Bytes the caller tried to add.
        requested: u64,
        /// Bytes still admissible.
        available: u64,
        /// Bytes held by explicit pins or active leases.
        pinned: u64,
        /// Bytes in the pending-unlink bucket.
        pending: u64,
    },
    /// The maintenance lock or the writer lock is already held.
    #[error("busy")]
    Busy,
    /// The caller cancelled a maintenance slice.
    #[error("cancelled")]
    Cancelled,
    /// The request does not match the lifecycle state.
    #[error("invalid request: {reason}")]
    InvalidRequest {
        /// Static description of the violated rule.
        reason: &'static str,
    },
    /// A pointed-to generation failed validation or was quarantined.
    #[error("corrupt index: {reason}")]
    CorruptIndex {
        /// Static description of the failed check.
        reason: &'static str,
    },
    /// Revocation blocks serving this generation.
    #[error("denied by selection policy")]
    PolicyDenied,
    /// The host reported that the volume cannot take another byte.
    #[error("disk full")]
    DiskFull,
    /// The ledger file names a version this crate does not read.
    #[error("unknown ledger version {found}")]
    UnknownLedgerVersion {
        /// Version word from the file header.
        found: u32,
    },
    /// The ledger bytes failed magic, length, checksum or body checks.
    #[error("corrupt ledger: {reason}")]
    CorruptLedger {
        /// Static description of the failed check.
        reason: &'static str,
    },
    /// A path left the cache root or named a symlink.
    #[error("path escapes the cache root")]
    OutsideCacheRoot,
    /// A filesystem call on the cache root failed.
    #[error("ledger io: {reason}")]
    LedgerIo {
        /// Static description of the failed call.
        reason: &'static str,
    },
}

impl CacheError {
    /// Maps onto the stable port error. Pinned and pending stay in [`CacheError::CacheLimit`].
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn into_dctx(self) -> DctxError {
        match self {
            Self::CacheLimit { requested, available, .. } => {
                DctxError::CacheLimit { requested, available }
            }
            Self::Busy => DctxError::Busy,
            Self::Cancelled => DctxError::Cancelled,
            Self::InvalidRequest { reason } => DctxError::InvalidRequest { reason },
            Self::CorruptIndex { reason } | Self::CorruptLedger { reason } => DctxError::CorruptIndex { reason },
            Self::PolicyDenied => DctxError::PolicyDenied,
            Self::DiskFull => DctxError::io("disk full"),
            Self::UnknownLedgerVersion { .. } => {
                DctxError::CorruptIndex { reason: "unknown ledger version" }
            }
            Self::OutsideCacheRoot => {
                DctxError::InvalidRequest { reason: "path escapes the cache root" }
            }
            Self::LedgerIo { reason } => DctxError::io(reason),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CacheError;
    use dctx_core::error::{DctxError, ErrorCode};

    #[test]
    fn cache_limit_keeps_its_port_shape_and_disk_full_is_io() {
        let mapped = CacheError::CacheLimit {
            requested: 3,
            available: 1,
            pinned: 8,
            pending: 2,
        }
        .into_dctx();
        assert_eq!(mapped, DctxError::CacheLimit { requested: 3, available: 1 });
        assert_eq!(CacheError::DiskFull.into_dctx(), DctxError::io("disk full"));
        assert_eq!(CacheError::DiskFull.into_dctx().code(), ErrorCode::Io);
        let unknown = CacheError::UnknownLedgerVersion { found: 9 }.into_dctx();
        assert_eq!(unknown, DctxError::CorruptIndex { reason: "unknown ledger version" });
        assert_eq!(
            CacheError::CorruptLedger { reason: "checksum" }.into_dctx().code(),
            ErrorCode::CorruptIndex
        );
        assert_eq!(CacheError::LedgerIo { reason: "read" }.into_dctx().code(), ErrorCode::Io);
        assert_eq!(
            CacheError::OutsideCacheRoot.into_dctx(),
            DctxError::InvalidRequest { reason: "path escapes the cache root" }
        );
    }
}
