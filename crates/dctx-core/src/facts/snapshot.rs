// SPDX-License-Identifier: Apache-2.0
//! Pack snapshot facts: the selected tree, its version, content digests and its own identity.
//! Implements decision 077/decision 078 snapshot identity: SHA-256 (dctx-hash) over a canonical encoding of
//! paths, kinds, decisions, metadata and content digests; no VCS id, no clock in the digest.
//! Depends on [`crate::datatypes`], [`crate::ids`], [`crate::version`] and `dctx-hash`.
//! Used by dctx-ports (`FactStore`) and dctx-pack (snapshot and diff engine).

#![deny(missing_docs)]

use crate::datatypes::{DirContextTreeNode, NodeType};
use crate::domain::FileHandlingDecision;
use crate::error::{DctxError, Resource};
use crate::ids::{ContentDigest, SnapshotDigest, Timestamp};
use crate::version::Version;
use dctx_hash::Sha256;
use std::collections::BTreeMap;

/// Content digests of the files whose bytes were read, keyed by relative path.
pub type ContentDigests = BTreeMap<String, ContentDigest>;

/// A non-fatal observation made while taking a snapshot. Paths are only ever of visible items.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Diagnostic {
    /// A visible directory could not be listed; it is recorded without children.
    UnlistableDirectory {
        /// Directory path.
        path: String,
    },
    /// Metadata of a visible item could not be read; size and time are zero.
    MissingMetadata {
        /// Item path.
        path: String,
    },
    /// A readable file could not be read; it is recorded without content.
    UnreadableFile {
        /// File path.
        path: String,
    },
    /// A listed name is not a valid path segment (for example it holds `\`); it is skipped.
    UnrepresentableName {
        /// Directory that listed it.
        parent: String,
    },
    /// A readable file exceeded the per-file limit and was not read.
    ContentWithheld {
        /// File path.
        path: String,
        /// Size reported by its metadata.
        size: u64,
    },
}

/// One taken snapshot of a pack view.
#[derive(Debug, Clone)]
pub struct PackSnapshot {
    tree: DirContextTreeNode,
    version: Version,
    taken_at: Timestamp,
    digests: ContentDigests,
    digest: SnapshotDigest,
}

impl PackSnapshot {
    /// Assembles a snapshot and computes its identity from `tree` and `digests`.
    /// # Errors
    /// Returns a resource-limit error if canonical snapshot bytes exceed the SHA-256 message length limit.
    pub fn new(
        tree: DirContextTreeNode,
        version: Version,
        taken_at: Timestamp,
        digests: ContentDigests,
    ) -> Result<Self, DctxError> {
        let digest = snapshot_digest(&tree, &digests)?;
        Ok(Self {
            tree,
            version,
            taken_at,
            digests,
            digest,
        })
    }

    /// The selected tree.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn tree(&self) -> &DirContextTreeNode {
        &self.tree
    }

    /// Consumes the snapshot, returning its tree.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn into_tree(self) -> DirContextTreeNode {
        self.tree
    }

    /// Pack version of this snapshot.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn version(&self) -> &Version {
        &self.version
    }

    /// When the snapshot was taken (not part of its identity).
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn taken_at(&self) -> Timestamp {
        self.taken_at
    }

    /// Content digests of read files.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn digests(&self) -> &ContentDigests {
        &self.digests
    }

    /// Own identity of this snapshot.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn digest(&self) -> SnapshotDigest {
        self.digest
    }
}

/// Canonical digest of a tree and its content digests. Equal facts give equal digests,
/// independent of the clock. Children are encoded in their stored (sorted) order.
/// # Errors
/// Returns a resource-limit error if canonical snapshot bytes exceed the SHA-256 message length limit.
pub fn snapshot_digest(
    tree: &DirContextTreeNode,
    digests: &ContentDigests,
) -> Result<SnapshotDigest, DctxError> {
    let mut hasher = Sha256::new();
    let mut stack = vec![tree];
    while let Some(node) = stack.pop() {
        encode_node(&mut hasher, node, digests.get(&node.relative_path))?;
        stack.extend(node.children.iter().rev());
    }
    Ok(SnapshotDigest(hasher.finalize()))
}

fn encode_node(
    hasher: &mut Sha256,
    node: &DirContextTreeNode,
    content: Option<&ContentDigest>,
) -> Result<(), DctxError> {
    let kind = match node.node_type {
        NodeType::File => b'F',
        NodeType::Directory => b'D',
    };
    let decision = match node.handling_decision {
        FileHandlingDecision::Include => 0u8,
        FileHandlingDecision::Exclude => 1,
        FileHandlingDecision::DebugVisible => 2,
        FileHandlingDecision::OmitContent => 3,
    };
    let path = node.relative_path.as_bytes();
    feed(hasher, &[kind, decision, u8::from(node.is_ignored)])?;
    feed(hasher, &(path.len() as u64).to_be_bytes())?;
    feed(hasher, path)?;
    feed(hasher, &node.content_size.to_be_bytes())?;
    feed(hasher, &node.last_modified_timestamp.to_be_bytes())?;
    match content {
        Some(digest) => {
            feed(hasher, &[1, digest.version()])?;
            feed(hasher, &digest.algorithm_code().to_be_bytes())?;
            feed(hasher, digest.as_bytes())?;
            feed(hasher, &digest.byte_length().to_be_bytes())
        }
        None => feed(hasher, &[0]),
    }
}

fn feed(hasher: &mut Sha256, bytes: &[u8]) -> Result<(), DctxError> {
    hasher.update(bytes).map_err(|_| DctxError::ResourceLimit {
        resource: Resource::SourceBytes,
        limit: u64::MAX,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tree(size: u64) -> DirContextTreeNode {
        let mut root = DirContextTreeNode::new(NodeType::Directory, "", PathBuf::new());
        let mut file = DirContextTreeNode::new(NodeType::File, "a.txt", PathBuf::from("a.txt"));
        file.content_size = size;
        root.children.push(file);
        root
    }

    fn digests(bytes: &[u8]) -> ContentDigests {
        let mut map = ContentDigests::new();
        map.insert("a.txt".to_string(), Sha256::digest(bytes).unwrap());
        map
    }

    #[test]
    fn identity_is_deterministic_and_ignores_the_clock() {
        let a =
            PackSnapshot::new(tree(3), Version::new_v1(), Timestamp(1), digests(b"abc")).unwrap();
        let b =
            PackSnapshot::new(tree(3), Version::new_v1(), Timestamp(99), digests(b"abc")).unwrap();
        assert_eq!(a.digest(), b.digest());
        assert_eq!(a.taken_at(), Timestamp(1));
        assert_eq!(a.digests().len(), 1);
    }

    #[test]
    fn identity_follows_content_even_with_equal_metadata() {
        let a = snapshot_digest(&tree(3), &digests(b"abc")).unwrap();
        let b = snapshot_digest(&tree(3), &digests(b"abd")).unwrap();
        let unread = snapshot_digest(&tree(3), &ContentDigests::new()).unwrap();
        assert_ne!(a, b);
        assert_ne!(a, unread);
    }

    #[test]
    fn identity_follows_metadata_and_decisions() {
        let base = snapshot_digest(&tree(3), &ContentDigests::new()).unwrap();
        assert_ne!(
            base,
            snapshot_digest(&tree(4), &ContentDigests::new()).unwrap()
        );
        let mut omitted = tree(3);
        omitted.children[0].handling_decision = FileHandlingDecision::OmitContent;
        assert_ne!(
            base,
            snapshot_digest(&omitted, &ContentDigests::new()).unwrap()
        );
    }
}
