//! Domain model for file handling decisions.
//!
//! This module defines the core state machine for how dctx handles files
//! and directories during traversal. It replaces the previous boolean flag
//! approach with an explicit, type-safe state model.

/// Represents the decision made about how to handle a file or directory.
///
/// This enum replaces the previous approach of using multiple boolean flags
/// (is_ignored, is_omitted, is_binary, should_debug) which created 2^N
/// possible states with unclear semantics.
///
/// With this enum, we have exactly 4 valid states, each with clear meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileHandlingDecision {
    /// Include the item in tree and read its content.
    ///
    /// This is the default state for normal files and directories.
    /// Files will have their content read and displayed in the output.
    /// Directories will be recursed into.
    Include,

    /// Exclude the item from the tree entirely.
    ///
    /// Items matching ignore rules (e.g., .git/, node_modules/) are excluded.
    /// They do not appear in the directory tree and their contents are never
    /// read. For directories, we never recurse into them.
    ///
    /// This is the most aggressive form of filtering - as if the file doesn't exist.
    Exclude,

    /// Show the item in tree with [IGNORED] marker, but never read content.
    ///
    /// This state is used in debug mode (--debug-ignored flag). It allows
    /// users to see that files were ignored and verify their ignore rules
    /// are working correctly, while still maintaining the privacy guarantee
    /// that ignored files are never read into memory.
    ///
    /// For directories, we recurse to show the structure, but mark all
    /// descendants as ignored.
    DebugVisible,

    /// Include the item in tree, but omit its content.
    ///
    /// This state is for files that should appear in the directory tree
    /// (so you know they exist) but whose content is not useful for LLM
    /// context. Examples: package-lock.json, Cargo.lock, binary files.
    ///
    /// The file appears in the tree with metadata (size, timestamp), and
    /// in the content section shows a placeholder like:
    /// "[Content omitted by pattern rules]"
    OmitContent,
}

impl FileHandlingDecision {
    /// Returns true if the walker should recurse into this directory.
    ///
    /// # Examples
    ///
    /// ```
    /// use dctx_core::domain::FileHandlingDecision;
    ///
    /// assert!(FileHandlingDecision::Include.should_recurse());
    /// assert!(!FileHandlingDecision::Exclude.should_recurse());
    /// assert!(FileHandlingDecision::DebugVisible.should_recurse());
    /// assert!(FileHandlingDecision::OmitContent.should_recurse());
    /// ```
    pub fn should_recurse(self) -> bool {
        match self {
            Self::Include | Self::OmitContent | Self::DebugVisible => true,
            Self::Exclude => false,
        }
    }

    /// Returns true if the content reader should read this file's content.
    ///
    /// # Privacy Guarantee
    ///
    /// This method enforces the privacy guarantee: files that are Excluded
    /// or DebugVisible are NEVER read into memory, even if they appear in
    /// the tree (DebugVisible case).
    ///
    /// # Examples
    ///
    /// ```
    /// use dctx_core::domain::FileHandlingDecision;
    ///
    /// assert!(FileHandlingDecision::Include.should_read_content());
    /// assert!(!FileHandlingDecision::Exclude.should_read_content());
    /// assert!(!FileHandlingDecision::DebugVisible.should_read_content());
    /// assert!(!FileHandlingDecision::OmitContent.should_read_content());
    /// ```
    pub fn should_read_content(self) -> bool {
        match self {
            Self::Include => true,
            Self::Exclude | Self::DebugVisible | Self::OmitContent => false,
        }
    }

    /// Returns true if this item should appear in the directory tree.
    ///
    /// # Examples
    ///
    /// ```
    /// use dctx_core::domain::FileHandlingDecision;
    ///
    /// assert!(FileHandlingDecision::Include.show_in_tree());
    /// assert!(!FileHandlingDecision::Exclude.show_in_tree());
    /// assert!(FileHandlingDecision::DebugVisible.show_in_tree());
    /// assert!(FileHandlingDecision::OmitContent.show_in_tree());
    /// ```
    pub fn show_in_tree(self) -> bool {
        match self {
            Self::Exclude => false,
            Self::Include | Self::DebugVisible | Self::OmitContent => true,
        }
    }
}

/// Context information for making file handling decisions.
///
/// This struct bundles all the information needed by the IgnoreEngine
/// to make a decision about how to handle a file or directory.
#[derive(Debug, Clone)]
pub struct ItemContext<'a> {
    /// Relative path from project root (e.g., "src/main.rs")
    pub relative_path: &'a str,

    /// Basename of the item (e.g., "main.rs")
    pub name: &'a str,

    /// True if the item is a directory
    pub is_directory: bool,

    /// True if --debug-ignored mode is active
    pub debug_mode: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- FileHandlingDecision Tests ---

    #[test]
    fn test_include_behavior() {
        let decision = FileHandlingDecision::Include;
        assert!(decision.should_recurse());
        assert!(decision.should_read_content());
        assert!(decision.show_in_tree());
    }

    #[test]
    fn test_exclude_behavior() {
        let decision = FileHandlingDecision::Exclude;
        assert!(!decision.should_recurse());
        assert!(!decision.should_read_content());
        assert!(!decision.show_in_tree());
    }

    #[test]
    fn test_debug_visible_behavior() {
        let decision = FileHandlingDecision::DebugVisible;
        assert!(decision.should_recurse());
        assert!(!decision.should_read_content()); // Privacy guarantee!
        assert!(decision.show_in_tree());
    }

    #[test]
    fn test_omit_content_behavior() {
        let decision = FileHandlingDecision::OmitContent;
        assert!(decision.should_recurse());
        assert!(!decision.should_read_content());
        assert!(decision.show_in_tree());
    }

    #[test]
    fn test_decision_equality() {
        assert_eq!(FileHandlingDecision::Include, FileHandlingDecision::Include);
        assert_ne!(FileHandlingDecision::Include, FileHandlingDecision::Exclude);
    }

    #[test]
    fn test_decision_debug_format() {
        let decision = FileHandlingDecision::Include;
        let debug_str = format!("{:?}", decision);
        assert!(debug_str.contains("Include"));
    }

    #[test]
    fn test_decision_clone() {
        let decision = FileHandlingDecision::Include;
        let cloned = decision.clone();
        assert_eq!(decision, cloned);
    }

    #[test]
    fn test_decision_copy() {
        let decision = FileHandlingDecision::Include;
        let copied = decision; // Copy semantics
        assert_eq!(decision, copied);
    }

    // --- ItemContext Tests ---

    #[test]
    fn test_item_context_creation() {
        let context = ItemContext {
            relative_path: "src/main.rs",
            name: "main.rs",
            is_directory: false,
            debug_mode: false,
        };

        assert_eq!(context.relative_path, "src/main.rs");
        assert_eq!(context.name, "main.rs");
        assert!(!context.is_directory);
        assert!(!context.debug_mode);
    }

    #[test]
    fn test_item_context_clone() {
        let context = ItemContext {
            relative_path: "src/",
            name: "src",
            is_directory: true,
            debug_mode: true,
        };

        let cloned = context.clone();
        assert_eq!(context.relative_path, cloned.relative_path);
        assert_eq!(context.name, cloned.name);
        assert_eq!(context.is_directory, cloned.is_directory);
        assert_eq!(context.debug_mode, cloned.debug_mode);
    }
}
