//! Original dctx-core ignore test assertions, group 2.
//! Verifies SPEC-MAP-MODE section 6.1 compatibility.
//! Uses the scan engine and temporary rule directories.

#[cfg(test)]
mod tests {
    use super::super::*;
    #[test]
    fn test_should_ignore_negation_order_matters() {
        // First negate, then ignore - last match wins
        let rules = vec![
            parse_ignore_pattern_line("!keep.log").unwrap(),
            parse_ignore_pattern_line("*.log").unwrap(),
        ];
        // Both rules match keep.log, but the last one (*.log which is NOT negation) wins
        assert!(should_ignore_item(
            "keep.log", "keep.log", false, &rules, false
        ));
    }

    #[test]
    fn test_should_ignore_glob_with_path() {
        let rules = vec![parse_ignore_pattern_line("**/test_*.rs").unwrap()];
        assert!(should_ignore_item(
            "test_main.rs",
            "test_main.rs",
            false,
            &rules,
            false
        ));
        assert!(should_ignore_item(
            "src/test_util.rs",
            "test_util.rs",
            false,
            &rules,
            false
        ));
        assert!(!should_ignore_item(
            "main.rs", "main.rs", false, &rules, false
        ));
    }

    #[test]
    fn test_should_ignore_multiple_rules() {
        let rules = vec![
            parse_ignore_pattern_line("*.pyc").unwrap(),
            parse_ignore_pattern_line("__pycache__/").unwrap(),
            parse_ignore_pattern_line("*.egg-info/").unwrap(),
        ];
        assert!(should_ignore_item(
            "module.pyc",
            "module.pyc",
            false,
            &rules,
            false
        ));
        assert!(should_ignore_item(
            "__pycache__",
            "__pycache__",
            true,
            &rules,
            false
        ));
        assert!(should_ignore_item(
            "mypackage.egg-info",
            "mypackage.egg-info",
            true,
            &rules,
            false
        ));
        assert!(!should_ignore_item(
            "setup.py", "setup.py", false, &rules, false
        ));
    }

    // --- DEPTH TESTING: Verify ignore patterns work at multiple directory levels ---

    #[test]
    fn test_node_modules_at_multiple_depths() {
        let rules = vec![parse_ignore_pattern_line("node_modules/").unwrap()];

        // Root level
        assert!(should_ignore_item(
            "node_modules",
            "node_modules",
            true,
            &rules,
            false
        ));

        // Depth 1
        assert!(should_ignore_item(
            "frontend/node_modules",
            "node_modules",
            true,
            &rules,
            false
        ));

        // Depth 2
        assert!(should_ignore_item(
            "apps/web/node_modules",
            "node_modules",
            true,
            &rules,
            false
        ));

        // Depth 3+
        assert!(should_ignore_item(
            "monorepo/packages/ui/node_modules",
            "node_modules",
            true,
            &rules,
            false
        ));
    }

    #[test]
    fn test_target_directory_at_multiple_depths() {
        let rules = vec![parse_ignore_pattern_line("target/").unwrap()];

        // Root level
        assert!(should_ignore_item("target", "target", true, &rules, false));

        // Depth 1
        assert!(should_ignore_item(
            "crates/target",
            "target",
            true,
            &rules,
            false
        ));

        // Depth 2
        assert!(should_ignore_item(
            "rust/backend/target",
            "target",
            true,
            &rules,
            false
        ));

        // Depth 3+
        assert!(should_ignore_item(
            "workspace/crates/core/target",
            "target",
            true,
            &rules,
            false
        ));
    }

    #[test]
    fn test_git_directory_at_multiple_depths() {
        let rules = vec![parse_ignore_pattern_line(".git/").unwrap()];

        // Root level (typical)
        assert!(should_ignore_item(".git", ".git", true, &rules, false));

        // Submodules at depth 1
        assert!(should_ignore_item(
            "vendor/lib/.git",
            ".git",
            true,
            &rules,
            false
        ));

        // Nested repos at depth 2+
        assert!(should_ignore_item(
            "external/projects/foo/.git",
            ".git",
            true,
            &rules,
            false
        ));
    }

    #[test]
    fn test_pycache_at_multiple_depths() {
        let rules = vec![parse_ignore_pattern_line("__pycache__/").unwrap()];

        // Root level
        assert!(should_ignore_item(
            "__pycache__",
            "__pycache__",
            true,
            &rules,
            false
        ));

        // Depth 1
        assert!(should_ignore_item(
            "src/__pycache__",
            "__pycache__",
            true,
            &rules,
            false
        ));

        // Depth 2
        assert!(should_ignore_item(
            "backend/api/__pycache__",
            "__pycache__",
            true,
            &rules,
            false
        ));

        // Depth 3+
        assert!(should_ignore_item(
            "services/ml/models/training/__pycache__",
            "__pycache__",
            true,
            &rules,
            false
        ));
    }

    #[test]
    fn test_build_directories_at_multiple_depths() {
        let rules = vec![
            parse_ignore_pattern_line("build/").unwrap(),
            parse_ignore_pattern_line("dist/").unwrap(),
            parse_ignore_pattern_line("out/").unwrap(),
        ];

        // Root level
        assert!(should_ignore_item("build", "build", true, &rules, false));
        assert!(should_ignore_item("dist", "dist", true, &rules, false));
        assert!(should_ignore_item("out", "out", true, &rules, false));

        // Depth 1
        assert!(should_ignore_item(
            "frontend/build",
            "build",
            true,
            &rules,
            false
        ));
        assert!(should_ignore_item(
            "backend/dist",
            "dist",
            true,
            &rules,
            false
        ));
        assert!(should_ignore_item("cli/out", "out", true, &rules, false));

        // Depth 2+
        assert!(should_ignore_item(
            "apps/web/frontend/build",
            "build",
            true,
            &rules,
            false
        ));
        assert!(should_ignore_item(
            "packages/ui/lib/dist",
            "dist",
            true,
            &rules,
            false
        ));
    }
}
