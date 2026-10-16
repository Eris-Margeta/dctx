//! Original dctx-core ignore test assertions, group 1.
//! Verifies SPEC-MAP-MODE section 6.1 compatibility.
//! Uses the scan engine and temporary rule directories.

#[cfg(test)]
mod tests {
    use super::super::*;
    #[test]
    fn test_glob_match_exact() {
        assert!(glob_match(b"hello", b"hello"));
        assert!(!glob_match(b"hello", b"world"));
    }

    #[test]
    fn test_glob_match_single_star() {
        assert!(glob_match(b"*.txt", b"file.txt"));
        assert!(glob_match(b"*.txt", b".txt"));
        assert!(!glob_match(b"*.txt", b"file.rs"));
        assert!(!glob_match(b"*.txt", b"dir/file.txt")); // * doesn't match /
    }

    #[test]
    fn test_glob_match_prefix_star() {
        assert!(glob_match(b"test*", b"test"));
        assert!(glob_match(b"test*", b"test.rs"));
        assert!(glob_match(b"test*", b"testing"));
        assert!(!glob_match(b"test*", b"mytest"));
    }

    #[test]
    fn test_glob_match_question_mark() {
        assert!(glob_match(b"test?", b"tests"));
        assert!(glob_match(b"test?", b"test1"));
        assert!(!glob_match(b"test?", b"test"));
        assert!(!glob_match(b"test?", b"test12"));
        assert!(!glob_match(b"test?", b"test/")); // ? doesn't match /
    }

    #[test]
    fn test_glob_match_double_star() {
        assert!(glob_match(b"**", b"anything"));
        assert!(glob_match(b"**", b"a/b/c"));
        assert!(glob_match(b"**/file.txt", b"file.txt"));
        assert!(glob_match(b"**/file.txt", b"dir/file.txt"));
        assert!(glob_match(b"**/file.txt", b"a/b/c/file.txt"));
    }

    #[test]
    fn test_glob_match_complex() {
        assert!(glob_match(b"src/*.rs", b"src/main.rs"));
        assert!(!glob_match(b"src/*.rs", b"src/sub/main.rs"));
        assert!(glob_match(b"src/**/*.rs", b"src/main.rs"));
        assert!(glob_match(b"src/**/*.rs", b"src/sub/main.rs"));
    }

    #[test]
    fn test_glob_match_empty() {
        assert!(glob_match(b"", b""));
        assert!(!glob_match(b"", b"something"));
        assert!(glob_match(b"*", b""));
    }

    // --- PATTERN PARSING TESTS ---

    #[test]
    fn test_parse_ignore_pattern_empty() {
        assert!(parse_ignore_pattern_line("").is_none());
        assert!(parse_ignore_pattern_line("   ").is_none());
    }

    #[test]
    fn test_parse_ignore_pattern_comment() {
        assert!(parse_ignore_pattern_line("# This is a comment").is_none());
        assert!(parse_ignore_pattern_line("  # Indented comment").is_none());
    }

    #[test]
    fn test_parse_ignore_pattern_basename() {
        let rule = parse_ignore_pattern_line("node_modules").unwrap();
        assert_eq!(rule.pattern, "node_modules");
        assert_eq!(rule.pattern_type, PatternType::Basename);
        assert!(!rule.is_dir_only);
        assert!(!rule.is_negation);
    }

    #[test]
    fn test_parse_ignore_pattern_dir_only() {
        let rule = parse_ignore_pattern_line("build/").unwrap();
        assert_eq!(rule.pattern, "build");
        assert_eq!(rule.pattern_type, PatternType::Basename);
        assert!(rule.is_dir_only);
    }

    #[test]
    fn test_parse_ignore_pattern_negation() {
        let rule = parse_ignore_pattern_line("!important.txt").unwrap();
        assert_eq!(rule.pattern, "important.txt");
        assert!(rule.is_negation);
    }

    #[test]
    fn test_parse_ignore_pattern_path() {
        let rule = parse_ignore_pattern_line("src/generated").unwrap();
        assert_eq!(rule.pattern, "src/generated");
        assert_eq!(rule.pattern_type, PatternType::Path);
    }

