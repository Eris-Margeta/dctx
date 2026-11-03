// SPDX-License-Identifier: Apache-2.0
//! Typed refusal for the SHA-256 content hasher.
//! Implements the length bound of FIPS 180-4 (a message longer than 2^64 − 1 bits).
//! Depends on no other dctx crate. [`crate::Sha256::update`] returns it.
//! Callers treat this as a hard refusal; the hasher is left unchanged.

use std::fmt;

/// A message the content hasher will not accept.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HashError {
    /// The message exceeds the SHA-256 length field (2^64 − 1 bits).
    MessageTooLong,
}

impl fmt::Display for HashError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MessageTooLong => f.write_str("SHA-256 message is longer than 2^64-1 bits"),
        }
    }
}

impl std::error::Error for HashError {}
