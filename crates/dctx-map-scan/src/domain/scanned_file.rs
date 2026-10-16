//! Model-independent scan records and content hashes.
//! Implements SPEC-MAP-MODE section 13a.
//! Uses SHA-256; consumed by downstream map indexers.

use super::FileHandlingDecision;
use sha2::{Digest, Sha256};
use std::path::PathBuf;

/// SHA-256 digest of exact file bytes.
pub type ContentHash = [u8; 32];
/// Hashes complete authorized content.
pub fn content_hash(bytes: &[u8]) -> ContentHash {
    Sha256::digest(bytes).into()
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
        let hex = |hash: ContentHash| hash.iter().map(|b| format!("{b:02x}")).collect::<String>();
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
