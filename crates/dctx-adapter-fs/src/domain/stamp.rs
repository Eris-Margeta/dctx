// SPDX-License-Identifier: Apache-2.0
//! Metadata pre-filter identity for one file.
//! Implements DX-B3 freshness: size, mtime, and inode where the platform has one.
//! Depends on no IO. Strict mode still hashes when this stamp matches.

/// Observable file identity that does not include bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileStamp {
    /// Length in bytes from directory metadata.
    pub size: u64,
    /// Modification time as nanoseconds since the Unix epoch.
    pub modified_ns: u128,
    /// Inode or file index when the filesystem provides one.
    pub inode: Option<u64>,
}

impl FileStamp {
    /// Builds a stamp from metadata the adapter has already read.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn new(size: u64, modified_ns: u128, inode: Option<u64>) -> Self {
        Self {
            size,
            modified_ns,
            inode,
        }
    }

    /// Compares the pre-filter fields.
    ///
    /// A missing inode does not by itself count as a change. Two present inodes must be equal.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn metadata_matches(&self, previous: &Self) -> bool {
        if self.size != previous.size || self.modified_ns != previous.modified_ns {
            return false;
        }
        match (self.inode, previous.inode) {
            (Some(left), Some(right)) => left == right,
            _ => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stamp(size: u64, modified: u128, inode: Option<u64>) -> FileStamp {
        FileStamp::new(size, modified, inode)
    }

    #[test]
    fn metadata_match_uses_size_mtime_and_present_inodes() {
        let previous = stamp(4, 10, Some(7));
        assert!(stamp(4, 10, Some(7)).metadata_matches(&previous));
        assert!(!stamp(5, 10, Some(7)).metadata_matches(&previous));
        assert!(!stamp(4, 11, Some(7)).metadata_matches(&previous));
        assert!(!stamp(4, 10, Some(8)).metadata_matches(&previous));
        assert!(stamp(4, 10, None).metadata_matches(&previous));
    }
}
