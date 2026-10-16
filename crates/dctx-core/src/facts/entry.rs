//! Source facts: listed entries, their metadata and the read grant that authorizes content.
//! Implements PRODUCT-CONTRACTS C03/P1: a content read needs a [`ReadGrant`], and a grant
//! exists only for an `Include` decision on an item whose ancestors are all `Include`.
//! Depends on [`crate::ids`], [`crate::datatypes`] and [`crate::domain`]. Used by dctx-ports and dctx-pack.

#![deny(missing_docs)]

use crate::datatypes::NodeType;
use crate::domain::FileHandlingDecision;
use crate::ids::{RelativePath, Timestamp};

/// One child of a listed directory: its single-segment name and kind. No metadata, no content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceEntry {
    /// Single path segment.
    pub name: String,
    /// File or directory.
    pub kind: NodeType,
}

impl SourceEntry {
    /// A file entry.
    pub fn file(name: &str) -> Self {
        Self { name: name.to_string(), kind: NodeType::File }
    }

    /// A directory entry.
    pub fn directory(name: &str) -> Self {
        Self { name: name.to_string(), kind: NodeType::Directory }
    }

    /// True for directories.
    pub fn is_dir(&self) -> bool {
        self.kind == NodeType::Directory
    }
}

/// File metadata used for freshness (decision 078: metadata plus the own digest, never VCS state).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EntryMeta {
    /// Size in bytes (0 for directories).
    pub size: u64,
    /// Last modification time.
    pub modified: Timestamp,
}

/// Authorization to read one file's content. Only [`ReadGrant::authorize`] creates one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadGrant {
    path: RelativePath,
}

impl ReadGrant {
    /// Grants a read of `path` iff the item and all its ancestors were decided `Include`
    /// (`ancestors_readable`) and `decision` itself permits reading content.
    pub fn authorize(
        path: &RelativePath,
        decision: FileHandlingDecision,
        ancestors_readable: bool,
    ) -> Option<Self> {
        (ancestors_readable && decision.should_read_content()).then(|| Self { path: path.clone() })
    }

    /// The authorized path.
    pub fn path(&self) -> &RelativePath {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_include_under_include_ancestors_is_granted() {
        let path = RelativePath::parse("src/a.rs").unwrap();
        let grant = ReadGrant::authorize(&path, FileHandlingDecision::Include, true).unwrap();
        assert_eq!(grant.path(), &path);
        for denied in [
            FileHandlingDecision::Exclude,
            FileHandlingDecision::DebugVisible,
            FileHandlingDecision::OmitContent,
        ] {
            assert!(ReadGrant::authorize(&path, denied, true).is_none(), "{denied:?}");
        }
        assert!(ReadGrant::authorize(&path, FileHandlingDecision::Include, false).is_none());
    }

    #[test]
    fn entries_know_their_kind() {
        assert!(SourceEntry::directory("src").is_dir());
        assert!(!SourceEntry::file("a.rs").is_dir());
    }
}
