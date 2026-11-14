// SPDX-License-Identifier: Apache-2.0
//! Iterative snapshot walks for counts, added/removed subtrees and comparison.
//! Child identity is the relative path; duplicate occurrences pair in stored order.
//! Depends on [`super`] and [`crate::datatypes`]; pure, no IO.
//! Used by [`super::compare::compare_trees`].
//! A file changes when its size differs, or when its modification time differs and its bytes do.

use super::{ChangeType, DiffEntry, DiffReport};
use crate::datatypes::{DirContextTreeNode, NodeType};
use std::collections::{BTreeMap, VecDeque};

pub(super) fn count_files_recursive(node: &DirContextTreeNode) -> usize {
    let mut count = 0;
    let mut stack = vec![node];
    while let Some(node) = stack.pop() {
        if node.node_type == NodeType::File {
            count += 1;
        } else {
            stack.extend(&node.children);
        }
    }
    count
}

pub(super) fn mark_all_as_added_recursive(node: &DirContextTreeNode, report: &mut DiffReport) {
    mark_subtree(node, ChangeType::Added, report);
}

pub(super) fn mark_all_as_removed_recursive(node: &DirContextTreeNode, report: &mut DiffReport) {
    mark_subtree(node, ChangeType::Removed, report);
}

fn mark_subtree(node: &DirContextTreeNode, change_type: ChangeType, report: &mut DiffReport) {
    let mut stack = vec![node];
    while let Some(node) = stack.pop() {
        report.entries.push(DiffEntry {
            change_type,
            node_type: node.node_type,
            relative_path: node.relative_path.clone(),
        });
        if node.node_type == NodeType::File {
            match change_type {
                ChangeType::Added => report.stats.files_added += 1,
                ChangeType::Removed => report.stats.files_removed += 1,
                ChangeType::Modified => unreachable!("subtree walks only add or remove"),
            }
        } else {
            stack.extend(node.children.iter().rev());
        }
    }
}

