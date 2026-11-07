// SPDX-License-Identifier: Apache-2.0
//! Content identity helpers for the facade.
//! Implements contracts/API-PORTS-v1.md identity goldens and decision 077 (own SHA-256, never a git id).
//! Depends on dctx-hash. Pack, map and `object_key` all use this digest.
//! The textual key is `dctx:object:` plus the hash crate's `v1:sha256:…:length` form.

use dctx_hash::Sha256;

use super::error::{DctxError, Resource};

/// Parser id recorded on strict structural answers.
/// Revision 3 fixes BOM dispatch and Markdown section spans and retains named
/// local functions in Kotlin, Swift and C#. Older cached graphs must rebuild.
pub const STRUCTURAL_PARSER: &str = "dctx-structural/3";

/// Algorithm-tagged object key of `bytes`.
///
/// The core does not accept a git object id. Callers that need one use a plugin and get
/// [`DctxError::UnsupportedCapability`].
/// # Errors
/// Returns a resource-limit error if the input exceeds the SHA-256 message length limit.
pub fn object_key(bytes: &[u8]) -> Result<String, DctxError> {
    let digest = Sha256::digest(bytes).map_err(|_| DctxError::ResourceLimit {
        resource: Resource::FileBytes,
        limit: u64::MAX,
    })?;
    Ok(format!("dctx:object:{digest}"))
}

/// Hashes `bytes` and returns the hash crate's display form.
pub(crate) fn digest_text(bytes: &[u8]) -> Result<dctx_hash::ContentDigest, DctxError> {
    Sha256::digest(bytes).map_err(|_| DctxError::ResourceLimit {
        resource: Resource::FileBytes,
        limit: u64::MAX,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_goldens_match_the_contract() {
        let empty = object_key(b"").expect("empty");
        assert_eq!(
            empty,
            "dctx:object:v1:sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855:0"
        );
        let abc = object_key(b"abc").expect("abc");
        assert_eq!(
            abc,
            "dctx:object:v1:sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad:3"
        );
        assert_ne!(empty, abc);
    }
}
