// SPDX-License-Identifier: Apache-2.0
//! Typed refusals for segment publication.
//! Implements SEGMENTS-v1 and CACHE-v1 names: disk full, cache limit, busy, corrupt, durability.
//! Depends on no other module. [`crate::SegmentStore`] returns these instead of panicking.
//! `Crashed` is the injected publication cut. It is not an OS error string.

use std::fmt;

/// Longest IO detail kept on an error, in bytes.
const MAX_DETAIL: usize = 160;

/// Why a store operation did not finish.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StoreError {
    /// Admission cap would grow past the configured hard cap.
    #[error("cache limit: requested {requested} bytes, {available} available")]
    CacheLimit {
        /// Bytes the operation still needed to write.
        requested: u64,
        /// Bytes left under the cap after emergency metadata.
        available: u64,
    },
    /// The filesystem refused a write because it had no space.
    #[error("disk full")]
    DiskFull,
    /// Another writer holds the lock, or a pin handshake exhausted its retries.
    #[error("busy")]
    Busy,
    /// The caller cancelled publication or maintenance.
    #[error("cancelled")]
    Cancelled,
    /// A pointer, manifest, or segment failed validation.
    #[error("corrupt index: {reason}")]
    CorruptIndex {
        /// Static check that failed.
        reason: &'static str,
    },
    /// Durable publication needs a sync, lock, or atomic replace this adapter lacks.
    #[error("unsupported durability: {capability}")]
    UnsupportedDurability {
        /// Missing capability: `sync`, `atomic-replace`, or `lock`.
        capability: &'static str,
    },
    /// The request itself is not a legal publication.
    #[error("invalid request: {reason}")]
    InvalidRequest {
        /// Static rule that rejected the request.
        reason: &'static str,
    },
    /// The stored major version is not 1.
    #[error("unsupported version {found}")]
    UnsupportedVersion {
        /// Major version found in the object.
        found: u16,
    },
    /// A manifest section carried a VCS identity (kind 6).
    #[error("vcs identity refused")]
    VcsRefused,
    /// Windows sharing rules refused replace or unlink while a handle was open.
    #[error("sharing violation")]
    SharingViolation,
    /// Adapter IO failed. The detail is bounded and has no file contents.
    #[error("io: {detail}")]
    Io {
        /// Truncated adapter message.
        detail: String,
    },
    /// A test cut stopped the process at a publication boundary.
    #[error("crashed at {boundary}")]
    Crashed {
        /// Boundary name from [`super::Boundary`].
        boundary: &'static str,
    },
}

impl StoreError {
    /// Builds an [`StoreError::Io`] cut to 160 bytes on a char boundary.
    pub fn io(detail: impl fmt::Display) -> Self {
        let mut text = detail.to_string();
        if text.len() > MAX_DETAIL {
            let mut end = MAX_DETAIL;
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            text.truncate(end);
        }
        Self::Io { detail: text }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn io_detail_stops_at_a_char_boundary_and_disk_full_is_its_own_error() {
        let long = "é".repeat(200);
        let StoreError::Io { detail } = StoreError::io(long) else {
            panic!("io");
        };
        assert!(detail.len() <= MAX_DETAIL);
        assert!(detail.chars().all(|ch| ch == 'é'));
        assert_eq!(StoreError::DiskFull.to_string(), "disk full");
        assert_ne!(StoreError::DiskFull, StoreError::CacheLimit { requested: 1, available: 0 });
    }
}
