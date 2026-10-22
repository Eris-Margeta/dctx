//! Original dctx-core ignore test assertions, group 4.
//! Verifies SPEC-MAP-MODE section 6.1 compatibility.
//! Uses the scan engine and temporary rule directories.

#[cfg(test)]
mod tests {
    use super::super::*;
    #[test]
    fn test_engine_build_success() {
        let temp = tempfile::TempDir::new().unwrap();
        let engine = IgnoreEngine::build(temp.path(), false).unwrap();
        assert!(engine.total_rules() > 0); // Should have hardcoded rules
    }

    #[test]
    fn test_engine_loads_hardcoded_rules() {
        let temp = tempfile::TempDir::new().unwrap();
        let engine = IgnoreEngine::build(temp.path(), false).unwrap();
        assert!(engine.stats().hardcoded_ignore_count > 0);
        assert!(engine.stats().hardcoded_omit_count > 0);
    }

    #[test]
    fn test_engine_loads_global_rules() {
        // This test depends on global config existing
        let temp = tempfile::TempDir::new().unwrap();
        let engine = IgnoreEngine::build(temp.path(), false).unwrap();
        // Just verify it doesn't panic - actual count depends on environment
        let _ = engine.stats().global_ignore_count;
    }

    #[test]
    fn test_engine_loads_local_rules() {
        let temp = tempfile::TempDir::new().unwrap();
        let ignore_file = temp.path().join(".dctxignore");
        fs::write(&ignore_file, "custom_pattern/\ntest_*.txt\n").unwrap();

        let engine = IgnoreEngine::build(temp.path(), false).unwrap();
        assert_eq!(engine.stats().local_ignore_count, 2);
    }

    #[test]
    fn test_decide_exclude_for_ignored() {
        let temp = tempfile::TempDir::new().unwrap();
        let engine = IgnoreEngine::build(temp.path(), false).unwrap();

        let context = ItemContext {
            relative_path: "node_modules/package",
            name: "node_modules",
            is_directory: true,
            debug_mode: false,
        };

        assert_eq!(engine.decide(&context), FileHandlingDecision::Exclude);
    }

    #[test]
    fn test_decide_debug_visible_in_debug_mode() {
        let temp = tempfile::TempDir::new().unwrap();
        let engine = IgnoreEngine::build(temp.path(), false).unwrap();

        let context = ItemContext {
            relative_path: ".git/hooks",
            name: ".git",
            is_directory: true,
            debug_mode: true,
        };

        assert_eq!(engine.decide(&context), FileHandlingDecision::DebugVisible);
    }

    #[test]
    fn test_decide_omit_content_for_lockfiles() {
        let temp = tempfile::TempDir::new().unwrap();
        let engine = IgnoreEngine::build(temp.path(), false).unwrap();

        let context = ItemContext {
            relative_path: "package-lock.json",
            name: "package-lock.json",
            is_directory: false,
            debug_mode: false,
        };

        assert_eq!(engine.decide(&context), FileHandlingDecision::OmitContent);
    }

    #[test]
    fn test_decide_include_for_normal_files() {
        let temp = tempfile::TempDir::new().unwrap();
        let engine = IgnoreEngine::build(temp.path(), false).unwrap();

        let context = ItemContext {
            relative_path: "src/main.rs",
            name: "main.rs",
            is_directory: false,
            debug_mode: false,
        };

        assert_eq!(engine.decide(&context), FileHandlingDecision::Include);
    }

    #[test]
    fn test_explain_produces_output() {
        let temp = tempfile::TempDir::new().unwrap();
        let engine = IgnoreEngine::build(temp.path(), false).unwrap();

        let context = ItemContext {
            relative_path: "node_modules/package",
            name: "node_modules",
            is_directory: true,
            debug_mode: false,
        };

        let explanation = engine.explain(&context);
        assert!(explanation.contains("node_modules"));
        assert!(explanation.contains("Decision:"));
    }

    // REGRESSION TEST: Verify .turbo/ pattern is loaded
    #[test]
    fn test_turbo_pattern_loaded() {
        let temp = tempfile::TempDir::new().unwrap();
        let engine = IgnoreEngine::build(temp.path(), false).unwrap();

        // Debug: Check if .turbo pattern exists
        let has_turbo = engine.ignore_rules.iter().any(|r| r.pattern == ".turbo");
        eprintln!("DEBUG: .turbo pattern in ignore_rules: {has_turbo}");
        if has_turbo {
            let turbo_rule = engine
                .ignore_rules
                .iter()
                .find(|r| r.pattern == ".turbo")
                .unwrap();
            eprintln!(
                "DEBUG: .turbo rule: pattern_type={:?}, is_dir_only={}",
                turbo_rule.pattern_type, turbo_rule.is_dir_only
            );
        }

        let context = ItemContext {
            relative_path: ".turbo/cache",
            name: ".turbo",
            is_directory: true,
            debug_mode: false,
        };

        let decision = engine.decide(&context);
        assert_eq!(
            decision,
            FileHandlingDecision::Exclude,
            ".turbo/ should be excluded. Engine has {} ignore rules, {} omit rules. Has .turbo pattern: {}",
            engine.ignore_rules.len(),
            engine.omit_rules.len(),
            has_turbo
        );
    }

