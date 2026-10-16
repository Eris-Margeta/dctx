//! Original dctx-core ignore test assertions, group 5.
//! Verifies SPEC-MAP-MODE section 6.1 compatibility.
//! Uses the scan engine and temporary rule directories.

#[cfg(test)]
mod tests {
    use super::super::*;
    #[test]
    fn test_glob_iterative_double_star_at_end() {
        assert!(glob_match(b"src/**", b"src/"));
        assert!(glob_match(b"src/**", b"src/file"));
        assert!(glob_match(b"src/**", b"src/a/b/c"));
    }

    #[test]
    fn test_glob_iterative_literal_with_special_chars() {
        assert!(glob_match(b"test-file.rs", b"test-file.rs"));
        assert!(glob_match(b"test_file.rs", b"test_file.rs"));
        assert!(glob_match(b"test.backup.rs", b"test.backup.rs"));
    }

    // --- PHASE 10.8.4: CACHING TESTS ---

    #[test]
    fn test_cache_basic_functionality() {
        let temp = tempfile::TempDir::new().unwrap();
        let engine = IgnoreEngine::build(temp.path(), false).unwrap();

        let rules = vec![parse_ignore_pattern_line("**/*.log").unwrap()];

        // First call - cache miss
        let result1 = should_ignore_item_cached(
            "logs/error.log",
            "error.log",
            false,
            &rules,
            false,
            Some(&engine.match_cache),
        );
        assert!(result1);

        // Second call - cache hit (same path)
        let result2 = should_ignore_item_cached(
            "logs/error.log",
            "error.log",
            false,
            &rules,
            false,
            Some(&engine.match_cache),
        );
        assert!(result2);

        // Cache should have 1 entry
        let stats = engine.cache_stats();
        assert_eq!(stats.entries, 1);
    }

    #[test]
    fn test_cache_different_paths_different_entries() {
        let temp = tempfile::TempDir::new().unwrap();
        let engine = IgnoreEngine::build(temp.path(), false).unwrap();

        // Use a real glob pattern (not optimized to Suffix)
        let rules = vec![parse_ignore_pattern_line("**/*.log").unwrap()];

        should_ignore_item_cached(
            "logs/error.log",
            "error.log",
            false,
            &rules,
            false,
            Some(&engine.match_cache),
        );
        should_ignore_item_cached(
            "logs/debug.log",
            "debug.log",
            false,
            &rules,
            false,
            Some(&engine.match_cache),
        );
        should_ignore_item_cached(
            "logs/access.log",
            "access.log",
            false,
            &rules,
            false,
            Some(&engine.match_cache),
        );

        // Cache should have 3 entries (different paths)
        let stats = engine.cache_stats();
        assert_eq!(stats.entries, 3);
    }

    #[test]
    fn test_cache_only_caches_glob_patterns() {
        let temp = tempfile::TempDir::new().unwrap();
        let engine = IgnoreEngine::build(temp.path(), false).unwrap();

        let rules = vec![
            parse_ignore_pattern_line("node_modules").unwrap(), // Basename - no caching
            parse_ignore_pattern_line("*.log").unwrap(),        // Suffix (optimized) - no caching
            parse_ignore_pattern_line("**/test_*.rs").unwrap(), // Glob - cached
        ];

        should_ignore_item_cached(
            "node_modules",
            "node_modules",
            true,
            &rules,
            false,
            Some(&engine.match_cache),
        );
        should_ignore_item_cached(
            "error.log",
            "error.log",
            false,
            &rules,
            false,
            Some(&engine.match_cache),
        );
        should_ignore_item_cached(
            "src/test_main.rs",
            "test_main.rs",
            false,
            &rules,
            false,
            Some(&engine.match_cache),
        );

        // Cache should have 3 entries (glob pattern checked against 3 different paths)
        // Each call checks all rules, so glob rule creates cache entry for each path
        let stats = engine.cache_stats();
        assert_eq!(stats.entries, 3); // Glob pattern cached for each of 3 paths checked
    }

    #[test]
    fn test_cache_respects_negation_semantics() {
        let temp = tempfile::TempDir::new().unwrap();
        let engine = IgnoreEngine::build(temp.path(), false).unwrap();

        let rules = vec![
            parse_ignore_pattern_line("*.log").unwrap(),
            parse_ignore_pattern_line("!important.log").unwrap(),
        ];

        // Normal .log file should be ignored
        assert!(should_ignore_item_cached(
            "debug.log",
            "debug.log",
            false,
            &rules,
            false,
            Some(&engine.match_cache)
        ));

        // important.log should NOT be ignored (negation)
        assert!(!should_ignore_item_cached(
            "important.log",
            "important.log",
            false,
            &rules,
            false,
            Some(&engine.match_cache)
        ));

        // Cached result should still respect negation on second call
        assert!(!should_ignore_item_cached(
            "important.log",
            "important.log",
            false,
            &rules,
            false,
            Some(&engine.match_cache)
        ));
    }

    #[test]
    fn test_cache_memory_usage_reasonable() {
        let temp = tempfile::TempDir::new().unwrap();
        let engine = IgnoreEngine::build(temp.path(), false).unwrap();

        let rules = vec![parse_ignore_pattern_line("**/*.rs").unwrap()];

        // Simulate 1000 files
        for i in 0..1000 {
            let path = format!("src/file{}.rs", i);
            let name = format!("file{}.rs", i);
            should_ignore_item_cached(
                &path,
                &name,
                false,
                &rules,
                false,
                Some(&engine.match_cache),
            );
        }

        let stats = engine.cache_stats();
        // Cache should have 1000 entries
        assert_eq!(stats.entries, 1000);
        // Memory should be reasonable (< 100KB for 1000 entries)
        assert!(stats.memory_bytes < 100_000);
    }

    #[test]
    fn test_cache_hash_collisions_dont_break_correctness() {
        let temp = tempfile::TempDir::new().unwrap();
        let engine = IgnoreEngine::build(temp.path(), false).unwrap();

        let rules = vec![
            parse_ignore_pattern_line("*.log").unwrap(),
            parse_ignore_pattern_line("*.txt").unwrap(),
        ];

        // Even if paths hash to same value, rule_index in cache key prevents collision
        assert!(should_ignore_item_cached(
            "file.log",
            "file.log",
            false,
            &rules,
            false,
            Some(&engine.match_cache)
        ));
        assert!(!should_ignore_item_cached(
            "file.rs",
            "file.rs",
            false,
            &rules,
            false,
            Some(&engine.match_cache)
        ));
        assert!(should_ignore_item_cached(
            "file.txt",
            "file.txt",
            false,
            &rules,
            false,
            Some(&engine.match_cache)
        ));
    }

    #[test]
    fn test_cache_stats_accuracy() {
        let temp = tempfile::TempDir::new().unwrap();
        let engine = IgnoreEngine::build(temp.path(), false).unwrap();

        let initial_stats = engine.cache_stats();
        assert_eq!(initial_stats.entries, 0);
        assert!(initial_stats.capacity >= 10_000); // Pre-allocated

        // Use real glob pattern (not optimized to Suffix)
        let rules = vec![parse_ignore_pattern_line("**/*.log").unwrap()];
        should_ignore_item_cached(
            "logs/error.log",
            "error.log",
            false,
            &rules,
            false,
            Some(&engine.match_cache),
        );

        let after_stats = engine.cache_stats();
        assert_eq!(after_stats.entries, 1);
        assert!(after_stats.memory_bytes > 0);
    }
}