enum Visit<'a> {
    Compare(&'a DirContextTreeNode, &'a DirContextTreeNode),
    Added(&'a DirContextTreeNode),
    Removed(&'a DirContextTreeNode),
}

pub(super) fn compare_nodes_recursive(
    old_node: &DirContextTreeNode,
    new_node: &DirContextTreeNode,
    report: &mut DiffReport,
) {
    let mut stack = vec![Visit::Compare(old_node, new_node)];
    while let Some(visit) = stack.pop() {
        let (old_node, new_node) = match visit {
            Visit::Compare(old, new) => (old, new),
            Visit::Added(node) => {
                mark_all_as_added_recursive(node, report);
                continue;
            }
            Visit::Removed(node) => {
                mark_all_as_removed_recursive(node, report);
                continue;
            }
        };
        if old_node.node_type != new_node.node_type {
            mark_all_as_removed_recursive(old_node, report);
            mark_all_as_added_recursive(new_node, report);
        } else if old_node.node_type == NodeType::File {
            if is_file_modified(old_node, new_node) {
                report.entries.push(DiffEntry {
                    change_type: ChangeType::Modified,
                    node_type: NodeType::File,
                    relative_path: new_node.relative_path.clone(),
                });
                report.stats.files_modified += 1;
            } else {
                report.stats.files_unchanged += 1;
            }
        } else {
            // Ordered keys make removals deterministic. Queues preserve every
            // occurrence of a duplicate instead of silently replacing one.
            let mut old_children: BTreeMap<&String, VecDeque<&DirContextTreeNode>> =
                BTreeMap::new();
            for child in &old_node.children {
                old_children
                    .entry(&child.relative_path)
                    .or_default()
                    .push_back(child);
            }
            let mut visits = Vec::new();
            for child in &new_node.children {
                let old = old_children
                    .get_mut(&child.relative_path)
                    .and_then(VecDeque::pop_front);
                visits.push(match old {
                    Some(old) => Visit::Compare(old, child),
                    None => Visit::Added(child),
                });
            }
            for old in old_children.into_values().flatten() {
                visits.push(Visit::Removed(old));
            }
            stack.extend(visits.into_iter().rev());
        }
    }
}

/// Logika za detekciju promjene sadržaja.
/// Size + Timestamp; a touch-only change (same size, other timestamp) is settled by the bytes when both nodes carry the whole file.
/// Without bytes on either side the timestamp decides, so state files written without bodies keep their old meaning.
fn is_file_modified(old: &DirContextTreeNode, new: &DirContextTreeNode) -> bool {
    // 1. Provjera veličine
    if old.content_size != new.content_size {
        return true;
    }

    // 2. Provjera vremena modifikacije
    // Napomena: Ovo može biti nepouzdano ako se kopira na drugi FS.
    // Ali za lokalni diff je standard.
    if old.last_modified_timestamp != new.last_modified_timestamp {
        // Same size, other timestamp: equal whole bodies mean the file was only touched.
        return match (whole_body(old), whole_body(new)) {
            (Some(before), Some(after)) => before != after,
            _ => true,
        };
    }

    false
}

/// The node's bytes when they are the complete file; an omitted or partial body is no evidence.
fn whole_body(node: &DirContextTreeNode) -> Option<&[u8]> {
    node.file_content
        .as_deref()
        .filter(|bytes| u64::try_from(bytes.len()).is_ok_and(|len| len == node.content_size))
}

#[cfg(test)]
mod tests {
    use super::*;
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

    // --- IS_FILE_MODIFIED TESTS ---

    #[test]
    fn test_is_file_modified_same() {
        let old = create_file_node("test.txt", 100, 1000);
        let new = create_file_node("test.txt", 100, 1000);
        assert!(!is_file_modified(&old, &new));
    }

    #[test]
    fn test_is_file_modified_size_changed() {
        let old = create_file_node("test.txt", 100, 1000);
        let new = create_file_node("test.txt", 150, 1000);
        assert!(is_file_modified(&old, &new));
    }

    #[test]
    fn test_is_file_modified_timestamp_changed() {
        let old = create_file_node("test.txt", 100, 1000);
        let new = create_file_node("test.txt", 100, 2000);
        assert!(is_file_modified(&old, &new));
    }

    #[test]
    fn test_is_file_modified_both_changed() {
        let old = create_file_node("test.txt", 100, 1000);
        let new = create_file_node("test.txt", 200, 2000);
        assert!(is_file_modified(&old, &new));
    }

    fn with_body(mut node: DirContextTreeNode, body: &[u8]) -> DirContextTreeNode {
        node.content_size = body.len() as u64;
        node.file_content = Some(body.to_vec());
        node
    }

    #[test]
    fn touch_only_with_equal_bodies_is_not_modified() {
        let old = with_body(create_file_node("a.txt", 0, 1000), b"same bytes");
        let new = with_body(create_file_node("a.txt", 0, 2000), b"same bytes");
        assert!(!is_file_modified(&old, &new));
    }

    #[test]
    fn same_size_edit_with_other_timestamp_is_modified() {
        let old = with_body(create_file_node("a.txt", 0, 1000), b"same bytes");
        let new = with_body(create_file_node("a.txt", 0, 2000), b"samx bytes");
        assert!(is_file_modified(&old, &new));
    }

    #[test]
    fn a_missing_or_partial_body_keeps_the_timestamp_rule() {
        let full = with_body(create_file_node("a.txt", 0, 1000), b"same bytes");
        let bare = create_file_node("a.txt", 10, 2000);
        assert!(is_file_modified(&full, &bare));
        assert!(is_file_modified(&bare, &full));
        let mut omitted = with_body(create_file_node("a.txt", 0, 2000), b"");
        omitted.content_size = 10;
        let mut omitted_old = with_body(create_file_node("a.txt", 0, 1000), b"");
        omitted_old.content_size = 10;
        assert!(is_file_modified(&omitted_old, &omitted));
    }

    #[test]
    fn equal_timestamp_and_size_stay_unmodified_whatever_the_bodies() {
        let old = with_body(create_file_node("a.txt", 0, 1000), b"same bytes");
        let new = with_body(create_file_node("a.txt", 0, 1000), b"samx bytes");
        assert!(!is_file_modified(&old, &new));
    }

    // --- COUNT FILES RECURSIVE TESTS ---

    #[test]
    fn test_count_files_recursive_empty() {
        let root = create_dir_node("");
        assert_eq!(count_files_recursive(&root), 0);
    }

    #[test]
    fn test_count_files_recursive_single_file() {
        let file = create_file_node("file.txt", 100, 1000);
        assert_eq!(count_files_recursive(&file), 1);
    }

    #[test]
    fn test_count_files_recursive_nested() {
        let mut root = create_dir_node("");
        root.children.push(create_file_node("a.txt", 10, 100));

        let mut subdir = create_dir_node("sub");
        subdir.children.push(create_file_node("sub/b.txt", 20, 200));
        subdir.children.push(create_file_node("sub/c.txt", 30, 300));
        root.children.push(subdir);

        assert_eq!(count_files_recursive(&root), 3);
    }
}
