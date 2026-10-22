//! Recursive snapshot walk: file counts, added and removed subtrees, and node comparison.
//! Implements the child-matching half of the pack diff (relative path identity, decision 078).
//! Depends on [`super`] and [`crate::datatypes`]; pure, no IO.
//! Used by [`super::compare::compare_trees`].
//! A file changes when its size or modification time differs; directories change through children.

use super::{ChangeType, DiffEntry, DiffReport};
use crate::datatypes::{DirContextTreeNode, NodeType};
use std::collections::HashMap;

pub(super) fn count_files_recursive(node: &DirContextTreeNode) -> usize {
    let mut count = 0;
    if node.node_type == NodeType::File {
        count = 1;
    }
    if node.node_type == NodeType::Directory {
        for child in &node.children {
            count += count_files_recursive(child);
        }
    }
    count
}

pub(super) fn mark_all_as_added_recursive(node: &DirContextTreeNode, report: &mut DiffReport) {
    // Dodajemo entry u diff listu
    report.entries.push(DiffEntry {
        change_type: ChangeType::Added,
        node_type: node.node_type,
        relative_path: node.relative_path.clone(),
    });

    // Ažuriramo statistiku
    if node.node_type == NodeType::File {
        report.stats.files_added += 1;
    }

    // Rekurzija za djecu
    if node.node_type == NodeType::Directory {
        for child in &node.children {
            mark_all_as_added_recursive(child, report);
        }
    }
}

pub(super) fn mark_all_as_removed_recursive(node: &DirContextTreeNode, report: &mut DiffReport) {
    report.entries.push(DiffEntry {
        change_type: ChangeType::Removed,
        node_type: node.node_type,
        relative_path: node.relative_path.clone(),
    });

    if node.node_type == NodeType::File {
        report.stats.files_removed += 1;
    }

    if node.node_type == NodeType::Directory {
        for child in &node.children {
            mark_all_as_removed_recursive(child, report);
        }
    }
}

pub(super) fn compare_nodes_recursive(
    old_node: &DirContextTreeNode,
    new_node: &DirContextTreeNode,
    report: &mut DiffReport,
) {
    // 1. Usporedba samog čvora (Modified check)
    // Ovdje provjeravamo samo ako su oba fajlovi. Direktoriji se ne "mijenjaju" na isti način,
    // promjene direktorija se reflektiraju kroz djecu.
    if old_node.node_type == NodeType::File && new_node.node_type == NodeType::File {
        let is_modified = is_file_modified(old_node, new_node);

        if is_modified {
            report.entries.push(DiffEntry {
                change_type: ChangeType::Modified,
                node_type: NodeType::File,
                relative_path: new_node.relative_path.clone(),
            });
            report.stats.files_modified += 1;
        } else {
            // Nije modificiran -> Unchanged
            report.stats.files_unchanged += 1;
        }
    } else if old_node.node_type != new_node.node_type {
        // Tip se promijenio (npr. bio file, sad je dir).
        // Tretiramo ovo kao Remove starog i Add novog.
        // Ali u praksi, pošto recursion ide po imenima, ovo će se vjerojatno riješiti kroz children logiku.
        // Ako smo ovdje, znači da smo matchali po imenu/pathu.

        // Logika: Stari file je maknut, novi dir je dodan.
        // Ali čekaj, ova funkcija prima dva čvora za koja ZNAMO da su "isti" path.
        // Za jednostavnost, ako se tip promijeni, sve ispod starog je removed, sve ispod novog je added.
        report.entries.push(DiffEntry {
            change_type: ChangeType::Removed,
            node_type: old_node.node_type,
            relative_path: old_node.relative_path.clone(),
        });
        report.entries.push(DiffEntry {
            change_type: ChangeType::Added,
            node_type: new_node.node_type,
            relative_path: new_node.relative_path.clone(),
        });

        // Update stats: Type changed (file->dir or dir->file)
        // Mark old as removed and recursively count its files
        if old_node.node_type == NodeType::File {
            report.stats.files_removed += 1;
        } else {
            // Old was directory - recursively count removed files
            report.stats.files_removed += count_files_recursive(old_node);
        }

        // Mark new as added and recursively count its files
        if new_node.node_type == NodeType::File {
            report.stats.files_added += 1;
        } else {
            // New is directory - recursively count added files
            report.stats.files_added += count_files_recursive(new_node);
        }

        return;
    }

    // 2. Usporedba djece (samo za direktorije)
    if old_node.node_type == NodeType::Directory && new_node.node_type == NodeType::Directory {
        // Kreiramo mapu stare djece za brzi lookup po imenu (basename ili relative path)
        // Relative path je jedinstven unutar stabla, ali unutar node.children je dovoljno gledati file_name
        // Međutim, u DirContextTreeNode imamo samo relative_path.
        // Koristit ćemo relative_path kao ključ.

        let mut old_children_map: HashMap<&String, &DirContextTreeNode> = HashMap::new();
        for child in &old_node.children {
            old_children_map.insert(&child.relative_path, child);
        }

        // Prolazimo kroz novu djecu
        for new_child in &new_node.children {
            if let Some(old_child) = old_children_map.remove(&new_child.relative_path) {
                // Postoji i u starom -> Rekurzivna usporedba
                compare_nodes_recursive(old_child, new_child, report);
            } else {
                // Ne postoji u starom -> ADDED
                mark_all_as_added_recursive(new_child, report);
            }
        }

        // Što je ostalo u mapi, to je REMOVED
        // Sortiranje nije nužno za logiku, ali je lijepo za determinizam (hashmap je random iter)
        // Ali DiffEntry vector će ionako biti miksan.
        for (_, old_child) in old_children_map {
            mark_all_as_removed_recursive(old_child, report);
        }
    }
}

/// Logika za detekciju promjene sadržaja.
/// Trenutno koristi Size + Timestamp.
/// U budućnosti ovdje možemo dodati Hash check.
fn is_file_modified(old: &DirContextTreeNode, new: &DirContextTreeNode) -> bool {
    // 1. Provjera veličine
    if old.content_size != new.content_size {
        return true;
    }

    // 2. Provjera vremena modifikacije
    // Napomena: Ovo može biti nepouzdano ako se kopira na drugi FS.
    // Ali za lokalni diff je standard.
    if old.last_modified_timestamp != new.last_modified_timestamp {
        return true;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn create_file_node(path: &str, size: u64, timestamp: u64) -> DirContextTreeNode {
        let mut node = DirContextTreeNode::new(
            NodeType::File,
            path,
            PathBuf::from(format!("/test/{path}")),
        );
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
