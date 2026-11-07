// SPDX-License-Identifier: Apache-2.0
//! Content-addressed fact identity for the SQLite comparison bridge.
//! Implements IDENTITY-v1 `FactKey` and decision 078: own dctx-hash digest, no git object id.
//! Depends on [`dctx_map_model::ContentId`]. The blob table and retain tests key payloads with it.
//! A plugin label is not a field and cannot change the key.

use super::StoreError;
use dctx_map_model::{ContentAlgorithm, ContentId};
use std::fmt;

/// Refusal reason when a label is a git object id or a `.git` path.
pub const GIT_IDENTITY: &str = "git identity is not a fact key";

/// Parser and extraction contract bound into a [`FactKey`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractionContract {
    language: u16,
    dialect: String,
    parser_id: String,
    parser_revision: String,
    extraction_digest: String,
    fact_schema: u16,
}

impl ExtractionContract {
    /// Builds a contract. Language and schema must be non-zero. Git-shaped tokens are refused.
    ///
    /// # Errors
    /// Refuses zero language/schema values and empty, oversized, non-token or git-shaped identity fields.
    pub fn new(
        language: u16,
        dialect: impl Into<String>,
        parser_id: impl Into<String>,
        parser_revision: impl Into<String>,
        extraction_digest: impl Into<String>,
        fact_schema: u16,
    ) -> Result<Self, StoreError> {
        let contract = Self {
            language,
            dialect: dialect.into(),
            parser_id: parser_id.into(),
            parser_revision: parser_revision.into(),
            extraction_digest: extraction_digest.into(),
            fact_schema,
        };
        contract.check()?;
        Ok(contract)
    }

    fn check(&self) -> Result<(), StoreError> {
        if self.language == 0 || self.fact_schema == 0 {
            return Err(StoreError::InvalidKey("invalid fact identity"));
        }
        for field in [
            &self.dialect,
            &self.parser_id,
            &self.parser_revision,
            &self.extraction_digest,
        ] {
            if !token(field) {
                return Err(StoreError::InvalidKey(if rejects_git_identity(field) {
                    GIT_IDENTITY
                } else {
                    "invalid fact identity"
                }));
            }
        }
        Ok(())
    }
}

/// One byte string under one extraction contract.
///
/// Equality is the whole contract. The same bytes with a new parser or plan are a new key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FactKey {
    content: ContentId,
    contract: ExtractionContract,
}

impl FactKey {
    /// Hashes `bytes` with dctx-hash SHA-256 and binds `contract`.
    ///
    /// # Errors
    /// Refuses an invalid extraction contract, oversized hash input or inconsistent digest metadata.
    pub fn from_bytes(bytes: &[u8], contract: ExtractionContract) -> Result<Self, StoreError> {
        contract.check()?;
        let digest = dctx_hash::Sha256::digest(bytes)
            .map_err(|_| StoreError::InvalidKey("invalid content digest"))?;
        let content = ContentId::of_bytes(bytes)
            .map_err(|_| StoreError::InvalidKey("invalid content digest"))?;
        let len = u64::try_from(bytes.len())
            .map_err(|_| StoreError::InvalidKey("invalid content digest"))?;
        if content.version() != 1
            || content.algorithm() != ContentAlgorithm::Sha256
            || content.algorithm().code() != dctx_hash::IdentityAlgorithm::SHA256
            || content.digest_bytes() != digest.as_bytes()
            || content.byte_length() != digest.byte_length()
            || content.byte_length() != len
        {
            return Err(StoreError::InvalidKey("invalid content digest"));
        }
        Ok(Self { content, contract })
    }

    /// Checks the tagged digest and the extraction contract.
    ///
    /// # Errors
    /// Refuses unsupported digest tags or an invalid extraction contract.
    pub fn check(&self) -> Result<(), StoreError> {
        if self.content.version() != 1 || self.content.algorithm() != ContentAlgorithm::Sha256 {
            return Err(StoreError::InvalidKey("invalid content digest"));
        }
        self.contract.check()
    }

    /// Algorithm-tagged content digest. This is not a git object id.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn content(&self) -> ContentId {
        self.content
    }
}

impl fmt::Display for FactKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}|{}|{}|{}|{}|{}|{}",
            self.content,
            self.contract.language,
            self.contract.dialect,
            self.contract.parser_id,
            self.contract.parser_revision,
            self.contract.extraction_digest,
            self.contract.fact_schema
        )
    }
}

/// Whether retaining a fact hashed new bytes or reused a stored key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Retain {
    /// The key was absent. The caller parsed, and the parse count increased by one.
    Parsed,
    /// The namespace already retained this key. The payload was not parsed again.
    Reused,
}

pub(crate) fn plain_label(value: &str) -> Result<(), StoreError> {
    if rejects_git_identity(value) {
        return Err(StoreError::InvalidKey(GIT_IDENTITY));
    }
    if value.is_empty() || value.len() > 4096 || value.chars().any(char::is_control) {
        return Err(StoreError::InvalidKey("invalid fact identity"));
    }
    Ok(())
}

pub(crate) fn view_label(value: &str) -> Result<(), StoreError> {
    if rejects_git_identity(value) {
        Err(StoreError::InvalidKey(GIT_IDENTITY))
    } else {
        Ok(())
    }
}

fn token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .chars()
            .all(|c| !c.is_whitespace() && !c.is_control() && c != '|' && c != '/' && c != '\\')
        && !rejects_git_identity(value)
}

fn rejects_git_identity(value: &str) -> bool {
    if value.starts_with("git:")
        || value.contains(".git/")
        || value.split(['/', '\\']).any(|part| part == ".git")
    {
        return true;
    }
    (value.len() == 40 || value.len() == 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contract(revision: &str) -> ExtractionContract {
        ExtractionContract::new(1, "rust-2024", "dctx-map-parse", revision, "plan-1", 1).unwrap()
    }

    #[test]
    fn own_digest_keys_facts_and_bare_hex_is_refused() {
        let key = FactKey::from_bytes(b"abc", contract("rev-1")).unwrap();
        assert!(
            key.to_string()
                .contains("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad:3")
        );
        assert!(key.to_string().starts_with("dctx:object:v1:sha256:"));
        assert_eq!(key.content().byte_length(), 3);
        let empty = FactKey::from_bytes(b"", contract("rev-1")).unwrap();
        assert!(
            empty
                .to_string()
                .contains("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855:0")
        );
        assert!(
            ExtractionContract::new(
                1,
                "rust-2024",
                "dctx-map-parse",
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
                "plan-1",
                1
            )
            .is_err()
        );
        assert!(
            ExtractionContract::new(
                1,
                "rust-2024",
                "dctx-map-parse",
                "rev-1",
                "git:da39a3ee5e6b4b0d3255bfef95601890afd80709",
                1
            )
            .is_err()
        );
        assert!(
            ExtractionContract::new(0, "rust-2024", "dctx-map-parse", "rev-1", "plan-1", 1)
                .is_err()
        );
        assert!(FactKey::from_bytes(b"abc", contract("rev-1")).is_ok());
    }

    #[test]
    fn parser_revision_and_bytes_change_the_key() {
        let left = FactKey::from_bytes(b"fn a() {}", contract("rev-1")).unwrap();
        let right = FactKey::from_bytes(b"fn a() {}", contract("rev-2")).unwrap();
        assert_ne!(left, right);
        assert_ne!(
            left,
            FactKey::from_bytes(b"fn b() {}", contract("rev-1")).unwrap()
        );
        assert_eq!(
            left,
            FactKey::from_bytes(b"fn a() {}", contract("rev-1")).unwrap()
        );
    }
}
