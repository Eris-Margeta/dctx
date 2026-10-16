//! File filtering for pack output (`--ext`, size, empty, hidden, binary filters).
//! Provides fine-grained control over what files appear in context output;
//! all filters apply to both the directory tree and the file content sections.
//! Depends on nothing; pure predicates over names and sizes.
//! Used by the legacy walker's `filter_tree` and by dctx-cli.

use std::path::Path;

/// Configuration for file filtering
#[derive(Debug, Clone, Default)]
pub struct FilterConfig {
    /// Only include files with these extensions (e.g., ["rs", "toml"])
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
    /// Create a new FilterConfig with no filters active
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if any filters are active
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
    pub fn with_extensions(mut self, extensions: Vec<String>) -> Self {
        self.extensions = extensions;
        self
    }

    /// Set whether to include binary files
    pub fn with_include_binary(mut self, include: bool) -> Self {
        self.include_binary = include;
        self
    }

    /// Set to show only empty files
    pub fn with_only_empty(mut self, only_empty: bool) -> Self {
        self.only_empty = only_empty;
        self
    }

    /// Set to show only non-empty files
    pub fn with_only_non_empty(mut self, only_non_empty: bool) -> Self {
        self.only_non_empty = only_non_empty;
        self
    }

    /// Set minimum file size in megabytes
    pub fn with_min_size_mb(mut self, mb: f64) -> Self {
        self.min_size_bytes = Some((mb * 1_048_576.0) as u64);
        self
    }

    /// Set maximum file size in megabytes
    pub fn with_max_size_mb(mut self, mb: f64) -> Self {
        self.max_size_bytes = Some((mb * 1_048_576.0) as u64);
        self
    }

    /// Set to show only hidden files
    pub fn with_only_hidden(mut self, only_hidden: bool) -> Self {
        self.only_hidden = only_hidden;
        self
    }

    /// Set whether to collect ignored folders for debugging
    pub fn with_collect_ignored(mut self, collect: bool) -> Self {
        self.collect_ignored_folders = collect;
        self
    }

    /// Check if a file should be included based on all active filters
    pub fn should_include_file(
        &self,
        file_name: &str,
        file_size: u64,
        is_binary: bool,
        _file_path: &Path,
    ) -> bool {
        // Hidden files filter
        if self.only_hidden && !is_hidden(file_name) {
            return false;
        }
        if !self.only_hidden && is_hidden(file_name) && !self.extensions.is_empty() {
            // If we have extension filters and this is a hidden file,
            // still check extension (e.g., .gitignore with --ext ignore)
        }

        // Binary filter (exclude binary files unless explicitly included)
        if is_binary && !self.include_binary {
            return false;
        }

        // Extension filter
        if !self.extensions.is_empty() {
            if let Some(ext) = get_extension(file_name) {
                if !self.extensions.iter().any(|e| e.eq_ignore_ascii_case(ext)) {
                    return false;
                }
            } else {
                // No extension - exclude if extension filter is active
                return false;
            }
        }

        // Empty/non-empty filter
        if self.only_empty && file_size > 0 {
            return false;
        }
        if self.only_non_empty && file_size == 0 {
            return false;
        }

        // Size filters
        if let Some(min_size) = self.min_size_bytes {
            if file_size < min_size {
                return false;
            }
        }
        if let Some(max_size) = self.max_size_bytes {
            if file_size > max_size {
                return false;
            }
        }

        true
    }

    /// Check if a directory should be included based on hidden filter
    pub fn should_include_directory(&self, dir_name: &str) -> bool {
        // Hidden filter for directories
        if self.only_hidden && !is_hidden(dir_name) {
            return false;
        }

        true
    }
}

/// Check if a file/folder name is hidden (starts with '.')
fn is_hidden(name: &str) -> bool {
    name.starts_with('.') && name != "." && name != ".."
}

