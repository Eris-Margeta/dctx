// SPDX-License-Identifier: Apache-2.0
//! Shipped rule templates (`templates/global-ignore`, `templates/global-omitcontent`) stay valid.
//! Checks syntax, duplicates, ignore/omit conflicts and the critical patterns installs rely on.
//! Depends only on the template files; moved here from `lib.rs` so the crate root is public API only.
//! Run by `cargo test -p dctx-core`.

#[cfg(test)]
mod template_validation_tests {
    use std::collections::HashSet;

    /// Parse patterns from a template file, ignoring comments and empty lines
    fn parse_patterns(content: &str) -> Vec<String> {
        content
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(std::string::ToString::to_string)
            .collect()
    }

    /// Normalize pattern for comparison (handle trailing slashes, case sensitivity)
    fn normalize_pattern(pattern: &str) -> String {
        pattern.trim_end_matches('/').to_lowercase()
    }

    #[test]
    fn test_no_conflicts_between_ignore_and_omit() {
        // Load the template files at compile time
        let ignore_content = include_str!("../templates/global-ignore");
        let omit_content = include_str!("../templates/global-omitcontent");

        // Parse patterns
        let ignore_patterns = parse_patterns(ignore_content);
        let omit_patterns = parse_patterns(omit_content);

        // Create normalized sets for comparison
        let ignore_set: HashSet<String> = ignore_patterns
            .iter()
            .map(|p| normalize_pattern(p))
            .collect();

        let omit_set: HashSet<String> = omit_patterns
            .iter()
            .map(|p| normalize_pattern(p))
            .collect();

        // Find conflicts
        let conflicts: Vec<String> = ignore_set
            .intersection(&omit_set)
            .cloned()
            .collect();

        // Assert no conflicts
        assert!(
            conflicts.is_empty(),
            "Found {} conflicting patterns between global-ignore and global-omitcontent:\n{}",
            conflicts.len(),
            conflicts.join("\n")
        );
    }

    #[test]
    fn test_ignore_patterns_syntax_validity() {
        let ignore_content = include_str!("../templates/global-ignore");
        let patterns = parse_patterns(ignore_content);

        for pattern in patterns {
            // Check for invalid characters
            assert!(
                !pattern.contains('\0'),
                "Pattern '{pattern}' contains null byte"
            );

            // Check for Windows-style paths (should use Unix-style)
            // Allow escaped special characters like \#, \!, \* for gitignore syntax
            // Only fail if backslash appears to be a path separator (followed by alphanumeric)
            if pattern.contains('\\') {
                let bytes = pattern.as_bytes();
                for i in 0..bytes.len() - 1 {
                    if bytes[i] == b'\\' {
                        let next = bytes[i + 1] as char;
                        // If backslash is followed by alphanumeric or space, it's likely a Windows path
                        assert!(!(next.is_alphanumeric() || next == ' ' || next == '/'),
                            "Pattern '{pattern}' contains Windows-style path separator - use forward slashes"
                        );
                    }
                }
            }

            // Check for double slashes (likely a mistake)
            assert!(
                !pattern.contains("//"),
                "Pattern '{pattern}' contains double slashes"
            );

            // Check for leading slashes (patterns should be relative)
            assert!(!(pattern.starts_with('/') && pattern != "/"),
                "Pattern '{pattern}' starts with '/' - patterns should be relative. \
                 Use 'folder/' instead of '/folder/'"
            );
        }
    }

