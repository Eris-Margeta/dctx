// SPDX-License-Identifier: Apache-2.0
//! Original dctx-core ignore test assertions, group 3.
//! Verifies SPEC-MAP-MODE section 6.1 compatibility.
//! Uses the scan engine and temporary rule directories.

#[cfg(test)]
mod tests {
    use super::super::*;
    #[test]
    fn test_wildcard_extensions_at_multiple_depths() {
        let rules = vec![
            parse_ignore_pattern_line("*.pyc").unwrap(),
            parse_ignore_pattern_line("*.class").unwrap(),
            parse_ignore_pattern_line("*.o").unwrap(),
        ];

        // Root level
        assert!(should_ignore_item(
            "module.pyc",
            "module.pyc",
            false,
            &rules,
            false
        ));
        assert!(should_ignore_item(
            "Main.class",
            "Main.class",
            false,
            &rules,
            false
        ));
        assert!(should_ignore_item("lib.o", "lib.o", false, &rules, false));

        // Depth 1
        assert!(should_ignore_item(
            "src/module.pyc",
            "module.pyc",
            false,
            &rules,
            false
        ));
        assert!(should_ignore_item(
            "bin/Main.class",
            "Main.class",
            false,
            &rules,
            false
        ));
        assert!(should_ignore_item(
            "build/lib.o",
            "lib.o",
            false,
            &rules,
            false
        ));

        // Depth 3+
        assert!(should_ignore_item(
            "backend/services/api/module.pyc",
            "module.pyc",
            false,
            &rules,
            false
        ));
        assert!(should_ignore_item(
            "java/src/com/example/Main.class",
            "Main.class",
            false,
            &rules,
            false
        ));
    }

    #[test]
    fn test_dotfiles_at_multiple_depths() {
        let rules = vec![
            parse_ignore_pattern_line(".DS_Store").unwrap(),
            parse_ignore_pattern_line("Thumbs.db").unwrap(),
            parse_ignore_pattern_line(".directory").unwrap(),
        ];

        // Root level
        assert!(should_ignore_item(
            ".DS_Store",
            ".DS_Store",
            false,
            &rules,
            false
        ));
        assert!(should_ignore_item(
            "Thumbs.db",
            "Thumbs.db",
            false,
            &rules,
            false
        ));
        assert!(should_ignore_item(
            ".directory",
            ".directory",
            false,
            &rules,
            false
        ));

        // Depth 1
        assert!(should_ignore_item(
            "assets/.DS_Store",
            ".DS_Store",
            false,
            &rules,
            false
        ));
        assert!(should_ignore_item(
            "images/Thumbs.db",
            "Thumbs.db",
            false,
            &rules,
            false
        ));

        // Depth 3+
        assert!(should_ignore_item(
            "public/assets/images/.DS_Store",
            ".DS_Store",
            false,
            &rules,
            false
        ));
    }

    #[test]
    fn test_vcs_directories_at_multiple_depths() {
        let rules = vec![
            parse_ignore_pattern_line(".git/").unwrap(),
            parse_ignore_pattern_line(".svn/").unwrap(),
            parse_ignore_pattern_line(".hg/").unwrap(),
        ];

        // Root level
        assert!(should_ignore_item(".git", ".git", true, &rules, false));
        assert!(should_ignore_item(".svn", ".svn", true, &rules, false));
        assert!(should_ignore_item(".hg", ".hg", true, &rules, false));

        // Nested in submodules/external repos
        assert!(should_ignore_item(
            "vendor/lib/.git",
            ".git",
            true,
            &rules,
            false
        ));
        assert!(should_ignore_item(
            "external/legacy/.svn",
            ".svn",
            true,
            &rules,
            false
        ));
        assert!(should_ignore_item(
            "third-party/old/.hg",
            ".hg",
            true,
            &rules,
            false
        ));
    }