/// Extract file extension from filename
/// For files like ".gitignore", returns "gitignore"
/// For files like "file.txt", returns "txt"
/// For files like "archive.tar.gz", returns "gz"
fn get_extension(file_name: &str) -> Option<&str> {
    // Find the last dot
    if let Some(pos) = file_name.rfind('.') {
        // If dot is at position 0 (e.g., ".gitignore"), treat everything after as extension
        if pos == 0 && file_name.len() > 1 {
            return Some(&file_name[1..]);
        }
        // Normal case: dot is not at the start or end
        if pos > 0 && pos < file_name.len() - 1 {
            return Some(&file_name[pos + 1..]);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_default_filter_no_active_filters() {
        let filter = FilterConfig::default();
        assert!(!filter.has_active_filters());
    }

    #[test]
    fn test_extension_filter_single() {
        let filter = FilterConfig::new().with_extensions(vec!["rs".to_string()]);
        assert!(filter.has_active_filters());

        assert!(filter.should_include_file("main.rs", 100, false, &PathBuf::from("main.rs")));
        assert!(!filter.should_include_file(
            "config.toml",
            100,
            false,
            &PathBuf::from("config.toml")
        ));
    }

    #[test]
    fn test_extension_filter_multiple() {
        let filter =
            FilterConfig::new().with_extensions(vec!["rs".to_string(), "toml".to_string()]);

        assert!(filter.should_include_file("main.rs", 100, false, &PathBuf::from("main.rs")));
        assert!(filter.should_include_file(
            "config.toml",
            100,
            false,
            &PathBuf::from("config.toml")
        ));
        assert!(!filter.should_include_file("readme.md", 100, false, &PathBuf::from("readme.md")));
    }

    #[test]
    fn test_extension_filter_case_insensitive() {
        let filter = FilterConfig::new().with_extensions(vec!["RS".to_string()]);

        assert!(filter.should_include_file("main.rs", 100, false, &PathBuf::from("main.rs")));
        assert!(filter.should_include_file("main.RS", 100, false, &PathBuf::from("main.RS")));
        assert!(filter.should_include_file("main.Rs", 100, false, &PathBuf::from("main.Rs")));
    }

    #[test]
    fn test_extension_filter_no_extension() {
        let filter = FilterConfig::new().with_extensions(vec!["rs".to_string()]);

        assert!(!filter.should_include_file("Makefile", 100, false, &PathBuf::from("Makefile")));
        assert!(!filter.should_include_file("LICENSE", 100, false, &PathBuf::from("LICENSE")));
    }

    #[test]
    fn test_binary_filter_default_excludes() {
        let filter = FilterConfig::new();

        // By default, binary files are excluded
        assert!(!filter.should_include_file("image.png", 1000, true, &PathBuf::from("image.png")));
        assert!(filter.should_include_file("main.rs", 1000, false, &PathBuf::from("main.rs")));
    }

    #[test]
    fn test_binary_filter_explicit_include() {
        let filter = FilterConfig::new().with_include_binary(true);

        assert!(filter.should_include_file("image.png", 1000, true, &PathBuf::from("image.png")));
        assert!(filter.should_include_file("main.rs", 1000, false, &PathBuf::from("main.rs")));
    }

    #[test]
    fn test_empty_filter() {
        let filter = FilterConfig::new().with_only_empty(true);

        assert!(filter.should_include_file("empty.txt", 0, false, &PathBuf::from("empty.txt")));
        assert!(!filter.should_include_file("data.txt", 100, false, &PathBuf::from("data.txt")));
    }

    #[test]
    fn test_non_empty_filter() {
        let filter = FilterConfig::new().with_only_non_empty(true);

        assert!(!filter.should_include_file("empty.txt", 0, false, &PathBuf::from("empty.txt")));
        assert!(filter.should_include_file("data.txt", 100, false, &PathBuf::from("data.txt")));
    }

    #[test]
    fn test_size_filter_large() {
        let filter = FilterConfig::new().with_min_size_mb(1.0); // 1MB

        assert!(!filter.should_include_file("small.txt", 1000, false, &PathBuf::from("small.txt")));
        assert!(filter.should_include_file(
            "large.bin",
            2_000_000,
            false,
            &PathBuf::from("large.bin")
        ));
    }

    #[test]
    fn test_size_filter_small() {
        let filter = FilterConfig::new().with_max_size_mb(0.1); // 100KB

        assert!(filter.should_include_file("tiny.txt", 1000, false, &PathBuf::from("tiny.txt")));
        assert!(!filter.should_include_file("big.txt", 200_000, false, &PathBuf::from("big.txt")));
    }

    #[test]
    fn test_size_filter_range() {
        let filter = FilterConfig::new()
            .with_min_size_mb(0.01) // 10KB
            .with_max_size_mb(0.1); // 100KB

        assert!(!filter.should_include_file("tiny.txt", 1000, false, &PathBuf::from("tiny.txt"))); // 1KB
        assert!(filter.should_include_file(
            "medium.txt",
            50_000,
            false,
            &PathBuf::from("medium.txt")
        )); // 50KB
        assert!(!filter.should_include_file(
            "large.txt",
            200_000,
            false,
            &PathBuf::from("large.txt")
        )); // 200KB
    }

    #[test]
    fn test_hidden_filter() {
        let filter = FilterConfig::new().with_only_hidden(true);

        assert!(filter.should_include_file(".gitignore", 100, false, &PathBuf::from(".gitignore")));
        assert!(filter.should_include_file(".env", 100, false, &PathBuf::from(".env")));
        assert!(!filter.should_include_file("main.rs", 100, false, &PathBuf::from("main.rs")));
    }

    #[test]
    fn test_hidden_directory_filter() {
        let filter = FilterConfig::new().with_only_hidden(true);

        assert!(filter.should_include_directory(".git"));
        assert!(filter.should_include_directory(".config"));
        assert!(!filter.should_include_directory("src"));
        assert!(!filter.should_include_directory("target"));
    }

    #[test]
    fn test_is_hidden_function() {
        assert!(is_hidden(".gitignore"));
        assert!(is_hidden(".env"));
        assert!(is_hidden(".config"));
        assert!(!is_hidden("file.txt"));
        assert!(!is_hidden(".")); // Current dir marker
        assert!(!is_hidden("..")); // Parent dir marker
    }

    #[test]
    fn test_get_extension_function() {
        assert_eq!(get_extension("file.txt"), Some("txt"));
        assert_eq!(get_extension("archive.tar.gz"), Some("gz"));
        assert_eq!(get_extension("Makefile"), None);
        assert_eq!(get_extension(".gitignore"), Some("gitignore"));
        assert_eq!(get_extension("file."), None);
        assert_eq!(get_extension(".hidden"), Some("hidden"));
    }

    #[test]
    fn test_combined_filters_extension_and_size() {
        let filter = FilterConfig::new()
            .with_extensions(vec!["rs".to_string()])
            .with_max_size_mb(0.5);

        assert!(filter.should_include_file("main.rs", 100_000, false, &PathBuf::from("main.rs"))); // 100KB Rust file
        assert!(!filter.should_include_file(
            "main.rs",
            1_000_000,
            false,
            &PathBuf::from("main.rs")
        )); // 1MB Rust file (too large)
        assert!(!filter.should_include_file(
            "config.toml",
            1000,
            false,
            &PathBuf::from("config.toml")
        )); // Wrong extension
    }

    #[test]
    fn test_combined_filters_extension_and_binary() {
        let filter = FilterConfig::new()
            .with_extensions(vec!["png".to_string()])
            .with_include_binary(true);

        // Binary .png file (e.g., actual PNG image)
        assert!(filter.should_include_file("image.png", 10000, true, &PathBuf::from("image.png")));
        // Binary .jpg file (wrong extension)
        assert!(!filter.should_include_file("photo.jpg", 10000, true, &PathBuf::from("photo.jpg")));
        // Non-binary .png file (e.g., text file named data.png) - should be included
        assert!(filter.should_include_file("data.png", 10000, false, &PathBuf::from("data.png")));
    }

    #[test]
    fn test_combined_filters_hidden_and_extension() {
        let filter = FilterConfig::new()
            .with_only_hidden(true)
            .with_extensions(vec!["env".to_string()]);

        // .env file: hidden AND matches extension
        assert!(filter.should_include_file(".env", 100, false, &PathBuf::from(".env")));
        // .gitignore file: hidden but wrong extension (gitignore != env)
        assert!(!filter.should_include_file(
            ".gitignore",
            100,
            false,
            &PathBuf::from(".gitignore")
        ));
        // config.env file: correct extension but NOT hidden
        assert!(!filter.should_include_file(
            "config.env",
            100,
            false,
            &PathBuf::from("config.env")
        ));
    }
}
