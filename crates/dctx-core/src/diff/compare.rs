// SPDX-License-Identifier: Apache-2.0
//! Compares two snapshot trees into a [`super::DiffReport`].
//! Implements pack diff metadata rules (size and modification time, decision 078; no VCS state).
//! Depends on [`super::walk`] and [`crate::datatypes`]; pure, no IO.
//! Re-exported as [`super::compare_trees`] for dctx-cli and dctx-pack.
//! A missing old tree marks every new node added.

use super::DiffReport;
use crate::datatypes::DirContextTreeNode;

/// Uspoređuje dva stabla i generira izvještaj.
/// `old_root` može biti None (u slučaju prvog run-a, sve je Added).
#[allow(
    clippy::must_use_candidate,
    reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
)]
pub fn compare_trees(
    old_root: Option<&DirContextTreeNode>,
    new_root: &DirContextTreeNode,
) -> DiffReport {
    let mut report = DiffReport::default();

    match old_root {
        Some(old) => {
            super::walk::compare_nodes_recursive(old, new_root, &mut report);
        }
        None => {
            // Ako nema starog stabla, sve u novom je ADDED
            super::walk::mark_all_as_added_recursive(new_root, &mut report);
        }
    }

    report.has_changes = !report.entries.is_empty();

    // Izračunaj total_files_in_new_state (samo files, ne dirs)
    report.stats.total_files_in_new_state = super::walk::count_files_recursive(new_root);

    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datatypes::NodeType;
    use std::path::PathBuf;

    fn create_file_node(path: &str, size: u64, timestamp: u64) -> DirContextTreeNode {
        let mut node =
            DirContextTreeNode::new(NodeType::File, path, PathBuf::from(format!("/test/{path}")));
        node.content_size = size;
        node.last_modified_timestamp = timestamp;
        node
    }

    fn create_dir_node(path: &str) -> DirContextTreeNode {
        DirContextTreeNode::new(
            NodeType::Directory,
            path,
            PathBuf::from(format!("/test/{path}")),
        )
    }

    // --- COMPARE_TREES TESTS ---

    #[test]
    fn test_compare_trees_no_old_tree() {
        let mut root = create_dir_node("");
        root.children.push(create_file_node("file1.txt", 10, 100));
        root.children.push(create_file_node("file2.txt", 20, 200));

        let report = compare_trees(None, &root);

        assert!(report.has_changes);
        assert_eq!(report.stats.files_added, 2);
        assert_eq!(report.stats.files_removed, 0);
        assert_eq!(report.stats.files_modified, 0);
        assert_eq!(report.stats.files_unchanged, 0);
        assert_eq!(report.stats.total_files_in_new_state, 2);
        assert_eq!(report.entries.len(), 3); // Root dir + 2 files
    }

    #[test]
    fn test_compare_trees_no_changes() {
        let mut old_root = create_dir_node("");
        old_root
            .children
            .push(create_file_node("file.txt", 100, 1000));

        let mut new_root = create_dir_node("");
        new_root
            .children
            .push(create_file_node("file.txt", 100, 1000));

        let report = compare_trees(Some(&old_root), &new_root);

        assert!(!report.has_changes);
        assert_eq!(report.stats.files_added, 0);
        assert_eq!(report.stats.files_removed, 0);
        assert_eq!(report.stats.files_modified, 0);
        assert_eq!(report.stats.files_unchanged, 1);
    }

    #[test]
    fn test_compare_trees_file_added() {
        let mut old_root = create_dir_node("");
        old_root
            .children
            .push(create_file_node("existing.txt", 100, 1000));

        let mut new_root = create_dir_node("");
        new_root
            .children
            .push(create_file_node("existing.txt", 100, 1000));
        new_root
            .children
            .push(create_file_node("new.txt", 50, 2000));

        let report = compare_trees(Some(&old_root), &new_root);

        assert!(report.has_changes);
        assert_eq!(report.stats.files_added, 1);
        assert_eq!(report.stats.files_unchanged, 1);
        assert_eq!(report.stats.total_files_in_new_state, 2);
    }

    #[test]
    fn test_compare_trees_file_removed() {
        let mut old_root = create_dir_node("");
        old_root
            .children
            .push(create_file_node("keep.txt", 100, 1000));
        old_root
            .children
            .push(create_file_node("delete.txt", 50, 1000));

        let mut new_root = create_dir_node("");
        new_root
            .children
            .push(create_file_node("keep.txt", 100, 1000));

        let report = compare_trees(Some(&old_root), &new_root);

        assert!(report.has_changes);
        assert_eq!(report.stats.files_removed, 1);
        assert_eq!(report.stats.files_unchanged, 1);
        assert_eq!(report.stats.total_files_in_new_state, 1);
    }

    #[test]
    fn test_compare_trees_file_modified() {
        let mut old_root = create_dir_node("");
        old_root
            .children
            .push(create_file_node("file.txt", 100, 1000));

        let mut new_root = create_dir_node("");
        new_root
            .children
            .push(create_file_node("file.txt", 150, 2000));

        let report = compare_trees(Some(&old_root), &new_root);

        assert!(report.has_changes);
        assert_eq!(report.stats.files_modified, 1);
        assert_eq!(report.stats.files_unchanged, 0);
    }

    #[test]
    fn test_compare_trees_nested_directories() {
        let mut old_root = create_dir_node("");
        let mut old_src = create_dir_node("src");
        old_src
            .children
            .push(create_file_node("src/main.rs", 100, 1000));
        old_root.children.push(old_src);

        let mut new_root = create_dir_node("");
        let mut new_src = create_dir_node("src");
        new_src
            .children
            .push(create_file_node("src/main.rs", 100, 1000));
        new_src
            .children
            .push(create_file_node("src/lib.rs", 50, 2000));
        new_root.children.push(new_src);

        let report = compare_trees(Some(&old_root), &new_root);

        assert!(report.has_changes);
        assert_eq!(report.stats.files_added, 1);
        assert_eq!(report.stats.files_unchanged, 1);
    }

    #[test]
    fn test_compare_trees_directory_added() {
        let old_root = create_dir_node("");

        let mut new_root = create_dir_node("");
        let mut new_dir = create_dir_node("newdir");
        new_dir
            .children
            .push(create_file_node("newdir/file.txt", 100, 1000));
        new_root.children.push(new_dir);

        let report = compare_trees(Some(&old_root), &new_root);

        assert!(report.has_changes);
        assert_eq!(report.stats.files_added, 1);
        // The directory itself is also in entries
        assert!(report.entries.iter().any(|e| e.relative_path == "newdir"));
    }

    #[test]
    fn test_compare_trees_directory_removed() {
        let mut old_root = create_dir_node("");
        let mut old_dir = create_dir_node("olddir");
        old_dir
            .children
            .push(create_file_node("olddir/file.txt", 100, 1000));
        old_root.children.push(old_dir);

        let new_root = create_dir_node("");

        let report = compare_trees(Some(&old_root), &new_root);

        assert!(report.has_changes);
        assert_eq!(report.stats.files_removed, 1);
    }

    #[test]
    fn test_compare_trees_mixed_changes() {
        let mut old_root = create_dir_node("");
        old_root
            .children
            .push(create_file_node("unchanged.txt", 100, 1000));
        old_root
            .children
            .push(create_file_node("modified.txt", 50, 1000));
        old_root
            .children
            .push(create_file_node("deleted.txt", 30, 1000));

        let mut new_root = create_dir_node("");
        new_root
            .children
            .push(create_file_node("unchanged.txt", 100, 1000));
        new_root
            .children
            .push(create_file_node("modified.txt", 75, 2000));
        new_root
            .children
            .push(create_file_node("added.txt", 25, 3000));

        let report = compare_trees(Some(&old_root), &new_root);

        assert!(report.has_changes);
        assert_eq!(report.stats.files_added, 1);
        assert_eq!(report.stats.files_removed, 1);
        assert_eq!(report.stats.files_modified, 1);
        assert_eq!(report.stats.files_unchanged, 1);
        assert_eq!(report.stats.total_files_in_new_state, 3);
    }
}
