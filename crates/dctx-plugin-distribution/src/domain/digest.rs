//! SHA-256 digests, key fingerprints and the streaming payload check.
//! Implements PLUGINS.md §5.2 "DX-SHA compares downloaded bytes against the signed digest" and
//! distribution.md fingerprints (lowercase hex SHA-256 of the exact 32 key bytes, via dctx-hash).
//! Depends on dctx-hash; used by manifest, trust, bundle, index and the download service.

use std::fmt;

use dctx_hash::Sha256;

use crate::error::{DistributionError, Document};

/// A 32-byte SHA-256 digest written as 64 lowercase hex digits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Sha256Hex([u8; 32]);

impl Sha256Hex {
    /// Parses exactly 64 lowercase hex digits.
    /// # Errors
    /// Refuses a digest that is not exactly 64 lowercase hexadecimal characters.
    pub fn parse(text: &str, document: Document) -> Result<Self, DistributionError> {
        let refuse = DistributionError::malformed(document, "sha256-hex");
        let bytes = text.as_bytes();
        if bytes.len() != 64 {
            return Err(refuse);
        }
        let mut out = [0u8; 32];
        for (slot, pair) in out.iter_mut().zip(bytes.as_chunks::<2>().0) {
            *slot = (nibble(pair[0]).ok_or(refuse)? << 4) | nibble(pair[1]).ok_or(refuse)?;
        }
        Ok(Self(out))
    }

    /// Wraps raw digest bytes.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Raw digest bytes.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Hashes a complete in-memory document.
    /// # Errors
    /// Returns a limit error when input exceeds the SHA-256 message length limit.
    pub fn of(bytes: &[u8]) -> Result<Self, DistributionError> {
        let digest = Sha256::digest(bytes).map_err(|_| DistributionError::LimitExceeded { limit: u64::MAX })?;
        Ok(Self(*digest.as_bytes()))
    }
}

impl fmt::Display for Sha256Hex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

fn nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        _ => None,
    }
}

/// Fingerprint of a 32-byte Ed25519 public key.
pub type Fingerprint = Sha256Hex;

/// Fingerprint of exact public key bytes.
#[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
pub fn fingerprint(public_key: &[u8; 32]) -> Fingerprint {
    Sha256Hex::of(public_key).unwrap_or(Sha256Hex([0; 32]))
}

/// Incremental check of one payload against its signed length and digest.
///
/// Refuses as soon as more bytes than signed arrive, so an oversized stream is never buffered.
#[derive(Debug)]
pub struct PayloadCheck {
    expected_bytes: u64,
    expected_digest: Sha256Hex,
    seen: u64,
    hasher: Sha256,
}

impl PayloadCheck {
    /// Starts a check for `expected_bytes` with digest `expected_digest`.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn new(expected_bytes: u64, expected_digest: Sha256Hex) -> Self {
        Self { expected_bytes, expected_digest, seen: 0, hasher: Sha256::new() }
    }

    /// Feeds the next chunk.
    /// # Errors
    /// Refuses bytes beyond the declared length or a hash input length overflow.
    pub fn update(&mut self, chunk: &[u8]) -> Result<(), DistributionError> {
        let seen = self.seen.saturating_add(chunk.len() as u64);
        if seen > self.expected_bytes {
            return Err(DistributionError::SizeMismatch { expected: self.expected_bytes });
        }
        self.hasher
            .update(chunk)
            .map_err(|_| DistributionError::SizeMismatch { expected: self.expected_bytes })?;
        self.seen = seen;
        Ok(())
    }

    /// Ends the stream: exact size first, then exact digest.
    /// # Errors
    /// Refuses a final size mismatch or a digest that differs from the expected digest.
    pub fn finish(self) -> Result<(), DistributionError> {
        if self.seen != self.expected_bytes {
            return Err(DistributionError::SizeMismatch { expected: self.expected_bytes });
        }
        if *self.hasher.finalize().as_bytes() != self.expected_digest.0 {
            return Err(DistributionError::IntegrityFailure);
        }
        Ok(())
    }

    /// Checks a complete in-memory payload.
    /// # Errors
    /// Refuses a size mismatch, hash length overflow, or digest mismatch.
    pub fn whole(bytes: &[u8], expected_bytes: u64, expected_digest: Sha256Hex) -> Result<(), DistributionError> {
        let mut check = Self::new(expected_bytes, expected_digest);
        check.update(bytes)?;
        check.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    #[test]
    fn hex_round_trips_and_refuses_uppercase() {
        let digest = Sha256Hex::parse(ABC, Document::Manifest).unwrap();
        assert_eq!(digest.to_string(), ABC);
        assert_eq!(Sha256Hex::of(b"abc").unwrap(), digest);
        assert!(Sha256Hex::parse(&ABC.to_uppercase(), Document::Manifest).is_err());
        assert!(Sha256Hex::parse(&ABC[..63], Document::Manifest).is_err());
    }

    #[test]
    fn fingerprint_matches_the_fixture_publisher_key() {
        let key: [u8; 32] = [
            0xd7, 0x5a, 0x98, 0x01, 0x82, 0xb1, 0x0a, 0xb7, 0xd5, 0x4b, 0xfe, 0xd3, 0xc9, 0x64, 0x07, 0x3a, 0x0e,
            0xe1, 0x72, 0xf3, 0xda, 0xa6, 0x23, 0x25, 0xaf, 0x02, 0x1a, 0x68, 0xf7, 0x07, 0x51, 0x1a,
        ];
        assert_eq!(
            fingerprint(&key).to_string(),
            "21fe31dfa154a261626bf854046fd2271b7bed4b6abe45aa58877ef47f9721b9"
        );
    }

    #[test]
    fn payload_check_refuses_size_before_hash() {
        let digest = Sha256Hex::parse(ABC, Document::Manifest).unwrap();
        assert!(PayloadCheck::whole(b"abc", 3, digest).is_ok());
        assert_eq!(PayloadCheck::whole(b"abcd", 3, digest), Err(DistributionError::SizeMismatch { expected: 3 }));
        assert_eq!(PayloadCheck::whole(b"ab", 3, digest), Err(DistributionError::SizeMismatch { expected: 3 }));
        assert_eq!(PayloadCheck::whole(b"abd", 3, digest), Err(DistributionError::IntegrityFailure));
    }

    proptest! {
        #[test]
        fn chunking_never_changes_the_verdict(data in proptest::collection::vec(any::<u8>(), 0..300), split in 0usize..300) {
            let digest = Sha256Hex::of(&data).unwrap();
            let cut = split.min(data.len());
            let mut check = PayloadCheck::new(data.len() as u64, digest);
            check.update(&data[..cut]).unwrap();
            check.update(&data[cut..]).unwrap();
            prop_assert!(check.finish().is_ok());
        }
    }
}
