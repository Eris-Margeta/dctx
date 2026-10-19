//! Pack diff rules: compare two snapshot trees into added/removed/modified entries and stats.
//! Modification is size or modification-time change (file metadata, decision 078; no VCS state).
//! Depends on [`crate::datatypes`]; pure function over two trees.
//! Used by dctx-cli directly and by the dctx-pack diff engine.

use crate::datatypes::NodeType;

mod compare;
mod walk;

pub use compare::compare_trees;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeType {
    Added,
    Removed,
    Modified,
}

#[derive(Debug, Clone)]
pub struct DiffEntry {
    pub change_type: ChangeType,
    pub node_type: NodeType,
    pub relative_path: String,
}

#[derive(Debug, Default, Clone)]
pub struct DiffStats {
    pub files_added: usize,
    pub files_removed: usize,
    pub files_modified: usize,
    /// Ključno za verzioniranje: koliko je fajlova ostalo identično.
    pub files_unchanged: usize,
    /// Ukupan broj fajlova u novom stablu (korisno za statistiku).
    pub total_files_in_new_state: usize,
}

#[derive(Debug, Default)]
pub struct DiffReport {
    pub entries: Vec<DiffEntry>,
    pub stats: DiffStats,
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
