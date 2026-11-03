// SPDX-License-Identifier: Apache-2.0
//! Pack tree data: node kinds, ignore-rule records and the snapshot tree node.
//! Ports the C `dircontxt` data structures; the binary format and formatter read these.
//! Depends on [`crate::domain`]; pure data (`disk_path` is a path value, never opened here).
//! Used by every pack module, legacy and new, and by dctx-cli and the desktop.

use std::path::PathBuf;
use crate::domain::FileHandlingDecision;

/// Ekvivalent za `NodeType` iz C-a
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeType {
    /// A file whose content may be included in a snapshot.
    File,
    /// A directory containing child nodes.
    Directory,
}

/// Ekvivalent za `PatternType` iz C-a
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatternType {
    /// No valid pattern has been assigned.
    Invalid,
    /// Matches only the file or directory basename.
    Basename, // Matches only the file/dir name (e.g., "node_modules")
    /// Matches the full path relative to the root.
    Path,     // Matches the full relative path (e.g., "src/main.c")
    /// Matches a suffix wildcard such as `*.log`.
    Suffix,   // Matches a suffix wildcard (e.g., "*.log")
    /// Matches a prefix wildcard such as `build/*`.
    Prefix,   // Matches a prefix wildcard (e.g., "build/*")

    // NOVO: Za složene uzorke
    /// Matches a compound glob such as `foo/**/bar`.
    Glob, // Matches complex patterns ("foo/**/bar", "config-*-prod.json")
}

/// Ekvivalent za `IgnoreRule` iz C-a
#[derive(Debug, Clone)]
pub struct IgnoreRule {
    /// Pattern text after ignore-rule prefixes have been removed.
    pub pattern: String,
    /// Matching strategy selected for this pattern.
    pub pattern_type: PatternType,
    /// Restricts the rule to directories.
    pub is_dir_only: bool,
    /// Re-includes matching items when the rule starts with `!`.
    pub is_negation: bool, // Set to true if the pattern starts with '!'
}

impl Default for IgnoreRule {
    fn default() -> Self {
        Self {
            pattern: String::new(),
            pattern_type: PatternType::Invalid,
            is_dir_only: false,
            is_negation: false,
        }
    }
}

impl IgnoreRule {
    /// Konstruktor za stvaranje praznog pravila
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn new() -> Self {
        Self::default()
    }
}

/// Ekvivalent za `DirContextTreeNode` iz C-a
#[derive(Debug, Clone)]
pub struct DirContextTreeNode {
    /// Whether this node represents a file or a directory.
    pub node_type: NodeType,

    /// Relativna putanja unutar arhive (npr. "src/main.rs")
    pub relative_path: String,

    /// Unix timestamp zadnje modifikacije
    pub last_modified_timestamp: u64,

    // --- Za datoteke ---
    /// Offset gdje počinje sadržaj u binarnoj datoteci
    pub content_offset_in_data_section: u64,

    /// Veličina sadržaja datoteke
    pub content_size: u64,

    /// Apsolutna putanja na disku
    pub disk_path: PathBuf,

    // --- Za direktorije ---
    /// Djeca ovog čvora.
    pub children: Vec<DirContextTreeNode>,

    // --- Za LLM Formatter ---
    /// Generirani ID (npr. "F001", "D002")
    pub generated_id_for_llm: String,

    // --- Cache za sadržaj ---
    /// Optional file bytes retained for rendering or binary serialization.
    pub file_content: Option<Vec<u8>>,

    // --- Binary file handling ---
    /// Indicates if this is a binary file and content should be omitted
    pub is_binary_file: bool,
    /// Type description for binary files (e.g., "PNG image", "PDF document")
    pub binary_file_type: Option<String>,

    // --- Privacy & Debug Support ---
    /// Marks files/folders that matched ignore rules.
    /// Only populated when --debug-ignored flag is active.
    /// When true: Show in tree but NEVER read content.
    pub is_ignored: bool,

    // --- Phase 10.7: Explicit State Model ---
    /// The decision made about how to handle this node.
    /// Added in Phase 10.7.2 as part of architecture refactoring.
    /// Default is Include for backward compatibility.
    pub handling_decision: FileHandlingDecision,
}

