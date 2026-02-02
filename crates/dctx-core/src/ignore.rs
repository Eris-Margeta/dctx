use crate::config::{get_global_ignore_path, DEFAULT_IGNORE_FILENAME, HARDCODED_IGNORE_RULES};
use crate::datatypes::{IgnoreRule, PatternType};
use std::fs;
use std::path::Path;

// --- GLOB MATCHING ENGINE ---

fn glob_match(pattern: &[u8], text: &[u8]) -> bool {
    if pattern.is_empty() {
        return text.is_empty();
    }

    if pattern.starts_with(b"**") {
        if pattern.len() == 2 {
            return true;
        }
        if pattern.len() > 2 && pattern[2] == b'/' {
            let rest_pattern = &pattern[3..];
            if glob_match(rest_pattern, text) {
                return true;
            }
            if !text.is_empty() {
                return glob_match(pattern, &text[1..]);
            }
            return false;
        }
    }

    if pattern[0] == b'*' {
        let rest_pattern = &pattern[1..];
        if glob_match(rest_pattern, text) {
            return true;
        }
        if !text.is_empty() && text[0] != b'/' {
            return glob_match(pattern, &text[1..]);
        }
        return false;
    }

    if pattern[0] == b'?' {
        if text.is_empty() || text[0] == b'/' {
            return false;
        }
        return glob_match(&pattern[1..], &text[1..]);
    }

    if !text.is_empty() && pattern[0] == text[0] {
        return glob_match(&pattern[1..], &text[1..]);
    }

    false
}

// --- PARSING ---

pub fn parse_ignore_pattern_line(line: &str) -> Option<IgnoreRule> {
    let mut trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }

    let mut rule = IgnoreRule::new();

    if trimmed.starts_with('!') {
        rule.is_negation = true;
        trimmed = &trimmed[1..];
    }

    if trimmed.ends_with('/') {
        rule.is_dir_only = true;
        trimmed = &trimmed[..trimmed.len() - 1];
    }

    let has_internal_slash = trimmed.contains('/');
    let has_glob_chars = trimmed.contains('*') || trimmed.contains('?') || trimmed.contains('[');

    if has_glob_chars {
        if trimmed.starts_with('*')
            && !trimmed[1..].contains('*')
            && !trimmed.contains('/')
            && !trimmed.contains('?')
        {
            rule.pattern_type = PatternType::Suffix;
            rule.pattern = trimmed[1..].to_string();
        } else if trimmed.ends_with("/*") && !trimmed[..trimmed.len() - 2].contains('*') {
            rule.pattern_type = PatternType::Prefix;
            rule.pattern = trimmed[..trimmed.len() - 1].to_string();
        } else {
            rule.pattern_type = PatternType::Glob;
            rule.pattern = trimmed.to_string();
        }
    } else if has_internal_slash {
        rule.pattern_type = PatternType::Path;
        rule.pattern = trimmed.to_string();
    } else {
        rule.pattern_type = PatternType::Basename;
        rule.pattern = trimmed.to_string();
    }
    Some(rule)
}

// --- MATCHING ---

pub fn should_ignore_item(
    item_relative_path: &str,
    item_name: &str,
    is_item_dir: bool,
    rules: &[IgnoreRule],
) -> bool {
    if rules.is_empty() {
        return false;
    }

    // DEBUG: Log checks for our problem directories
    let is_debug_target = item_name.contains("turbo") || item_name.contains("localstack") || item_name.contains("pnpm");
    if is_debug_target {
        eprintln!("🔎 IGNORE CHECK: name='{}', rel_path='{}', is_dir={}, rules_count={}",
            item_name, item_relative_path, is_item_dir, rules.len());
    }

    let mut is_ignored = false;

    for rule in rules {
        if rule.is_dir_only && !is_item_dir {
            continue;
        }

        let matched = match rule.pattern_type {
            PatternType::Invalid => false,
            PatternType::Path => item_relative_path == rule.pattern,
            PatternType::Prefix => item_relative_path.starts_with(&rule.pattern),
            PatternType::Basename => item_name == rule.pattern,
            PatternType::Suffix => item_name.ends_with(&rule.pattern),
            PatternType::Glob => {
                if rule.pattern.contains('/') {
                    glob_match(rule.pattern.as_bytes(), item_relative_path.as_bytes())
                } else {
                    glob_match(rule.pattern.as_bytes(), item_name.as_bytes())
                }
            }
        };

        if is_debug_target && matched {
            eprintln!("  ✅ MATCH: pattern='{}' (type={:?}, dir_only={}) matched '{}' => is_ignored={}",
                rule.pattern, rule.pattern_type, rule.is_dir_only, item_name, !rule.is_negation);
        }

        if matched {
            is_ignored = !rule.is_negation;
        }
    }

    if is_debug_target {
        eprintln!("  FINAL RESULT: is_ignored={}", is_ignored);
    }

    is_ignored
}

