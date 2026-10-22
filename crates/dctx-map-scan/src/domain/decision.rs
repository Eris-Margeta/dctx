//! Privacy permissions for each file handling decision.
//! Implements ADR-014 and SPEC-MAP-MODE section 6.1.
//! Pure values used by ignore engines and future walkers.

/// Represents the decision made about how to handle a file or directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileHandlingDecision {
    /// Include the item in tree and read its content.
    Include,

    /// Exclude the item from the tree entirely.
    Exclude,

    /// Show the item in tree with `[IGNORED]` marker, but never read content.
    DebugVisible,

    /// Include the item in tree, but omit its content.
    OmitContent,
}

impl FileHandlingDecision {
    /// Returns true if the walker should recurse into this directory.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn should_recurse(self) -> bool {
        match self {
            Self::Include | Self::OmitContent | Self::DebugVisible => true,
            Self::Exclude => false,
        }
    }

    /// Returns true if the content reader should read this file's content.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn should_read_content(self) -> bool {
        match self {
            Self::Include => true,
            Self::Exclude | Self::DebugVisible | Self::OmitContent => false,
        }
    }

    /// Returns true if this item should appear in the directory tree.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn show_in_tree(self) -> bool {
        match self {
            Self::Exclude => false,
            Self::Include | Self::DebugVisible | Self::OmitContent => true,
        }
    }

    /// Returns true if metadata (size, mtime) should be read from filesystem.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn needs_metadata(self) -> bool {
        match self {
            // Full records, omitted-content tree entries and debug entries all need metadata.
            Self::Include | Self::OmitContent | Self::DebugVisible => true,
            Self::Exclude => false,     // Never shown, skip metadata
        }
    }
}

/// Context information for making file handling decisions.
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
        let debug_str = format!("{decision:?}");
        assert!(debug_str.contains("Include"));
    }

    #[test]
    fn test_decision_clone() {
        let decision = FileHandlingDecision::Include;
        let cloned = decision;
        assert_eq!(decision, cloned);
    }

    #[test]
    fn test_decision_copy() {
        let decision = FileHandlingDecision::Include;
        let copied = decision; // Copy semantics
        assert_eq!(decision, copied);
    }

    #[test]
    fn test_needs_metadata_helper() {
        assert!(FileHandlingDecision::Include.needs_metadata());
        assert!(FileHandlingDecision::OmitContent.needs_metadata());
        assert!(FileHandlingDecision::DebugVisible.needs_metadata());
        assert!(!FileHandlingDecision::Exclude.needs_metadata());
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
