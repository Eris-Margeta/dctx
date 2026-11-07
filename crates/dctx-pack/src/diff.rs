// SPDX-License-Identifier: Apache-2.0
//! Snapshot diff: compares a new scan with the previous snapshot and picks the next version.
//! Implements the CLI's diff-and-versioning step over facts: no previous snapshot is V1 with an
//! empty report; otherwise `compare_trees` (size/mtime) then `version_after_diff`.
//! Depends on dctx-core `diff` and `version`. Used by [`crate::engine`].

use dctx_core::datatypes::DirContextTreeNode;
use dctx_core::diff::{compare_trees, DiffReport};
use dctx_core::facts::PackSnapshot;
use dctx_core::version::{version_after_diff, Version};

/// The change between two snapshots and the version the new one gets.
#[derive(Debug)]
pub struct VersionedDiff {
    /// Changes; empty when there is no previous snapshot.
    pub report: DiffReport,
    /// Version of the new snapshot.
    pub version: Version,
    /// True when the version is a major reset (old diffs are obsolete).
    pub major_reset: bool,
}

/// Diffs `tree` against `previous` and decides the new version.
pub fn diff_against(
    previous: Option<&PackSnapshot>,
    tree: &DirContextTreeNode,
    max_diff_files: u32,
) -> VersionedDiff {
    let report = match previous {
        Some(old) => compare_trees(Some(old.tree()), tree),
        None => DiffReport::default(),
    };
    let (version, major_reset) =
        version_after_diff(previous.map(PackSnapshot::version), &report, max_diff_files);
    VersionedDiff {
        report,
        version,
        major_reset,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dctx_core::datatypes::NodeType;
    use dctx_core::diff::ChangeType;
    use dctx_core::facts::ContentDigests;
    use dctx_core::ids::Timestamp;
    use std::path::PathBuf;

    fn tree(files: &[(&str, u64)]) -> DirContextTreeNode {
        let mut root = DirContextTreeNode::new(NodeType::Directory, "", PathBuf::new());
        for (path, size) in files {
            let mut file = DirContextTreeNode::new(NodeType::File, path, PathBuf::from(path));
            file.content_size = *size;
            root.children.push(file);
        }
        root
    }

    fn snapshot(files: &[(&str, u64)], version: Version) -> PackSnapshot {
        PackSnapshot::new(tree(files), version, Timestamp(0), ContentDigests::new()).unwrap()
    }

    #[test]
    fn first_snapshot_is_v1_with_reset_and_no_entries() {
        let diff = diff_against(None, &tree(&[("a", 1)]), 999);
        assert_eq!(diff.version, Version::new_v1());
        assert!(diff.major_reset);
        assert!(!diff.report.has_changes);
    }

    #[test]
    fn unchanged_keeps_version_and_changes_bump_minor() {
        let previous = snapshot(&[("a", 1), ("b", 2)], Version { major: 1, minor: 2 });
        let same = diff_against(Some(&previous), &tree(&[("a", 1), ("b", 2)]), 999);
        assert_eq!(same.version, Version { major: 1, minor: 2 });
        assert!(!same.major_reset);
        let changed = diff_against(Some(&previous), &tree(&[("a", 1), ("b", 3)]), 999);
        assert_eq!(changed.version, Version { major: 1, minor: 3 });
        assert_eq!(changed.report.entries[0].change_type, ChangeType::Modified);
    }

    #[test]
    fn massive_change_and_limit_force_a_major_reset() {
        let previous = snapshot(&[("a", 1)], Version { major: 1, minor: 2 });
        let massive = diff_against(Some(&previous), &tree(&[("a", 9)]), 999);
        assert_eq!(
            (massive.version, massive.major_reset),
            (Version { major: 2, minor: 0 }, true)
        );
        let previous = snapshot(&[("a", 1), ("b", 1)], Version { major: 1, minor: 2 });
        let limited = diff_against(Some(&previous), &tree(&[("a", 1), ("b", 2)]), 2);
        assert_eq!(
            (limited.version, limited.major_reset),
            (Version { major: 2, minor: 0 }, true)
        );
    }
}
