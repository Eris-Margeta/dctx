// SPDX-License-Identifier: Apache-2.0
//! Algorithm-tagged content identity.
//! Implements IDENTITY-v1 `ObjectKey` text and decision 078: own hashing, no git object ids.
//! Wraps [`dctx_hash::ContentDigest`]. Callers are facts, provenance and the future wire adapter.
//! Untagged hex is refused, including 40- and 64-character git object ids.

use std::{fmt, str::FromStr};

/// Digest algorithm stored beside the bytes. Code 1 is SHA-256.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ContentAlgorithm {
    /// FIPS 180-4 SHA-256. Numeric code 1.
    Sha256,
}

impl ContentAlgorithm {
    /// Stable code stored with the digest.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub const fn code(self) -> u16 {
        match self {
            Self::Sha256 => dctx_hash::IdentityAlgorithm::SHA256,
        }
    }

    /// Short name used in the textual identity.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Sha256 => "sha256",
        }
    }
}

/// Why content bytes or text could not become an identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvalidContentId {
    /// The text is not `dctx:object:v1:sha256:<64 lowercase hex>:<length>`.
    UnsupportedIdentity,
    /// The byte string does not fit in the SHA-256 length field.
    MessageTooLong,
}

impl fmt::Display for InvalidContentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::UnsupportedIdentity => "unsupported content identity",
            Self::MessageTooLong => "content is longer than the identity hasher accepts",
        })
    }
}

impl std::error::Error for InvalidContentId {}

/// Content identity: version, algorithm, digest and byte length.
///
/// Equality includes every one of those fields. A git object id is not a value of this type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ContentId(dctx_hash::ContentDigest);

impl ContentId {
    /// Hashes authorized file bytes with `dctx-hash` SHA-256.
    ///
    /// # Errors
    /// Returns [`InvalidContentId::MessageTooLong`] when the input exceeds the SHA-256 length field.
    pub fn of_bytes(bytes: &[u8]) -> Result<Self, InvalidContentId> {
        dctx_hash::Sha256::digest(bytes)
            .map(Self)
            .map_err(|_| InvalidContentId::MessageTooLong)
    }

    /// Identity layout version. SHA-256 v1 returns 1.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub const fn version(self) -> u8 {
        self.0.version()
    }

    /// Algorithm tag.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn algorithm(self) -> ContentAlgorithm {
        match self.0.algorithm() {
            dctx_hash::IdentityAlgorithm::Sha256 => ContentAlgorithm::Sha256,
        }
    }

    /// Number of content bytes that were hashed.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub const fn byte_length(self) -> u64 {
        self.0.byte_length()
    }

    /// Raw digest bytes. The algorithm tag is [`Self::algorithm`].
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub const fn digest_bytes(&self) -> &[u8; 32] {
        self.0.as_bytes()
    }

    /// Restores a previously emitted identity. This does not hash the text.
    ///
    /// # Errors
    /// Returns [`InvalidContentId::UnsupportedIdentity`] for unsupported tags, malformed digest text or a noncanonical byte length.
    pub fn parse(text: &str) -> Result<Self, InvalidContentId> {
        let rest = text
            .strip_prefix("dctx:object:v")
            .ok_or(InvalidContentId::UnsupportedIdentity)?;
        let (version, rest) = rest
            .split_once(':')
            .ok_or(InvalidContentId::UnsupportedIdentity)?;
        let (algorithm, rest) = rest
            .split_once(':')
            .ok_or(InvalidContentId::UnsupportedIdentity)?;
        let (hex, length) = rest
            .split_once(':')
            .ok_or(InvalidContentId::UnsupportedIdentity)?;
        if version != "1" || algorithm != ContentAlgorithm::Sha256.name() || hex.len() != 64 {
            return Err(InvalidContentId::UnsupportedIdentity);
        }
        if !hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(InvalidContentId::UnsupportedIdentity);
        }
        let mut bytes = [0u8; 32];
        for (index, chunk) in hex.as_bytes().chunks(2).enumerate() {
            let pair = std::str::from_utf8(chunk).map_err(|_| InvalidContentId::UnsupportedIdentity)?;
            bytes[index] = u8::from_str_radix(pair, 16)
                .map_err(|_| InvalidContentId::UnsupportedIdentity)?;
        }
        let byte_length = canonical_u64(length).ok_or(InvalidContentId::UnsupportedIdentity)?;
        Ok(Self(dctx_hash::ContentDigest::sha256(bytes, byte_length)))
    }
}

fn canonical_u64(text: &str) -> Option<u64> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    if text.len() > 1 && text.starts_with('0') {
        return None;
    }
    text.parse().ok()
}

impl fmt::Display for ContentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "dctx:object:v{}:{}:",
            self.version(),
            self.algorithm().name()
        )?;
        for byte in self.digest_bytes() {
            write!(f, "{byte:02x}")?;
        }
        write!(f, ":{}", self.byte_length())
    }
}

impl FromStr for ContentId {
    type Err = InvalidContentId;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;
    use super::*;

    #[test]
    fn own_digest_is_tagged_and_untagged_hex_is_refused() {
        let empty = ContentId::of_bytes(b"").unwrap();
        assert_eq!(empty.version(), 1);
        assert_eq!(empty.algorithm(), ContentAlgorithm::Sha256);
        assert_eq!(empty.algorithm().code(), 1);
        assert_eq!(empty.byte_length(), 0);
        assert_eq!(
            empty.to_string(),
            "dctx:object:v1:sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855:0"
        );
        assert_eq!(ContentId::parse(&empty.to_string()).unwrap(), empty);
        let abc = ContentId::of_bytes(b"abc").unwrap();
        assert_eq!(
            abc.digest_bytes()
                .iter()
                .fold(String::new(), |mut text, byte| { write!(text, "{byte:02x}").unwrap(); text }),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert!(abc.to_string().ends_with(":3"));
        let forged = format!("{}:4", abc.to_string().rsplit_once(':').unwrap().0);
        assert_ne!(ContentId::parse(&forged).unwrap(), abc);
        assert_eq!(ContentId::parse(&abc.to_string()).unwrap(), abc);
        for rejected in [
            "da39a3ee5e6b4b0d3255bfef95601890afd80709",
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            "git:da39a3ee5e6b4b0d3255bfef95601890afd80709",
            "dctx:object:v1:sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855:00",
            "dctx:object:v1:sha256:E3B0C44298FC1C149AFBF4C8996FB92427AE41E4649B934CA495991B7852B855:0",
            "dctx:object:v2:sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855:0",
        ] {
            assert_eq!(
                ContentId::parse(rejected),
                Err(InvalidContentId::UnsupportedIdentity),
                "{rejected}"
            );
        }
    }
}
