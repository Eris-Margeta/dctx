// SPDX-License-Identifier: Apache-2.0
//! Model-independent scan records and content hashes.
//! Implements SPEC-MAP-MODE section 13a.
//! Depends on dctx-hash SHA-256. Map indexers consume these records.

use super::FileHandlingDecision;
use dctx_hash::{HashError, Sha256};
use std::path::PathBuf;

/// SHA-256 digest of exact file bytes.
pub type ContentHash = [u8; 32];
/// Hashes complete authorized content.
#[allow(
    clippy::must_use_candidate,
    reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
)]
pub fn content_hash(bytes: &[u8]) -> ContentHash {
    match Sha256::digest(bytes) {
        Ok(digest) => *digest.as_bytes(),
        // The fixed public array cannot carry HashError. Only an over-long message arrives here.
        Err(HashError::MessageTooLong) => [0; 32],
    }
}

/// One visible regular file; directories are traversal structure only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScannedFile {
    /// Root-relative path, or basename for a file root.
    pub path: PathBuf,
    /// Final privacy and classification decision.
    pub decision: FileHandlingDecision,
    /// Byte length, using actual bytes when content is read.
    pub size: u64,
    /// Absent if content access was forbidden by rules.
    pub hash: Option<ContentHash>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn known_vectors() {
        let hex = |hash: ContentHash| {
            use std::fmt::Write as _;
            let mut text = String::with_capacity(64);
            for byte in hash {
                write!(text, "{byte:02x}").expect("writing to a String cannot fail");
            }
            text
        };
        assert_eq!(
            hex(content_hash(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hex(content_hash(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_ne!(content_hash(b"abc"), content_hash(b"abd"));
    }
}
