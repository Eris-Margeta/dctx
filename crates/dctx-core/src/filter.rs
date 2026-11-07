// SPDX-License-Identifier: Apache-2.0
//! File filtering for pack output (`--ext`, size, empty, hidden, binary filters).
//! Provides fine-grained control over what files appear in context output;
//! all filters apply to both the directory tree and the file content sections.
//! Depends on nothing; pure predicates over names and sizes.
//! Used by the legacy walker's `filter_tree` and by dctx-cli.

mod include;
mod names;

pub use names::{has_git_segment, is_secret_shaped_name};

/// Configuration for file filtering
#[derive(Debug, Clone, Default)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "These independent public filters retain their existing combinations, including empty selections."
)]
pub struct FilterConfig {
    /// Only include files with these extensions (e.g., `["rs", "toml"]`)
    /// Empty vec = no extension filter
    pub extensions: Vec<String>,

    /// Include binary files in output (default: false)
    pub include_binary: bool,

    /// Show only empty files (0 bytes)
    pub only_empty: bool,

    /// Show only non-empty files (>0 bytes)
    pub only_non_empty: bool,

    /// Show only files larger than this size in bytes (None = no filter)
    pub min_size_bytes: Option<u64>,

    /// Show only files smaller than this size in bytes (None = no filter)
    pub max_size_bytes: Option<u64>,

    /// Show ONLY hidden files/folders (starting with '.')
    pub only_hidden: bool,

    /// Collect ignored folders for debugging
    pub collect_ignored_folders: bool,
}

impl FilterConfig {
    /// Create a new `FilterConfig` with no filters active
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if any filters are active
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn has_active_filters(&self) -> bool {
        !self.extensions.is_empty()
            || self.include_binary
            || self.only_empty
            || self.only_non_empty
            || self.min_size_bytes.is_some()
            || self.max_size_bytes.is_some()
            || self.only_hidden
    }

    /// Add file extensions to filter by (e.g., "rs", "toml")
    #[allow(
        clippy::must_use_candidate,
        clippy::return_self_not_must_use,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn with_extensions(mut self, extensions: Vec<String>) -> Self {
        self.extensions = extensions;
        self
    }

    /// Set whether to include binary files
    #[allow(
        clippy::must_use_candidate,
        clippy::return_self_not_must_use,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn with_include_binary(mut self, include: bool) -> Self {
        self.include_binary = include;
        self
    }

    /// Set to show only empty files
    #[allow(
        clippy::must_use_candidate,
        clippy::return_self_not_must_use,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn with_only_empty(mut self, only_empty: bool) -> Self {
        self.only_empty = only_empty;
        self
    }

    /// Set to show only non-empty files
    #[allow(
        clippy::must_use_candidate,
        clippy::return_self_not_must_use,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn with_only_non_empty(mut self, only_non_empty: bool) -> Self {
        self.only_non_empty = only_non_empty;
        self
    }

    /// Set minimum file size in megabytes
    #[allow(
        clippy::must_use_candidate,
        clippy::return_self_not_must_use,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "Preserve the public size conversion: fractional bytes truncate, negatives/NaN become zero, and overflow saturates to u64::MAX."
    )]
    pub fn with_min_size_mb(mut self, mb: f64) -> Self {
        self.min_size_bytes = Some((mb * 1_048_576.0) as u64);
        self
    }

    /// Set maximum file size in megabytes
    #[allow(
        clippy::must_use_candidate,
        clippy::return_self_not_must_use,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "Preserve the public size conversion: fractional bytes truncate, negatives/NaN become zero, and overflow saturates to u64::MAX."
    )]
    pub fn with_max_size_mb(mut self, mb: f64) -> Self {
        self.max_size_bytes = Some((mb * 1_048_576.0) as u64);
        self
    }

    /// Set to show only hidden files
    #[allow(
        clippy::must_use_candidate,
        clippy::return_self_not_must_use,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn with_only_hidden(mut self, only_hidden: bool) -> Self {
        self.only_hidden = only_hidden;
        self
    }

    /// Set whether to collect ignored folders for debugging
    #[allow(
        clippy::must_use_candidate,
        clippy::return_self_not_must_use,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn with_collect_ignored(mut self, collect: bool) -> Self {
        self.collect_ignored_folders = collect;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_filter_no_active_filters() {
        let filter = FilterConfig::default();
        assert!(!filter.has_active_filters());
    }
}
