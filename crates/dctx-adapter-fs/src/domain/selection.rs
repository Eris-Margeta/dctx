//! File-handling permission after a selection decision.
//! Implements ADR-014 guarantees and ADR-015b's four decisions.
//! Depends on no IO. The walker and `FsSource::read` both consult it before content.

/// How an adapter may treat one path after rules have been applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileHandlingDecision {
    /// The path may be hashed and returned as content.
    Include,
    /// The path is absent from the inventory. No metadata and no content.
    Exclude,
    /// The path may appear as metadata for diagnostics. Content stays closed.
    DebugVisible,
    /// The path may appear with metadata. Content stays closed.
    OmitContent,
}

impl FileHandlingDecision {
    /// Whether a directory with this decision may be listed.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn should_recurse(self) -> bool {
        match self {
            Self::Include | Self::OmitContent | Self::DebugVisible => true,
            Self::Exclude => false,
        }
    }

    /// Whether file bytes may be opened. Only [`FileHandlingDecision::Include`] allows it.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn should_read_content(self) -> bool {
        matches!(self, Self::Include)
    }

    /// Whether the path appears in an inventory at all.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn show_in_tree(self) -> bool {
        !matches!(self, Self::Exclude)
    }

    /// Whether size, mtime, and inode may be read. Exclude never asks.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn needs_metadata(self) -> bool {
        !matches!(self, Self::Exclude)
    }
}

/// Facts the pure policy needs. The relative path uses `/` separators.
#[derive(Debug, Clone, Copy)]
pub struct ItemContext<'a> {
    /// Path relative to the source root, without a leading slash.
    pub relative_path: &'a str,
    /// Final path component.
    pub name: &'a str,
    /// Directory entries use directory rules; files do not.
    pub is_directory: bool,
    /// When set, an ignore match is [`FileHandlingDecision::DebugVisible`].
    pub debug_mode: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_include_opens_content() {
        assert!(FileHandlingDecision::Include.should_read_content());
        assert!(!FileHandlingDecision::Exclude.should_read_content());
        assert!(!FileHandlingDecision::DebugVisible.should_read_content());
        assert!(!FileHandlingDecision::OmitContent.should_read_content());
        assert!(!FileHandlingDecision::Exclude.needs_metadata());
        assert!(FileHandlingDecision::OmitContent.needs_metadata());
        assert!(!FileHandlingDecision::Exclude.should_recurse());
        assert!(FileHandlingDecision::OmitContent.should_recurse());
    }
}
