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
}

impl CacheError {
    /// Maps onto the stable port error. Pinned and pending stay in [`CacheError::CacheLimit`].
    pub fn into_dctx(self) -> DctxError {
        match self {
            Self::CacheLimit { requested, available, .. } => {
                DctxError::CacheLimit { requested, available }
            }
            Self::Busy => DctxError::Busy,
            Self::Cancelled => DctxError::Cancelled,
            Self::InvalidRequest { reason } => DctxError::InvalidRequest { reason },
            Self::CorruptIndex { reason } => DctxError::CorruptIndex { reason },
            Self::PolicyDenied => DctxError::PolicyDenied,
            Self::DiskFull => DctxError::io("disk full"),
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
    }
}