impl DirContextTreeNode {
    /// Creates an empty node with an include decision and no file content.
    /// All sizes, timestamps and offsets start at zero; `disk_path` is stored without IO.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn new(node_type: NodeType, relative_path: &str, disk_path: PathBuf) -> Self {
        Self {
            node_type,
            relative_path: relative_path.to_string(),
            last_modified_timestamp: 0,
            content_offset_in_data_section: 0,
            content_size: 0,
            disk_path,
            children: Vec::new(),
            generated_id_for_llm: String::new(),
            file_content: None,
            is_binary_file: false,
            binary_file_type: None,
            is_ignored: false,
            handling_decision: FileHandlingDecision::Include,
        }
    }

    /// Returns whether this node is a directory.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn is_dir(&self) -> bool {
        self.node_type == NodeType::Directory
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_type_equality() {
        assert_eq!(NodeType::File, NodeType::File);
        assert_eq!(NodeType::Directory, NodeType::Directory);
        assert_ne!(NodeType::File, NodeType::Directory);
    }

    #[test]
    fn test_pattern_type_equality() {
        assert_eq!(PatternType::Basename, PatternType::Basename);
        assert_eq!(PatternType::Path, PatternType::Path);
        assert_eq!(PatternType::Suffix, PatternType::Suffix);
        assert_eq!(PatternType::Prefix, PatternType::Prefix);
        assert_eq!(PatternType::Glob, PatternType::Glob);
        assert_eq!(PatternType::Invalid, PatternType::Invalid);
    }

    #[test]
    fn test_ignore_rule_new() {
        let rule = IgnoreRule::new();
        assert_eq!(rule.pattern, "");
        assert_eq!(rule.pattern_type, PatternType::Invalid);
        assert!(!rule.is_dir_only);
        assert!(!rule.is_negation);
    }

    #[test]
    fn test_dir_context_tree_node_new_file() {
        let node = DirContextTreeNode::new(
            NodeType::File,
            "src/main.rs",
            PathBuf::from("/home/user/project/src/main.rs"),
        );

        assert_eq!(node.node_type, NodeType::File);
        assert_eq!(node.relative_path, "src/main.rs");
        assert_eq!(
            node.disk_path,
            PathBuf::from("/home/user/project/src/main.rs")
        );
        assert_eq!(node.last_modified_timestamp, 0);
        assert_eq!(node.content_offset_in_data_section, 0);
        assert_eq!(node.content_size, 0);
        assert!(node.children.is_empty());
        assert_eq!(node.generated_id_for_llm, "");
        assert!(node.file_content.is_none());
        assert!(!node.is_dir());
    }

    #[test]
    fn test_dir_context_tree_node_new_directory() {
        let node = DirContextTreeNode::new(
            NodeType::Directory,
            "src",
            PathBuf::from("/home/user/project/src"),
        );

        assert_eq!(node.node_type, NodeType::Directory);
        assert_eq!(node.relative_path, "src");
        assert!(node.is_dir());
    }

    #[test]
    fn test_dir_context_tree_node_is_dir() {
        let file_node =
            DirContextTreeNode::new(NodeType::File, "test.txt", PathBuf::from("/test.txt"));
        let dir_node =
            DirContextTreeNode::new(NodeType::Directory, "test_dir", PathBuf::from("/test_dir"));

        assert!(!file_node.is_dir());
        assert!(dir_node.is_dir());
    }

    #[test]
    fn test_dir_context_tree_node_with_children() {
        let mut parent =
            DirContextTreeNode::new(NodeType::Directory, "parent", PathBuf::from("/parent"));

        let child = DirContextTreeNode::new(
            NodeType::File,
            "parent/child.txt",
            PathBuf::from("/parent/child.txt"),
        );

        parent.children.push(child);
        assert_eq!(parent.children.len(), 1);
        assert_eq!(parent.children[0].relative_path, "parent/child.txt");
    }

    #[test]
    fn test_dir_context_tree_node_with_content() {
        let mut node =
            DirContextTreeNode::new(NodeType::File, "test.txt", PathBuf::from("/test.txt"));

        let content = b"Hello, World!".to_vec();
        node.file_content = Some(content.clone());
        node.content_size = content.len() as u64;

        assert!(node.file_content.is_some());
        assert_eq!(node.content_size, 13);
        assert_eq!(node.file_content.unwrap(), b"Hello, World!");
    }

    #[test]
    fn test_dir_context_tree_node_clone() {
        let mut original =
            DirContextTreeNode::new(NodeType::File, "test.txt", PathBuf::from("/test.txt"));
        original.content_size = 100;
        original.last_modified_timestamp = 1_234_567_890;

        let cloned = original.clone();
        assert_eq!(cloned.relative_path, original.relative_path);
        assert_eq!(cloned.content_size, original.content_size);
        assert_eq!(
            cloned.last_modified_timestamp,
            original.last_modified_timestamp
        );
    }
}
