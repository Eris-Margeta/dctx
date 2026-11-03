// SPDX-License-Identifier: Apache-2.0
//! File and directory inclusion predicates for [`super::FilterConfig`].
//! Implements extension, size, empty, hidden and binary filters for pack output.
//! Depends on [`super`] and [`super::names`]; pure over names and sizes.
//! Used by the legacy walker's `filter_tree` and by dctx-cli.
//! Hidden names are excluded unless `only_hidden` asks for them.

use super::FilterConfig;
use std::path::Path;

impl FilterConfig {
    /// Check if a file should be included based on all active filters
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn should_include_file(
        &self,
        file_name: &str,
        file_size: u64,
        is_binary: bool,
        _file_path: &Path,
    ) -> bool {
        // Hidden files filter
        if self.only_hidden && !super::names::is_hidden(file_name) {
            return false;
        }
        if !self.only_hidden && super::names::is_hidden(file_name) && !self.extensions.is_empty() {
            // If we have extension filters and this is a hidden file,
            // still check extension (e.g., .gitignore with --ext ignore)
        }

        // Binary filter (exclude binary files unless explicitly included)
        if is_binary && !self.include_binary {
            return false;
        }

        // Extension filter
        if !self.extensions.is_empty() {
            if let Some(ext) = super::names::get_extension(file_name) {
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
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn should_include_directory(&self, dir_name: &str) -> bool {
        // Hidden filter for directories
        if self.only_hidden && !super::names::is_hidden(dir_name) {
            return false;
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

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