// --- LOADING ---

fn load_rules_from_file(filepath: &Path, rules: &mut Vec<IgnoreRule>) {
    if let Ok(content) = fs::read_to_string(filepath) {
        for line in content.lines() {
            if let Some(rule) = parse_ignore_pattern_line(line) {
                rules.push(rule);
            }
        }
    }
}

pub fn load_ignore_rules(base_dir_path: &Path) -> Vec<IgnoreRule> {
    let mut rules = Vec::new();

    for &p in HARDCODED_IGNORE_RULES {
        if let Some(rule) = parse_ignore_pattern_line(p) {
            rules.push(rule);
        }
    }

    if let Some(global_path) = get_global_ignore_path() {
        if global_path.exists() {
            load_rules_from_file(&global_path, &mut rules);
        }
    }

    let project_ignore_path = base_dir_path.join(DEFAULT_IGNORE_FILENAME);
    if project_ignore_path.exists() {
        load_rules_from_file(&project_ignore_path, &mut rules);
    }

    rules
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- GLOB MATCHING TESTS ---

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
        assert!(!should_ignore_item("test.txt", "test.txt", false, &rules));
    }

    #[test]
    fn test_should_ignore_basename_match() {
        let rules = vec![parse_ignore_pattern_line("node_modules").unwrap()];
        assert!(should_ignore_item(
            "node_modules",
            "node_modules",
            true,
            &rules
        ));
        assert!(should_ignore_item(
            "path/to/node_modules",
            "node_modules",
            true,
            &rules
        ));
        assert!(!should_ignore_item(
            "my_modules",
            "my_modules",
            true,
            &rules
        ));
    }

    #[test]
    fn test_should_ignore_suffix_match() {
        let rules = vec![parse_ignore_pattern_line("*.log").unwrap()];
        assert!(should_ignore_item("debug.log", "debug.log", false, &rules));
        assert!(should_ignore_item(
            "path/to/error.log",
            "error.log",
            false,
            &rules
        ));
        assert!(!should_ignore_item("log.txt", "log.txt", false, &rules));
    }

    #[test]
    fn test_should_ignore_path_match() {
        let rules = vec![parse_ignore_pattern_line("src/generated").unwrap()];
        assert!(should_ignore_item(
            "src/generated",
            "generated",
            true,
            &rules
        ));
        assert!(!should_ignore_item(
            "other/generated",
            "generated",
            true,
            &rules
        ));
    }

    #[test]
    fn test_should_ignore_prefix_match() {
        let rules = vec![parse_ignore_pattern_line("build/*").unwrap()];
        assert!(should_ignore_item(
            "build/output.o",
            "output.o",
            false,
            &rules
        ));
        assert!(should_ignore_item(
            "build/sub/file.txt",
            "file.txt",
            false,
            &rules
        ));
        assert!(!should_ignore_item(
            "mybuild/file.txt",
            "file.txt",
            false,
            &rules
        ));
    }

    #[test]
    fn test_should_ignore_dir_only_rule() {
        let rules = vec![parse_ignore_pattern_line("cache/").unwrap()];
        assert!(should_ignore_item("cache", "cache", true, &rules));
        assert!(!should_ignore_item("cache", "cache", false, &rules)); // File named "cache" should NOT be ignored
    }

    #[test]
    fn test_should_ignore_negation() {
        let rules = vec![
            parse_ignore_pattern_line("*.log").unwrap(),
            parse_ignore_pattern_line("!important.log").unwrap(),
        ];
        assert!(should_ignore_item("debug.log", "debug.log", false, &rules));
        assert!(!should_ignore_item(
            "important.log",
            "important.log",
            false,
            &rules
        ));
    }

    #[test]
    fn test_should_ignore_negation_order_matters() {
        // First negate, then ignore - last match wins
        let rules = vec![
            parse_ignore_pattern_line("!keep.log").unwrap(),
            parse_ignore_pattern_line("*.log").unwrap(),
        ];
        // Both rules match keep.log, but the last one (*.log which is NOT negation) wins
        assert!(should_ignore_item("keep.log", "keep.log", false, &rules));
    }

    #[test]
    fn test_should_ignore_glob_with_path() {
        let rules = vec![parse_ignore_pattern_line("**/test_*.rs").unwrap()];
        assert!(should_ignore_item(
            "test_main.rs",
            "test_main.rs",
            false,
            &rules
        ));
        assert!(should_ignore_item(
            "src/test_util.rs",
            "test_util.rs",
            false,
            &rules
        ));
        assert!(!should_ignore_item("main.rs", "main.rs", false, &rules));
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
            &rules
        ));
        assert!(should_ignore_item(
            "__pycache__",
            "__pycache__",
            true,
            &rules
        ));
        assert!(should_ignore_item(
            "mypackage.egg-info",
            "mypackage.egg-info",
            true,
            &rules
        ));
        assert!(!should_ignore_item("setup.py", "setup.py", false, &rules));
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
            &rules
        ));

        // Depth 1
        assert!(should_ignore_item(
            "frontend/node_modules",
            "node_modules",
            true,
            &rules
        ));

        // Depth 2
        assert!(should_ignore_item(
            "apps/web/node_modules",
            "node_modules",
            true,
            &rules
        ));

        // Depth 3+
        assert!(should_ignore_item(
            "monorepo/packages/ui/node_modules",
            "node_modules",
            true,
            &rules
        ));
    }

    #[test]
    fn test_target_directory_at_multiple_depths() {
        let rules = vec![parse_ignore_pattern_line("target/").unwrap()];

        // Root level
        assert!(should_ignore_item("target", "target", true, &rules));

        // Depth 1
        assert!(should_ignore_item(
            "crates/target",
            "target",
            true,
            &rules
        ));

        // Depth 2
        assert!(should_ignore_item(
            "rust/backend/target",
            "target",
            true,
            &rules
        ));

        // Depth 3+
        assert!(should_ignore_item(
            "workspace/crates/core/target",
            "target",
            true,
            &rules
        ));
    }

    #[test]
    fn test_git_directory_at_multiple_depths() {
        let rules = vec![parse_ignore_pattern_line(".git/").unwrap()];

        // Root level (typical)
        assert!(should_ignore_item(".git", ".git", true, &rules));

        // Submodules at depth 1
        assert!(should_ignore_item("vendor/lib/.git", ".git", true, &rules));

        // Nested repos at depth 2+
        assert!(should_ignore_item(
            "external/projects/foo/.git",
            ".git",
            true,
            &rules
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
            &rules
        ));

        // Depth 1
        assert!(should_ignore_item(
            "src/__pycache__",
            "__pycache__",
            true,
            &rules
        ));

        // Depth 2
        assert!(should_ignore_item(
            "backend/api/__pycache__",
            "__pycache__",
            true,
            &rules
        ));

        // Depth 3+
        assert!(should_ignore_item(
            "services/ml/models/training/__pycache__",
            "__pycache__",
            true,
            &rules
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
        assert!(should_ignore_item("build", "build", true, &rules));
        assert!(should_ignore_item("dist", "dist", true, &rules));
        assert!(should_ignore_item("out", "out", true, &rules));

        // Depth 1
        assert!(should_ignore_item("frontend/build", "build", true, &rules));
        assert!(should_ignore_item("backend/dist", "dist", true, &rules));
        assert!(should_ignore_item("cli/out", "out", true, &rules));

        // Depth 2+
        assert!(should_ignore_item(
            "apps/web/frontend/build",
            "build",
            true,
            &rules
        ));
        assert!(should_ignore_item(
            "packages/ui/lib/dist",
            "dist",
            true,
            &rules
        ));
    }

    #[test]
    fn test_wildcard_extensions_at_multiple_depths() {
        let rules = vec![
            parse_ignore_pattern_line("*.pyc").unwrap(),
            parse_ignore_pattern_line("*.class").unwrap(),
            parse_ignore_pattern_line("*.o").unwrap(),
        ];

        // Root level
        assert!(should_ignore_item("module.pyc", "module.pyc", false, &rules));
        assert!(should_ignore_item("Main.class", "Main.class", false, &rules));
        assert!(should_ignore_item("lib.o", "lib.o", false, &rules));

        // Depth 1
        assert!(should_ignore_item(
            "src/module.pyc",
            "module.pyc",
            false,
            &rules
        ));
        assert!(should_ignore_item(
            "bin/Main.class",
            "Main.class",
            false,
            &rules
        ));
        assert!(should_ignore_item("build/lib.o", "lib.o", false, &rules));

        // Depth 3+
        assert!(should_ignore_item(
            "backend/services/api/module.pyc",
            "module.pyc",
            false,
            &rules
        ));
        assert!(should_ignore_item(
            "java/src/com/example/Main.class",
            "Main.class",
            false,
            &rules
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
        assert!(should_ignore_item(".DS_Store", ".DS_Store", false, &rules));
        assert!(should_ignore_item("Thumbs.db", "Thumbs.db", false, &rules));
        assert!(should_ignore_item(".directory", ".directory", false, &rules));

        // Depth 1
        assert!(should_ignore_item(
            "assets/.DS_Store",
            ".DS_Store",
            false,
            &rules
        ));
        assert!(should_ignore_item(
            "images/Thumbs.db",
            "Thumbs.db",
            false,
            &rules
        ));

        // Depth 3+
        assert!(should_ignore_item(
            "public/assets/images/.DS_Store",
            ".DS_Store",
            false,
            &rules
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
        assert!(should_ignore_item(".git", ".git", true, &rules));
        assert!(should_ignore_item(".svn", ".svn", true, &rules));
        assert!(should_ignore_item(".hg", ".hg", true, &rules));

        // Nested in submodules/external repos
        assert!(should_ignore_item("vendor/lib/.git", ".git", true, &rules));
        assert!(should_ignore_item(
            "external/legacy/.svn",
            ".svn",
            true,
            &rules
        ));
        assert!(should_ignore_item("third-party/old/.hg", ".hg", true, &rules));
    }

    #[test]
    fn test_ide_directories_at_multiple_depths() {
        let rules = vec![
            parse_ignore_pattern_line(".idea/").unwrap(),
            parse_ignore_pattern_line(".vscode/").unwrap(),
            parse_ignore_pattern_line(".vs/").unwrap(),
        ];

        // Root level
        assert!(should_ignore_item(".idea", ".idea", true, &rules));
        assert!(should_ignore_item(".vscode", ".vscode", true, &rules));
        assert!(should_ignore_item(".vs", ".vs", true, &rules));

        // Nested in subprojects
        assert!(should_ignore_item(
            "frontend/.idea",
            ".idea",
            true,
            &rules
        ));
        assert!(should_ignore_item(
            "backend/.vscode",
            ".vscode",
            true,
            &rules
        ));
        assert!(should_ignore_item("dotnet/api/.vs", ".vs", true, &rules));
    }

    #[test]
    fn test_glob_patterns_at_multiple_depths() {
        let rules = vec![parse_ignore_pattern_line("**/*.log").unwrap()];

        // Root level
        assert!(should_ignore_item("error.log", "error.log", false, &rules));

        // Depth 1
        assert!(should_ignore_item("logs/error.log", "error.log", false, &rules));

        // Depth 2
        assert!(should_ignore_item(
            "var/logs/error.log",
            "error.log",
            false,
            &rules
        ));

        // Depth 5+
        assert!(should_ignore_item(
            "apps/backend/services/api/logs/error.log",
            "error.log",
            false,
            &rules
        ));

        // Verify non-.log files are not matched
        assert!(!should_ignore_item(
            "apps/backend/services/api/logs/error.txt",
            "error.txt",
            false,
            &rules
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
            &rules
        ));
        assert!(should_ignore_item(
            "crates/core/target",
            "target",
            true,
            &rules
        ));
        assert!(should_ignore_item(
            "backend/api/__pycache__",
            "__pycache__",
            true,
            &rules
        ));
        assert!(should_ignore_item(
            "services/ml/models/trainer.pyc",
            "trainer.pyc",
            false,
            &rules
        ));
        assert!(should_ignore_item(
            "frontend/dist",
            "dist",
            true,
            &rules
        ));
        assert!(should_ignore_item(
            "public/assets/.DS_Store",
            ".DS_Store",
            false,
            &rules
        ));

        // Verify non-ignored files
        assert!(!should_ignore_item(
            "apps/web/package.json",
            "package.json",
            false,
            &rules
        ));
        assert!(!should_ignore_item(
            "crates/core/Cargo.toml",
            "Cargo.toml",
            false,
            &rules
        ));
        assert!(!should_ignore_item(
            "backend/api/main.py",
            "main.py",
            false,
            &rules
        ));
    }
}
