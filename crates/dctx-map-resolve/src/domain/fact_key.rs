//! Content-addressed fact identity for contextual resolution.
//! Implements IDENTITY-v1 `FactKey` and decision 078: own digest, no git object id.
//! Depends on the model content id. Views and the fact store key payloads with it.
use super::ResolveError;
use dctx_map_model::{ContentAlgorithm, ContentId};
use std::cmp::Ordering;
use std::fmt;

/// Parser and extraction contract bound into a [`FactKey`].
/// A plugin label is not a field and cannot change the key.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ExtractionContract {
    /// IDENTITY-v1 language tag. Zero is invalid. Rust is 1.
    pub language: u16,
    /// Dialect such as a Rust edition or JSX. Not a VCS revision.
    pub dialect: String,
    /// Extractor name, for example `dctx-map-parse`.
    pub parser_id: String,
    /// Extractor and grammar revision. Changing it is a new key.
    pub parser_revision: String,
    /// Digest of the extraction plan. Not a git object id.
    pub extraction_digest: String,
    /// Fact schema version. Zero is invalid.
    pub fact_schema: u16,
}

impl ExtractionContract {
    /// Rejects an empty contract, schema zero, and git-shaped tokens.
    ///
    /// # Errors
    /// Refuses unsupported digest metadata or zero, empty, non-token or git-shaped extraction fields.
    pub fn check(&self) -> Result<(), ResolveError> {
        if self.language == 0 || self.fact_schema == 0 {
            return Err(ResolveError::InvalidScope("extraction".into()));
        }
        for field in [
            &self.dialect,
            &self.parser_id,
            &self.parser_revision,
            &self.extraction_digest,
        ] {
            if !identity_token(field) {
                return Err(ResolveError::InvalidScope(field.clone()));
            }
        }
        Ok(())
    }
}

/// One byte string under one extraction contract.
/// Equality is the whole contract. Plugin output is not an input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FactKey {
    content: ContentId,
    contract: ExtractionContract,
}

impl FactKey {
    /// Builds a key from an own content digest and a checked contract.
    ///
    /// # Errors
    /// Refuses unsupported content-digest metadata or an invalid extraction contract.
    pub fn new(content: ContentId, contract: ExtractionContract) -> Result<Self, ResolveError> {
        let key = Self { content, contract };
        key.check()?;
        Ok(key)
    }

    /// Checks the tagged digest and the extraction contract.
    ///
    /// # Errors
    /// Refuses unsupported digest metadata or zero, empty, non-token or git-shaped extraction fields.
    pub fn check(&self) -> Result<(), ResolveError> {
        if self.content.version() != 1 || self.content.algorithm() != ContentAlgorithm::Sha256 {
            return Err(ResolveError::InvalidRoot(self.content.to_string()));
        }
        self.contract.check()
    }

    /// Algorithm-tagged content digest. This is not a git object id.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn content(&self) -> ContentId {
        self.content
    }

    /// Extraction contract that distinguishes otherwise equal bytes.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn contract(&self) -> &ExtractionContract {
        &self.contract
    }
}

impl PartialOrd for FactKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for FactKey {
    fn cmp(&self, other: &Self) -> Ordering {
        self.content
            .version()
            .cmp(&other.content.version())
            .then(self.content.algorithm().code().cmp(&other.content.algorithm().code()))
            .then(self.content.digest_bytes().cmp(other.content.digest_bytes()))
            .then(self.content.byte_length().cmp(&other.content.byte_length()))
            .then(self.contract.cmp(&other.contract))
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

pub(crate) fn identity_token(value: &str) -> bool {
    !value.is_empty()
        && !value.chars().any(|c| c.is_whitespace() || c.is_control())
        && !rejects_git_identity(value)
}

/// True for untagged 40- or 64-hex strings, `git:` prefixes, and `.git` paths.
pub(crate) fn rejects_git_identity(value: &str) -> bool {
    let bare = value.strip_prefix("git:").unwrap_or(value);
    let hex_object = (bare.len() == 40 || bare.len() == 64)
        && bare.bytes().all(|byte| byte.is_ascii_hexdigit())
        && !value.starts_with("dctx:");
    hex_object
        || value.split(['/', '\\']).any(|part| part == ".git")
        || value.contains(".git/")
}

pub(crate) fn portable_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains(['\\', ':', '~'])
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != ".." && part != ".git")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contract(revision: &str, plan: &str) -> ExtractionContract {
        ExtractionContract {
            language: 1,
            dialect: "rust-2024".into(),
            parser_id: "dctx-map-parse".into(),
            parser_revision: revision.into(),
            extraction_digest: plan.into(),
            fact_schema: 1,
        }
    }

    #[test]
    fn fact_key_uses_the_own_digest_and_refuses_git_object_ids() {
        let bytes = b"fn entry() {}";
        let content = ContentId::of_bytes(bytes).unwrap();
        let key = FactKey::new(content, contract("p1", "plan-1")).unwrap();
        assert!(key.to_string().starts_with("dctx:object:v1:sha256:"));
        assert_eq!(key.content().byte_length(), bytes.len() as u64);
        let again = FactKey::new(ContentId::of_bytes(bytes).unwrap(), contract("p1", "plan-1")).unwrap();
        assert_eq!(key, again);
        assert_ne!(
            key,
            FactKey::new(content, contract("p2", "plan-1")).unwrap()
        );
        assert_ne!(
            key,
            FactKey::new(content, contract("p1", "plan-2")).unwrap()
        );
        for rejected in [
            "da39a3ee5e6b4b0d3255bfef95601890afd80709",
            "git:da39a3ee5e6b4b0d3255bfef95601890afd80709",
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        ] {
            assert!(ContentId::parse(rejected).is_err(), "{rejected}");
            assert!(rejects_git_identity(rejected), "{rejected}");
        }
        assert!(FactKey::new(content, contract("git:da39a3ee5e6b4b0d3255bfef95601890afd80709", "plan")).is_err());
        assert!(portable_path("src/lib.rs"));
        assert!(!portable_path(".git/HEAD"));
        assert!(!portable_path("src/.git/config"));
    }
}