    #[test]
    fn test_parse_ignore_pattern_suffix() {
        let rule = parse_ignore_pattern_line("*.log").unwrap();
        assert_eq!(rule.pattern, ".log");
        assert_eq!(rule.pattern_type, PatternType::Suffix);
    }

    #[test]
    fn test_parse_ignore_pattern_prefix() {
        let rule = parse_ignore_pattern_line("build/*").unwrap();
        assert_eq!(rule.pattern, "build/");
        assert_eq!(rule.pattern_type, PatternType::Prefix);
    }

    #[test]
    fn test_parse_ignore_pattern_glob() {
        let rule = parse_ignore_pattern_line("**/*.log").unwrap();
        assert_eq!(rule.pattern, "**/*.log");
        assert_eq!(rule.pattern_type, PatternType::Glob);
    }

    #[test]
    fn test_parse_ignore_pattern_complex_glob() {
        let rule = parse_ignore_pattern_line("config-*-prod.json").unwrap();
        assert_eq!(rule.pattern, "config-*-prod.json");
        assert_eq!(rule.pattern_type, PatternType::Glob);
    }

    // --- SHOULD IGNORE TESTS ---

    #[test]
    fn test_should_ignore_empty_rules() {
        let rules: Vec<IgnoreRule> = vec![];
        assert!(!should_ignore_item(
            "test.txt", "test.txt", false, &rules, false
        ));
    }

    #[test]
    fn test_should_ignore_basename_match() {
        let rules = vec![parse_ignore_pattern_line("node_modules").unwrap()];
        assert!(should_ignore_item(
            "node_modules",
            "node_modules",
            true,
            &rules,
            false
        ));
        assert!(should_ignore_item(
            "path/to/node_modules",
            "node_modules",
            true,
            &rules,
            false
        ));
        assert!(!should_ignore_item(
            "my_modules",
            "my_modules",
            true,
            &rules,
            false
        ));
    }

    #[test]
    fn test_should_ignore_suffix_match() {
        let rules = vec![parse_ignore_pattern_line("*.log").unwrap()];
        assert!(should_ignore_item(
            "debug.log",
            "debug.log",
            false,
            &rules,
            false
        ));
        assert!(should_ignore_item(
            "path/to/error.log",
            "error.log",
            false,
            &rules,
            false
        ));
        assert!(!should_ignore_item(
            "log.txt", "log.txt", false, &rules, false
        ));
    }

    #[test]
    fn test_should_ignore_path_match() {
        let rules = vec![parse_ignore_pattern_line("src/generated").unwrap()];
        assert!(should_ignore_item(
            "src/generated",
            "generated",
            true,
            &rules,
            false
        ));
        assert!(!should_ignore_item(
            "other/generated",
            "generated",
            true,
            &rules,
            false
        ));
    }

    #[test]
    fn test_should_ignore_prefix_match() {
        let rules = vec![parse_ignore_pattern_line("build/*").unwrap()];
        assert!(should_ignore_item(
            "build/output.o",
            "output.o",
            false,
            &rules,
            false
        ));
        assert!(should_ignore_item(
            "build/sub/file.txt",
            "file.txt",
            false,
            &rules,
            false
        ));
        assert!(!should_ignore_item(
            "mybuild/file.txt",
            "file.txt",
            false,
            &rules,
            false
        ));
    }

    #[test]
    fn test_should_ignore_dir_only_rule() {
        let rules = vec![parse_ignore_pattern_line("cache/").unwrap()];
        assert!(should_ignore_item("cache", "cache", true, &rules, false));
        assert!(!should_ignore_item("cache", "cache", false, &rules, false)); // File named "cache" should NOT be ignored
    }

    #[test]
    fn test_should_ignore_negation() {
        let rules = vec![
            parse_ignore_pattern_line("*.log").unwrap(),
            parse_ignore_pattern_line("!important.log").unwrap(),
        ];
        assert!(should_ignore_item(
            "debug.log",
            "debug.log",
            false,
            &rules,
            false
        ));
        assert!(!should_ignore_item(
            "important.log",
            "important.log",
            false,
            &rules,
            false
        ));
    }
}