    // REGRESSION TEST: Verify .pnpm-store/ pattern is loaded
    #[test]
    fn test_pnpm_store_pattern_loaded() {
        let temp = tempfile::TempDir::new().unwrap();
        let engine = IgnoreEngine::build(temp.path(), false).unwrap();

        let context = ItemContext {
            relative_path: ".pnpm-store/v3",
            name: ".pnpm-store",
            is_directory: true,
            debug_mode: false,
        };

        let decision = engine.decide(&context);
        assert_eq!(
            decision,
            FileHandlingDecision::Exclude,
            ".pnpm-store/ should be excluded"
        );
    }

    #[test]
    fn test_error_display() {
        let error = IgnoreError::FileReadError {
            path: PathBuf::from("/nonexistent"),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "test"),
        };
        assert!(format!("{error}").contains("Failed to read"));
    }

    #[test]
    fn test_ignored_directories_never_recursed() {
        let temp = tempfile::TempDir::new().unwrap();
        let engine = IgnoreEngine::build(temp.path(), false).unwrap();

        // Test that ignored directories return decisions with should_recurse() = false
        let context = ItemContext {
            relative_path: ".git/objects",
            name: ".git",
            is_directory: true,
            debug_mode: false,
        };

        let decision = engine.decide(&context);
        assert_eq!(decision, FileHandlingDecision::Exclude);
        assert!(
            !decision.should_recurse(),
            ".git/ should not be recursed into"
        );
    }

    // --- PHASE 10.8.4: GLOB MATCHING ITERATIVE TESTS ---

    #[test]
    fn test_glob_iterative_double_star_zero_segments() {
        assert!(glob_match(b"**/test", b"test"));
        assert!(glob_match(b"**/test.rs", b"test.rs"));
    }

    #[test]
    fn test_glob_iterative_double_star_multiple_segments() {
        assert!(glob_match(b"**/test.rs", b"a/test.rs"));
        assert!(glob_match(b"**/test.rs", b"a/b/test.rs"));
        assert!(glob_match(b"**/test.rs", b"a/b/c/d/e/test.rs"));
    }

    #[test]
    fn test_glob_iterative_star_within_segment() {
        assert!(glob_match(b"test*.rs", b"test.rs"));
        assert!(glob_match(b"test*.rs", b"test_main.rs"));
        assert!(glob_match(b"test*.rs", b"testing.rs"));
        assert!(!glob_match(b"test*.rs", b"dir/test.rs")); // * stops at /
    }

    #[test]
    fn test_glob_iterative_question_mark() {
        assert!(glob_match(b"test?.rs", b"test1.rs"));
        assert!(glob_match(b"test?.rs", b"testa.rs"));
        assert!(!glob_match(b"test?.rs", b"test.rs")); // ? requires one char
        assert!(!glob_match(b"test?.rs", b"test12.rs")); // ? is exactly one char
        assert!(!glob_match(b"test?rs", b"test/rs")); // ? doesn't match /
    }

    #[test]
    fn test_glob_iterative_complex_patterns() {
        assert!(glob_match(b"src/**/*.rs", b"src/main.rs"));
        assert!(glob_match(b"src/**/*.rs", b"src/lib/util.rs"));
        assert!(glob_match(b"src/**/*.rs", b"src/a/b/c/test.rs"));
        assert!(!glob_match(b"src/**/*.rs", b"tests/main.rs"));
        assert!(!glob_match(b"src/**/*.rs", b"src/main.txt"));
    }

    #[test]
    fn test_glob_iterative_empty_pattern() {
        assert!(glob_match(b"", b""));
        assert!(!glob_match(b"", b"text"));
        assert!(!glob_match(b"pattern", b""));
    }

    #[test]
    fn test_glob_iterative_pattern_longer_than_text() {
        assert!(!glob_match(b"very_long_pattern", b"short"));
        assert!(!glob_match(b"test.rs.backup", b"test.rs"));
    }

    #[test]
    fn test_glob_iterative_multiple_stars() {
        assert!(glob_match(b"*test*", b"test"));
        assert!(glob_match(b"*test*", b"mytest"));
        assert!(glob_match(b"*test*", b"testing"));
        assert!(glob_match(b"*test*", b"mytesting"));
        assert!(!glob_match(b"*test*", b"my/testing")); // * stops at /
    }
}