    #[test]
    fn test_omit_patterns_syntax_validity() {
        let omit_content = include_str!("../templates/global-omitcontent");
        let patterns = parse_patterns(omit_content);

        for pattern in patterns {
            // Check for invalid characters
            assert!(
                !pattern.contains('\0'),
                "Pattern '{pattern}' contains null byte"
            );

            // Check for Windows-style paths (allow escaped special characters)
            if pattern.contains('\\') {
                let bytes = pattern.as_bytes();
                for i in 0..bytes.len() - 1 {
                    if bytes[i] == b'\\' {
                        let next = bytes[i + 1] as char;
                        // If backslash is followed by alphanumeric or space, it's likely a Windows path
                        assert!(!(next.is_alphanumeric() || next == ' ' || next == '/'),
                            "Pattern '{pattern}' contains Windows-style path separator - use forward slashes"
                        );
                    }
                }
            }

            // Check for double slashes
            assert!(
                !pattern.contains("//"),
                "Pattern '{pattern}' contains double slashes"
            );

            // Check for leading slashes
            assert!(!(pattern.starts_with('/') && pattern != "/"),
                "Pattern '{pattern}' starts with '/' - patterns should be relative"
            );
        }
    }

    #[test]
    fn test_no_duplicate_patterns_in_ignore() {
        let ignore_content = include_str!("../templates/global-ignore");
        let patterns = parse_patterns(ignore_content);

        let mut seen = HashSet::new();
        let mut duplicates = Vec::new();

        for pattern in patterns {
            let normalized = normalize_pattern(&pattern);
            if !seen.insert(normalized.clone()) {
                duplicates.push(pattern);
            }
        }

        assert!(
            duplicates.is_empty(),
            "Found {} duplicate patterns in global-ignore:\n{}",
            duplicates.len(),
            duplicates.join("\n")
        );
    }

    #[test]
    fn test_no_duplicate_patterns_in_omit() {
        let omit_content = include_str!("../templates/global-omitcontent");
        let patterns = parse_patterns(omit_content);

        let mut seen = HashSet::new();
        let mut duplicates = Vec::new();

        for pattern in patterns {
            let normalized = normalize_pattern(&pattern);
            if !seen.insert(normalized.clone()) {
                duplicates.push(pattern);
            }
        }

        assert!(
            duplicates.is_empty(),
            "Found {} duplicate patterns in global-omitcontent:\n{}",
            duplicates.len(),
            duplicates.join("\n")
        );
    }

    #[test]
    fn test_template_files_not_empty() {
        let ignore_content = include_str!("../templates/global-ignore");
        let omit_content = include_str!("../templates/global-omitcontent");

        let ignore_patterns = parse_patterns(ignore_content);
        let omit_patterns = parse_patterns(omit_content);

        assert!(
            !ignore_patterns.is_empty(),
            "global-ignore template is empty"
        );
        assert!(
            !omit_patterns.is_empty(),
            "global-omitcontent template is empty"
        );

        // Ensure we have a reasonable number of patterns (sanity check)
        assert!(
            ignore_patterns.len() >= 50,
            "global-ignore has too few patterns: {}",
            ignore_patterns.len()
        );
        assert!(
            omit_patterns.len() >= 50,
            "global-omitcontent has too few patterns: {}",
            omit_patterns.len()
        );
    }

    #[test]
    fn test_critical_patterns_present() {
        let ignore_content = include_str!("../templates/global-ignore");
        let omit_content = include_str!("../templates/global-omitcontent");

        let ignore_patterns = parse_patterns(ignore_content);
        let omit_patterns = parse_patterns(omit_content);

        // Critical patterns that must be in ignore
        let critical_ignore = vec![
            "node_modules/",
            ".git/",
            "__pycache__/",
            "target/",
            ".DS_Store",
        ];

        for pattern in critical_ignore {
            assert!(
                ignore_patterns.contains(&pattern.to_string()),
                "Critical ignore pattern missing: {pattern}"
            );
        }

        // Critical patterns that must be in omit
        let critical_omit = vec![
            "package-lock.json",
            "Cargo.lock",
            ".env",
            "*.pdf",
        ];

        for pattern in critical_omit {
            assert!(
                omit_patterns.contains(&pattern.to_string()),
                "Critical omit pattern missing: {pattern}"
            );
        }
    }
}
