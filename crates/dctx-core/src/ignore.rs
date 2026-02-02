use crate::config::{get_global_ignore_path, get_global_omit_content_path, DEFAULT_IGNORE_FILENAME, HARDCODED_IGNORE_RULES, HARDCODED_OMIT_CONTENT_RULES};
use crate::datatypes::{IgnoreRule, PatternType};
use crate::domain::{FileHandlingDecision, ItemContext};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

// --- ERROR TYPES ---

/// Errors that can occur during ignore engine operations
#[derive(Debug)]
pub enum IgnoreError {
    /// Failed to read a rule file (permissions, corruption, etc.)
    FileReadError {
        path: PathBuf,
        source: io::Error,
    },
}

impl std::fmt::Display for IgnoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IgnoreError::FileReadError { path, source } => {
                write!(f, "Failed to read ignore file '{}': {}", path.display(), source)
            }
        }
    }
}

impl std::error::Error for IgnoreError {}

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

    // Strip inline comments (everything after unescaped #)
    // Note: We don't handle escaped # for simplicity - gitignore rarely uses it
    if let Some(comment_pos) = trimmed.find('#') {
        trimmed = trimmed[..comment_pos].trim_end();
        if trimmed.is_empty() {
            return None;
        }
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

fn load_rules_from_file(filepath: &Path, rules: &mut Vec<IgnoreRule>) -> Result<(), IgnoreError> {
    let content = fs::read_to_string(filepath)
        .map_err(|e| IgnoreError::FileReadError {
            path: filepath.to_path_buf(),
            source: e,
        })?;

    for line in content.lines() {
        if let Some(rule) = parse_ignore_pattern_line(line) {
            rules.push(rule);
        }
    }

    Ok(())
}

// --- IGNORE ENGINE ---

/// Statistics for diagnostic output
#[derive(Debug, Clone)]
pub struct EngineStats {
    hardcoded_ignore_count: usize,
    global_ignore_count: usize,
    local_ignore_count: usize,
    hardcoded_omit_count: usize,
    global_omit_count: usize,
    local_omit_count: usize,
}

/// Pure ignore decision engine with 3-layer rule loading
///
/// This engine is responsible for making file handling decisions based on:
/// - Hardcoded rules (built into binary)
/// - Global rules (~/.config/dctx/ignore)
/// - Local rules (.dctxignore in project)
///
/// The engine is stateless after construction and thread-safe.
pub struct IgnoreEngine {
    /// Rules for complete exclusion (don't show in tree, don't recurse)
    ignore_rules: Vec<IgnoreRule>,

    /// Rules for content omission (show in tree, omit content)
    omit_rules: Vec<IgnoreRule>,

    /// Statistics for diagnostic output
    stats: EngineStats,
}

impl IgnoreEngine {
    /// Build a new ignore engine with rules from all 3 layers
    ///
    /// # Layers (in order of precedence):
    /// 1. Hardcoded rules (HARDCODED_IGNORE_RULES constant)
    /// 2. Global rules (~/.config/dctx/ignore and ~/.config/dctx/omitcontent)
    /// 3. Local rules (.dctxignore and .dctxomitcontent in base_dir)
    ///
    /// # Errors
    /// Returns `IgnoreError` if:
    /// - A rule file exists but cannot be read (permissions error)
    ///
    /// Note: Missing files are NOT an error - they are simply skipped.
    pub fn build(base_dir: &Path) -> Result<Self, IgnoreError> {
        let mut ignore_rules = Vec::new();
        let mut omit_rules = Vec::new();

        // Layer 1: Hardcoded ignore rules
        let hardcoded_ignore_start = ignore_rules.len();
        for &pattern in HARDCODED_IGNORE_RULES {
            if let Some(rule) = parse_ignore_pattern_line(pattern) {
                ignore_rules.push(rule);
            }
        }
        let hardcoded_ignore_count = ignore_rules.len() - hardcoded_ignore_start;

        // Layer 1: Hardcoded omit rules
        let hardcoded_omit_start = omit_rules.len();
        for &pattern in HARDCODED_OMIT_CONTENT_RULES {
            if let Some(rule) = parse_ignore_pattern_line(pattern) {
                omit_rules.push(rule);
            }
        }
        let hardcoded_omit_count = omit_rules.len() - hardcoded_omit_start;

        // Layer 2: Global ignore rules
        let global_ignore_start = ignore_rules.len();
        if let Some(global_path) = get_global_ignore_path() {
            if global_path.exists() {
                load_rules_from_file(&global_path, &mut ignore_rules)?;
            }
        }
        let global_ignore_count = ignore_rules.len() - global_ignore_start;

        // Layer 2: Global omit rules
        let global_omit_start = omit_rules.len();
        if let Some(global_path) = get_global_omit_content_path() {
            if global_path.exists() {
                load_rules_from_file(&global_path, &mut omit_rules)?;
            }
        }
        let global_omit_count = omit_rules.len() - global_omit_start;

        // Layer 3: Local ignore rules
        let local_ignore_start = ignore_rules.len();
        let project_ignore_path = base_dir.join(DEFAULT_IGNORE_FILENAME);
        if project_ignore_path.exists() {
            load_rules_from_file(&project_ignore_path, &mut ignore_rules)?;
        }
        let local_ignore_count = ignore_rules.len() - local_ignore_start;

        // Layer 3: Local omit rules (future support)
        let local_omit_count = 0;

        let stats = EngineStats {
            hardcoded_ignore_count,
            global_ignore_count,
            local_ignore_count,
            hardcoded_omit_count,
            global_omit_count,
            local_omit_count,
        };

        // Log statistics
        eprintln!("🔧 IgnoreEngine loaded:");
        eprintln!("   IGNORE: {} hardcoded + {} global + {} local = {} total",
            stats.hardcoded_ignore_count,
            stats.global_ignore_count,
            stats.local_ignore_count,
            stats.hardcoded_ignore_count + stats.global_ignore_count + stats.local_ignore_count
        );
        eprintln!("   OMIT:   {} hardcoded + {} global + {} local = {} total",
            stats.hardcoded_omit_count,
            stats.global_omit_count,
            stats.local_omit_count,
            stats.hardcoded_omit_count + stats.global_omit_count + stats.local_omit_count
        );

        Ok(Self {
            ignore_rules,
            omit_rules,
            stats,
        })
    }

    /// Make a file handling decision for the given item
    ///
    /// This is the SINGLE DECISION POINT for all file handling logic.
    ///
    /// # Decision Logic
    /// 1. Check if item matches any ignore rule
    ///    - If yes and debug_mode: return DebugVisible
    ///    - If yes and !debug_mode: return Exclude
    /// 2. Check if item matches any omit rule
    ///    - If yes: return OmitContent
    /// 3. Otherwise: return Include
    pub fn decide(&self, context: &ItemContext) -> FileHandlingDecision {
        // Check ignore rules first (most restrictive)
        if should_ignore_item(
            context.relative_path,
            context.name,
            context.is_directory,
            &self.ignore_rules,
        ) {
            return if context.debug_mode {
                FileHandlingDecision::DebugVisible
            } else {
                FileHandlingDecision::Exclude
            };
        }

        // Check omit rules second
        if should_ignore_item(
            context.relative_path,
            context.name,
            context.is_directory,
            &self.omit_rules,
        ) {
            return FileHandlingDecision::OmitContent;
        }

        // Default: include everything
        FileHandlingDecision::Include
    }

    /// Explain which rules matched for diagnostic purposes
    ///
    /// Returns a human-readable string showing:
    /// - Final decision
    /// - Which rules matched
    /// - Source of each matching rule (hardcoded/global/local)
    ///
    /// Used for CLI `--explain-ignore` flag and debugging.
    pub fn explain(&self, context: &ItemContext) -> String {
        let decision = self.decide(context);
        let mut output = String::new();

        output.push_str(&format!("Item: {}\n", context.relative_path));
        output.push_str(&format!("Decision: {:?}\n", decision));
        output.push_str("Matching Rules:\n");

        // Check which ignore rules matched
        for (i, rule) in self.ignore_rules.iter().enumerate() {
            if matches_rule(context, rule) {
                let source = if i < self.stats.hardcoded_ignore_count {
                    "hardcoded"
                } else if i < self.stats.hardcoded_ignore_count + self.stats.global_ignore_count {
                    "global"
                } else {
                    "local"
                };
                output.push_str(&format!("  [IGNORE/{}] {} ({:?})\n",
                    source, rule.pattern, rule.pattern_type));
            }
        }

        // Check which omit rules matched
        for (i, rule) in self.omit_rules.iter().enumerate() {
            if matches_rule(context, rule) {
                let source = if i < self.stats.hardcoded_omit_count {
                    "hardcoded"
                } else if i < self.stats.hardcoded_omit_count + self.stats.global_omit_count {
                    "global"
                } else {
                    "local"
                };
                output.push_str(&format!("  [OMIT/{}] {} ({:?})\n",
                    source, rule.pattern, rule.pattern_type));
            }
        }

        output
    }

    /// Get total number of rules loaded
    pub fn total_rules(&self) -> usize {
        self.ignore_rules.len() + self.omit_rules.len()
    }

    /// Get statistics about loaded rules
    pub fn stats(&self) -> &EngineStats {
        &self.stats
    }
}

/// Helper: Check if a single rule matches the context
fn matches_rule(context: &ItemContext, rule: &IgnoreRule) -> bool {
    if rule.is_dir_only && !context.is_directory {
        return false;
    }

    match rule.pattern_type {
        PatternType::Invalid => false,
        PatternType::Path => context.relative_path == rule.pattern,
        PatternType::Prefix => context.relative_path.starts_with(&rule.pattern),
        PatternType::Basename => context.name == rule.pattern,
        PatternType::Suffix => context.name.ends_with(&rule.pattern),
        PatternType::Glob => {
            if rule.pattern.contains('/') {
                glob_match(rule.pattern.as_bytes(), context.relative_path.as_bytes())
            } else {
                glob_match(rule.pattern.as_bytes(), context.name.as_bytes())
            }
        }
    }
}

// --- LEGACY API (DEPRECATED) ---

/// Legacy function for backward compatibility
///
/// # Deprecated
/// Use `IgnoreEngine::build()` instead for better error handling and statistics.
///
/// This function silently ignores errors and provides no visibility into
/// which rules were loaded from which sources.
#[deprecated(since = "0.14.9", note = "Use IgnoreEngine::build() instead")]
pub fn load_ignore_rules(base_dir_path: &Path) -> Vec<IgnoreRule> {
    match IgnoreEngine::build(base_dir_path) {
        Ok(engine) => engine.ignore_rules.clone(),
        Err(e) => {
            eprintln!("⚠️  Warning: Failed to load ignore rules: {}", e);
            // Return only hardcoded rules as fallback
            HARDCODED_IGNORE_RULES
                .iter()
                .filter_map(|&p| parse_ignore_pattern_line(p))
                .collect()
        }
    }
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

    // --- IGNORE ENGINE TESTS ---

    #[test]
    fn test_engine_build_success() {
        let temp = tempfile::TempDir::new().unwrap();
        let engine = IgnoreEngine::build(temp.path()).unwrap();
        assert!(engine.total_rules() > 0); // Should have hardcoded rules
    }

    #[test]
    fn test_engine_loads_hardcoded_rules() {
        let temp = tempfile::TempDir::new().unwrap();
        let engine = IgnoreEngine::build(temp.path()).unwrap();
        assert!(engine.stats().hardcoded_ignore_count > 0);
        assert!(engine.stats().hardcoded_omit_count > 0);
    }

    #[test]
    fn test_engine_loads_global_rules() {
        // This test depends on global config existing
        let temp = tempfile::TempDir::new().unwrap();
        let engine = IgnoreEngine::build(temp.path()).unwrap();
        // Just verify it doesn't panic - actual count depends on environment
        let _ = engine.stats().global_ignore_count;
    }

    #[test]
    fn test_engine_loads_local_rules() {
        let temp = tempfile::TempDir::new().unwrap();
        let ignore_file = temp.path().join(".dctxignore");
        fs::write(&ignore_file, "custom_pattern/\ntest_*.txt\n").unwrap();

        let engine = IgnoreEngine::build(temp.path()).unwrap();
        assert_eq!(engine.stats().local_ignore_count, 2);
    }

    #[test]
    fn test_decide_exclude_for_ignored() {
        let temp = tempfile::TempDir::new().unwrap();
        let engine = IgnoreEngine::build(temp.path()).unwrap();

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
        let engine = IgnoreEngine::build(temp.path()).unwrap();

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
        let engine = IgnoreEngine::build(temp.path()).unwrap();

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
        let engine = IgnoreEngine::build(temp.path()).unwrap();

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
        let engine = IgnoreEngine::build(temp.path()).unwrap();

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
        let engine = IgnoreEngine::build(temp.path()).unwrap();

        // Debug: Check if .turbo pattern exists
        let has_turbo = engine.ignore_rules.iter().any(|r| r.pattern == ".turbo");
        eprintln!("DEBUG: .turbo pattern in ignore_rules: {}", has_turbo);
        if has_turbo {
            let turbo_rule = engine.ignore_rules.iter().find(|r| r.pattern == ".turbo").unwrap();
            eprintln!("DEBUG: .turbo rule: pattern_type={:?}, is_dir_only={}", turbo_rule.pattern_type, turbo_rule.is_dir_only);
        }

        let context = ItemContext {
            relative_path: ".turbo/cache",
            name: ".turbo",
            is_directory: true,
            debug_mode: false,
        };

        let decision = engine.decide(&context);
        assert_eq!(decision, FileHandlingDecision::Exclude,
            ".turbo/ should be excluded. Engine has {} ignore rules, {} omit rules. Has .turbo pattern: {}",
            engine.ignore_rules.len(), engine.omit_rules.len(), has_turbo);
    }

    // REGRESSION TEST: Verify .pnpm-store/ pattern is loaded
    #[test]
    fn test_pnpm_store_pattern_loaded() {
        let temp = tempfile::TempDir::new().unwrap();
        let engine = IgnoreEngine::build(temp.path()).unwrap();

        let context = ItemContext {
            relative_path: ".pnpm-store/v3",
            name: ".pnpm-store",
            is_directory: true,
            debug_mode: false,
        };

        let decision = engine.decide(&context);
        assert_eq!(decision, FileHandlingDecision::Exclude,
            ".pnpm-store/ should be excluded");
    }

    #[test]
    fn test_error_display() {
        let error = IgnoreError::FileReadError {
            path: PathBuf::from("/nonexistent"),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "test"),
        };
        assert!(format!("{}", error).contains("Failed to read"));
    }

    #[test]
    fn test_ignored_directories_never_recursed() {
        let temp = tempfile::TempDir::new().unwrap();
        let engine = IgnoreEngine::build(temp.path()).unwrap();

        // Test that ignored directories return decisions with should_recurse() = false
        let context = ItemContext {
            relative_path: ".git/objects",
            name: ".git",
            is_directory: true,
            debug_mode: false,
        };

        let decision = engine.decide(&context);
        assert_eq!(decision, FileHandlingDecision::Exclude);
        assert!(!decision.should_recurse(), ".git/ should not be recursed into");
    }
}