    #[test]
    fn test_ide_directories_at_multiple_depths() {
        let rules = vec![
            parse_ignore_pattern_line(".idea/").unwrap(),
            parse_ignore_pattern_line(".vscode/").unwrap(),
            parse_ignore_pattern_line(".vs/").unwrap(),
        ];

        // Root level
        assert!(should_ignore_item(".idea", ".idea", true, &rules, false));
        assert!(should_ignore_item(
            ".vscode", ".vscode", true, &rules, false
        ));
        assert!(should_ignore_item(".vs", ".vs", true, &rules, false));

        // Nested in subprojects
        assert!(should_ignore_item(
            "frontend/.idea",
            ".idea",
            true,
            &rules,
            false
        ));
        assert!(should_ignore_item(
            "backend/.vscode",
            ".vscode",
            true,
            &rules,
            false
        ));
        assert!(should_ignore_item(
            "dotnet/api/.vs",
            ".vs",
            true,
            &rules,
            false
        ));
    }

    #[test]
    fn test_glob_patterns_at_multiple_depths() {
        let rules = vec![parse_ignore_pattern_line("**/*.log").unwrap()];

        // Root level
        assert!(should_ignore_item(
            "error.log",
            "error.log",
            false,
            &rules,
            false
        ));

        // Depth 1
        assert!(should_ignore_item(
            "logs/error.log",
            "error.log",
            false,
            &rules,
            false
        ));

        // Depth 2
        assert!(should_ignore_item(
            "var/logs/error.log",
            "error.log",
            false,
            &rules,
            false
        ));

        // Depth 5+
        assert!(should_ignore_item(
            "apps/backend/services/api/logs/error.log",
            "error.log",
            false,
            &rules,
            false
        ));

        // Verify non-.log files are not matched
        assert!(!should_ignore_item(
            "apps/backend/services/api/logs/error.txt",
            "error.txt",
            false,
            &rules,
            false
        ));
    }

    #[test]
    fn test_complex_real_world_scenario() {
        // Simulate a complex monorepo structure
        let rules = vec![
            parse_ignore_pattern_line("node_modules/").unwrap(),
            parse_ignore_pattern_line("target/").unwrap(),
            parse_ignore_pattern_line(".git/").unwrap(),
            parse_ignore_pattern_line("__pycache__/").unwrap(),
            parse_ignore_pattern_line("*.pyc").unwrap(),
            parse_ignore_pattern_line("build/").unwrap(),
            parse_ignore_pattern_line("dist/").unwrap(),
            parse_ignore_pattern_line(".DS_Store").unwrap(),
        ];

        // Verify ignores at various depths
        assert!(should_ignore_item(
            "apps/web/node_modules",
            "node_modules",
            true,
            &rules,
            false
        ));
        assert!(should_ignore_item(
            "crates/core/target",
            "target",
            true,
            &rules,
            false
        ));
        assert!(should_ignore_item(
            "backend/api/__pycache__",
            "__pycache__",
            true,
            &rules,
            false
        ));
        assert!(should_ignore_item(
            "services/ml/models/trainer.pyc",
            "trainer.pyc",
            false,
            &rules,
            false
        ));
        assert!(should_ignore_item(
            "frontend/dist",
            "dist",
            true,
            &rules,
            false
        ));
        assert!(should_ignore_item(
            "public/assets/.DS_Store",
            ".DS_Store",
            false,
            &rules,
            false
        ));

        // Verify non-ignored files
        assert!(!should_ignore_item(
            "apps/web/package.json",
            "package.json",
            false,
            &rules,
            false
        ));
        assert!(!should_ignore_item(
            "crates/core/Cargo.toml",
            "Cargo.toml",
            false,
            &rules,
            false
        ));
        assert!(!should_ignore_item(
            "backend/api/main.py",
            "main.py",
            false,
            &rules,
            false
        ));
    }

    // --- IGNORE ENGINE TESTS ---
}
