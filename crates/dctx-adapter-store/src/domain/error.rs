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
    #[error(
        "cache limit: requested {requested} bytes, {available} available; release unused pins and run gc, or raise the cap, then retry"
    )]
    CacheLimit {
        /// Bytes the operation still needed to write.
        requested: u64,
        /// Bytes left under the cap after emergency metadata.
        available: u64,
    },
    /// The filesystem refused a write because it had no space.
    #[error("disk full; free space on the store volume and retry")]
    DiskFull,
    /// Another writer holds the lock, or a pin handshake exhausted its retries.
    #[error("busy; retry after the current writer or pin operation finishes")]
    Busy,
    /// The caller cancelled publication or maintenance.
    #[error("cancelled; start a new operation when ready")]
    Cancelled,
    /// A pointer, manifest, or segment failed validation.
    #[error(
        "corrupt index: {reason}; run recovery or rebuild the affected generation from authorized sources"
    )]
    CorruptIndex {
        /// Static check that failed.
        reason: &'static str,
    },
    /// Durable publication needs a sync, lock, or atomic replace this adapter lacks.
    #[error(
        "unsupported durability: {capability}; use a filesystem with this capability or explicitly select ephemeral mode"
    )]
    UnsupportedDurability {
        /// Missing capability: `sync`, `atomic-replace`, or `lock`.
        capability: &'static str,
    },
    /// The request itself is not a legal publication.
    #[error(
        "invalid request: {reason}; correct the request and acquire any pin from this store before retrying"
    )]
    InvalidRequest {
        /// Static rule that rejected the request.
        reason: &'static str,
    },
    /// The stored major version is not 1.
    #[error(
        "unsupported version {found}; use a compatible version of dctx or rebuild the affected generation"
    )]
    UnsupportedVersion {
        /// Major version found in the object.
        found: u16,
    },
    /// A manifest section carried a VCS identity (kind 6).
    #[error("vcs identity refused; publish a manifest without VCS identity sections")]
    VcsRefused,
    /// Windows sharing rules refused replace or unlink while a handle was open.
    #[error("sharing violation; close handles to the affected object and retry")]
    SharingViolation,
    /// Adapter IO failed. The detail is bounded and has no file contents.
    #[error("io: {detail}; check store permissions and available space, then retry")]
    Io {
        /// Truncated adapter message.
        detail: String,
    },
    /// A test cut stopped the process at a publication boundary.
    #[error("crashed at {boundary}; reopen the store and run recovery before retrying")]
    Crashed {
        /// Boundary name from [`super::Boundary`].
        boundary: &'static str,
    },
}

impl StoreError {
    /// Builds an [`StoreError::Io`] with at most 160 UTF-8 bytes.
    /// Control characters are escaped, and formatting stops as soon as the bound is reached.
    pub fn io(detail: impl fmt::Display) -> Self {
        let mut bounded = BoundedDetail(String::with_capacity(MAX_DETAIL));
        let _ = fmt::write(&mut bounded, format_args!("{detail}"));
        Self::Io { detail: bounded.0 }
    }
}

/// Bounds formatting itself, so a large display value is never buffered in full.
struct BoundedDetail(String);

impl BoundedDetail {
    fn push(&mut self, character: char) -> fmt::Result {
        if self.0.len() + character.len_utf8() > MAX_DETAIL {
            return Err(fmt::Error);
        }
        self.0.push(character);
        Ok(())
    }
}

impl fmt::Write for BoundedDetail {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        for character in text.chars() {
            if character.is_control() {
                for escaped in character.escape_default() {
                    self.push(escaped)?;
                }
            } else {
                self.push(character)?;
            }
        }
        Ok(())
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
        assert_eq!(
            StoreError::DiskFull.to_string(),
            "disk full; free space on the store volume and retry"
        );
        assert_ne!(
            StoreError::DiskFull,
            StoreError::CacheLimit {
                requested: 1,
                available: 0
            }
        );
    }
}
