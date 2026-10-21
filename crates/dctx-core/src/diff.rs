//! Pack diff rules: compare two snapshot trees into added/removed/modified entries and stats.
//! Modification is size or modification-time change (file metadata, decision 078; no VCS state).
//! Depends on [`crate::datatypes`]; pure function over two trees.
//! Used by dctx-cli directly and by the dctx-pack diff engine.

use crate::datatypes::NodeType;

mod compare;
mod walk;

pub use compare::compare_trees;

/// Kind of change between two snapshot trees.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeType {
    /// The item was added, including a replacement whose node kind changed.
    Added,
    /// The item was removed, including a replacement whose node kind changed.
    Removed,
    /// A file changed content size or modification time.
    Modified,
}

/// One changed file or directory in a snapshot comparison.
#[derive(Debug, Clone)]
pub struct DiffEntry {
    /// How the item differs between snapshots.
    pub change_type: ChangeType,
    /// Node kind, taken from the corresponding added, removed or modified item.
    pub node_type: NodeType,
    /// Path of the item relative to the snapshot root.
    pub relative_path: String,
}

/// File counts collected while comparing snapshot trees.
#[derive(Debug, Default, Clone)]
pub struct DiffStats {
    /// Number of files added to the new tree.
    pub files_added: usize,
    /// Number of files removed from the old tree.
    pub files_removed: usize,
    /// Number of files with changed size or modification time.
    pub files_modified: usize,
    /// Ključno za verzioniranje: koliko je fajlova ostalo identično.
    pub files_unchanged: usize,
    /// Ukupan broj fajlova u novom stablu (korisno za statistiku).
    pub total_files_in_new_state: usize,
}

/// Changes and file statistics produced by comparing two snapshot trees.
#[derive(Debug, Default)]
pub struct DiffReport {
    /// Changed entries collected during the tree walk.
    pub entries: Vec<DiffEntry>,
    /// File counts used for reporting and version advancement.
    pub stats: DiffStats,
    /// Whether the comparison found any changed entries.
    pub has_changes: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- CHANGE TYPE TESTS ---

    #[test]
    fn test_change_type_equality() {
        assert_eq!(ChangeType::Added, ChangeType::Added);
        assert_eq!(ChangeType::Removed, ChangeType::Removed);
        assert_eq!(ChangeType::Modified, ChangeType::Modified);
        assert_ne!(ChangeType::Added, ChangeType::Removed);
    }

    // --- DIFF ENTRY TESTS ---

    #[test]
    fn test_diff_entry_clone() {
        let entry = DiffEntry {
            change_type: ChangeType::Added,
            node_type: NodeType::File,
            relative_path: "test.txt".to_string(),
        };
        let cloned = entry.clone();
        assert_eq!(cloned.change_type, entry.change_type);
        assert_eq!(cloned.node_type, entry.node_type);
        assert_eq!(cloned.relative_path, entry.relative_path);
    }
}
