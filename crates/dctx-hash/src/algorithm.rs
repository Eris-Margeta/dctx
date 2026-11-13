// SPDX-License-Identifier: Apache-2.0
//! Versioned, algorithm-tagged content identity.
//! Implements `ObjectKey`'s identity layer in ARCHITECTURE-PROPOSAL (algorithm, digest, byte length).
//! Depends on no other module. [`crate::Sha256`] produces [`ContentDigest`] values.
//! A later algorithm is a new variant, so two digests cannot be compared without their tags.

use std::fmt;

/// Layout version of [`ContentDigest`].
/// Version 1 is the triple `(algorithm code, digest bytes, byte length)`.
pub const IDENTITY_VERSION: u8 = 1;

/// Content-identity algorithm. The numeric code is part of the stored identity.
/// Names and codes are stable; unsupported codes have no fallback algorithm.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum IdentityAlgorithm {
    /// FIPS 180-4 SHA-256. Code [`IdentityAlgorithm::SHA256`].
    Sha256,
}

impl IdentityAlgorithm {
    /// Stable code for [`IdentityAlgorithm::Sha256`]. Unknown codes are refused.
    pub const SHA256: u16 = 1;

    /// Numeric tag stored beside the digest.
    #[must_use]
    pub const fn code(self) -> u16 {
        match self {
            Self::Sha256 => Self::SHA256,
        }
    }

    /// Restores a tag. Any code other than [`IdentityAlgorithm::SHA256`] is refused.
    /// Returns `None` for an unsupported code; select a supported algorithm before hashing.
    #[must_use]
    pub const fn from_code(code: u16) -> Option<Self> {
        match code {
            Self::SHA256 => Some(Self::Sha256),
            _ => None,
        }
    }

    /// Short name used in the textual form `v1:sha256:<hex>:<byte_length>`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Sha256 => "sha256",
        }
    }
}

/// Algorithm-tagged digest of a known number of bytes.
///
/// Equality includes the version, the algorithm, the digest, and the byte length.
/// A future algorithm or layout is a different variant and cannot compare equal to SHA-256 v1.
/// Formatting produces `v1:sha256:<64 lowercase hex digits>:<decimal byte length>`.
/// The format contains no whitespace and is independent of locale.
///
/// This is identity metadata, not proof that the bytes or length were computed
/// from a message. Use [`crate::Sha256::digest`] to compute an identity from bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ContentDigest {
    /// Identity format v1 for a FIPS 180-4 SHA-256 digest.
    Sha256V1 {
        /// 32-byte SHA-256 digest.
        bytes: [u8; 32],
        /// Number of message bytes that were hashed.
        byte_length: u64,
    },
}

impl ContentDigest {
    /// Tags `bytes` as a SHA-256 v1 digest of `byte_length` message bytes.
    ///
    /// This does not hash anything. It rebuilds an identity that was already computed.
    /// Every `u64` length is representable, including values above SHA-256's
    /// message-size limit; callers must validate externally supplied metadata.
    #[must_use]
    pub const fn sha256(bytes: [u8; 32], byte_length: u64) -> Self {
        Self::Sha256V1 { bytes, byte_length }
    }

    /// Layout version. SHA-256 v1 returns [`IDENTITY_VERSION`].
    #[must_use]
    pub const fn version(self) -> u8 {
        match self {
            Self::Sha256V1 { .. } => IDENTITY_VERSION,
        }
    }

    /// Algorithm tag.
    #[must_use]
    pub const fn algorithm(self) -> IdentityAlgorithm {
        match self {
            Self::Sha256V1 { .. } => IdentityAlgorithm::Sha256,
        }
    }

    /// Numeric algorithm code. SHA-256 v1 returns [`IdentityAlgorithm::SHA256`].
    #[must_use]
    pub const fn algorithm_code(self) -> u16 {
        self.algorithm().code()
    }

    /// Message length covered by this digest, in bytes.
    #[must_use]
    pub const fn byte_length(self) -> u64 {
        match self {
            Self::Sha256V1 { byte_length, .. } => byte_length,
        }
    }

    /// Raw digest bytes. The algorithm tag is not included; use [`Self::algorithm`] for that.
    /// The byte length is also separate; use the whole value for identity comparisons.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        match self {
            Self::Sha256V1 { bytes, .. } => bytes,
        }
    }
}

impl fmt::Display for ContentDigest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "v{}:{}:", self.version(), self.algorithm().name())?;
        for byte in self.as_bytes() {
            write!(f, "{byte:02x}")?;
        }
        write!(f, ":{}", self.byte_length())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_algorithm_round_trips_and_unknown_codes_are_refused() {
        assert_eq!(
            IdentityAlgorithm::from_code(IdentityAlgorithm::SHA256),
            Some(IdentityAlgorithm::Sha256)
        );
        assert_eq!(IdentityAlgorithm::Sha256.code(), IdentityAlgorithm::SHA256);
        assert_eq!(IdentityAlgorithm::Sha256.name(), "sha256");
        assert_eq!(IdentityAlgorithm::from_code(0), None);
        assert_eq!(IdentityAlgorithm::from_code(2), None);
        assert_eq!(IdentityAlgorithm::from_code(u16::MAX), None);
    }

    #[test]
    fn tag_and_length_are_part_of_identity() {
        let bytes = [0x11; 32];
        let digest = ContentDigest::sha256(bytes, 4);
        assert_eq!(digest.version(), IDENTITY_VERSION);
        assert_eq!(digest.algorithm(), IdentityAlgorithm::Sha256);
        assert_eq!(digest.algorithm_code(), IdentityAlgorithm::SHA256);
        assert_eq!(digest.byte_length(), 4);
        assert_eq!(digest.as_bytes(), &bytes);
        assert_eq!(digest, ContentDigest::sha256(bytes, 4));
        assert_ne!(digest, ContentDigest::sha256(bytes, 5));
        assert_ne!(digest, ContentDigest::sha256([0x22; 32], 4));
        let text = digest.to_string();
        assert!(text.starts_with("v1:sha256:"));
        assert!(text.ends_with(":4"));
    }
}
