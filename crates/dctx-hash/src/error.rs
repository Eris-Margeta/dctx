// SPDX-License-Identifier: Apache-2.0
//! Typed refusal for the SHA-256 content hasher.
//! Implements the length bound of FIPS 180-4 (a message longer than 2^64 − 1 bits).
//! Depends on no other dctx crate. [`crate::Sha256::update`] returns it.
//! Callers treat this as a hard refusal; the hasher is left unchanged.

use std::fmt;

/// A message the content hasher will not accept.
///
/// A refused [`crate::Sha256::update`] leaves the entire hasher unchanged.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HashError {
    /// The message exceeds the SHA-256 length field (2^64 − 1 bits).
    /// Hash at most `u64::MAX / 8` bytes in one computation.
    MessageTooLong,
}

impl fmt::Display for HashError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MessageTooLong => f.write_str(
                "SHA-256 message is longer than 2^64-1 bits; hash fewer bytes (at most 2305843009213693951 bytes per message)",
            ),
        }
    }
}

impl std::error::Error for HashError {}
