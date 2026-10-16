//! Legacy compatibility (DX-B0 bridge): `IgnoreEngine`, which loads rule files from disk and
//! memoizes glob matches. Reading the global and local rule files is IO, so it stays here.
//! Rules, matching and decisions delegate to the pure [`crate::selection`] context.
//! Used by dctx-cli, the desktop, the legacy walker and the pattern-matching bench.

use crate::config::{get_global_ignore_path, get_global_omit_content_path, DEFAULT_IGNORE_FILENAME, HARDCODED_IGNORE_RULES};
use crate::datatypes::{IgnoreRule, PatternType};
use crate::domain::{FileHandlingDecision, ItemContext};
use crate::selection::glob::glob_match;
use crate::selection::policy::{RuleTexts, SelectionPolicy};
use crate::selection::rule::{glob_subject, rule_matches};
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub use crate::selection::rule::parse_ignore_pattern_line;

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

// --- MATCHING ---

pub fn should_ignore_item(
    item_relative_path: &str,
    item_name: &str,
    is_item_dir: bool,
    rules: &[IgnoreRule],
    debug_verbose: bool,
) -> bool {
    should_ignore_item_cached(
        item_relative_path,
        item_name,
        is_item_dir,
        rules,
        debug_verbose,
        None,
    )
}

/// Pattern matching with optional caching (Phase 10.8.4)
///
/// # Caching Strategy
/// - Simple patterns (Basename, Path, Prefix, Suffix): No caching (already <50ns)
/// - Glob patterns: Cache results (1-10μs per match, high redundancy)
/// - Cache key: (rule_index, path_hash) for memory efficiency
/// - Expected hit rate: 30-40% on large monorepos
///
/// Single-rule semantics are [`crate::selection::rule::rule_matches`]; last match wins.
pub fn should_ignore_item_cached(
    item_relative_path: &str,
    item_name: &str,
    is_item_dir: bool,
    rules: &[IgnoreRule],
    debug_verbose: bool,
    cache: Option<&RefCell<HashMap<(usize, u64), bool>>>,
) -> bool {
    if rules.is_empty() {
        return false;
    }

    // DEBUG: Log checks for diagnostic purposes (only with --debug-verbose)
    let is_debug_target = debug_verbose && (item_name.contains("turbo") || item_name.contains("localstack") || item_name.contains("pnpm"));
    if is_debug_target {
        eprintln!("🔎 IGNORE CHECK: name='{}', rel_path='{}', is_dir={}, rules_count={}",
            item_name, item_relative_path, is_item_dir, rules.len());
    }

    let mut is_ignored = false;

    for (rule_idx, rule) in rules.iter().enumerate() {
        let matched = match (rule.pattern_type, cache) {
            (PatternType::Glob, Some(cache_ref)) if !(rule.is_dir_only && !is_item_dir) => {
                let text = glob_subject(rule, item_relative_path, item_name);
                let cache_key = (rule_idx, IgnoreEngine::fast_hash(text));
                let mut cache_map = cache_ref.borrow_mut();
                *cache_map
                    .entry(cache_key)
                    .or_insert_with(|| glob_match(rule.pattern.as_bytes(), text.as_bytes()))
            }
            _ => rule_matches(rule, item_relative_path, item_name, is_item_dir),
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

fn read_rule_file(filepath: &Path) -> Result<Option<String>, IgnoreError> {
    if !filepath.exists() {
        return Ok(None);
    }
    fs::read_to_string(filepath)
        .map(Some)
        .map_err(|e| IgnoreError::FileReadError {
            path: filepath.to_path_buf(),
            source: e,
        })
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

/// Ignore decision engine with 3-layer rule loading
///
/// This engine is responsible for making file handling decisions based on:
/// - Hardcoded rules (built into binary)
/// - Global rules (~/.config/dctx/ignore)
/// - Local rules (.dctxignore in project)
///
/// The decision itself is [`SelectionPolicy::decide`]; this type adds rule-file
/// loading and a glob match cache (Phase 10.8.4, key (rule_index, path_hash)).
pub struct IgnoreEngine {
    /// Ignore and omit rule sets with their layer counts.
    policy: SelectionPolicy,

    /// Statistics for diagnostic output
    stats: EngineStats,

    /// Controls whether detailed debug output is shown
    debug_verbose: bool,

    /// Pattern match result cache (Phase 10.8.4)
    match_cache: RefCell<HashMap<(usize, u64), bool>>,
}

impl IgnoreEngine {
    /// Build a new ignore engine with rules from all 3 layers
    ///
    /// # Layers (in order of precedence):
    /// 1. Hardcoded rules (HARDCODED_IGNORE_RULES constant)
    /// 2. Global rules (~/.config/dctx/ignore and ~/.config/dctx/omitcontent)
    /// 3. Local rules (.dctxignore in base_dir)
    ///
    /// # Errors
    /// Returns `IgnoreError` if a rule file exists but cannot be read.
    /// Missing files are NOT an error - they are simply skipped.
    pub fn build(base_dir: &Path, debug_verbose: bool) -> Result<Self, IgnoreError> {
        let global_ignore = match get_global_ignore_path() {
            Some(path) => read_rule_file(&path)?,
            None => None,
        };
        let global_omit = match get_global_omit_content_path() {
            Some(path) => read_rule_file(&path)?,
            None => None,
        };
        let local_ignore = read_rule_file(&base_dir.join(DEFAULT_IGNORE_FILENAME))?;

        let engine = Self::from_policy(
            SelectionPolicy::from_texts(&RuleTexts {
                global_ignore: global_ignore.as_deref(),
                global_omit: global_omit.as_deref(),
                local_ignore: local_ignore.as_deref(),
                local_omit: None,
            }),
            debug_verbose,
        );

        let stats = &engine.stats;
        let total_ignore = stats.hardcoded_ignore_count + stats.global_ignore_count + stats.local_ignore_count;
        let total_omit = stats.hardcoded_omit_count + stats.global_omit_count + stats.local_omit_count;
        eprintln!("🔧 Loaded {} ignore + {} omit rules (from 3 sources)", total_ignore, total_omit);

        if debug_verbose {
            eprintln!("   IGNORE RULES:");
            eprintln!("     • Hardcoded: {}", stats.hardcoded_ignore_count);
            eprintln!("     • Global: {}", stats.global_ignore_count);
            eprintln!("     • Local: {}", stats.local_ignore_count);
            eprintln!("   OMIT RULES:");
            eprintln!("     • Hardcoded: {}", stats.hardcoded_omit_count);
            eprintln!("     • Global: {}", stats.global_omit_count);
            eprintln!("     • Local: {}", stats.local_omit_count);
        }

        Ok(engine)
    }

    /// Wraps an already-built policy (no IO, no logging).
    pub fn from_policy(policy: SelectionPolicy, debug_verbose: bool) -> Self {
        let ignore = policy.ignore_layers();
        let omit = policy.omit_layers();
        let stats = EngineStats {
            hardcoded_ignore_count: ignore.hardcoded,
            global_ignore_count: ignore.global,
            local_ignore_count: ignore.local,
            hardcoded_omit_count: omit.hardcoded,
            global_omit_count: omit.global,
            local_omit_count: omit.local,
        };
        Self {
            policy,
            stats,
            debug_verbose,
            // Pre-allocate 10,000 entries (typical monorepo scan needs 10k-50k)
            match_cache: RefCell::new(HashMap::with_capacity(10_000)),
        }
    }

    /// The pure selection policy this engine applies.
    pub fn policy(&self) -> &SelectionPolicy {
        &self.policy
    }

    /// Make a file handling decision for the given item
    ///
    /// This is the SINGLE DECISION POINT for all file handling logic:
    /// ignore match → Exclude (DebugVisible in debug mode), omit match → OmitContent, else Include.
    pub fn decide(&self, context: &ItemContext) -> FileHandlingDecision {
        self.policy.decide_with(context, |rules| {
            should_ignore_item_cached(
                context.relative_path,
                context.name,
                context.is_directory,
                rules,
                self.debug_verbose,
                Some(&self.match_cache),
            )
        })
    }

    /// Explain which rules matched for diagnostic purposes
    ///
    /// Used for CLI `--explain-ignore` flag and debugging.
    pub fn explain(&self, context: &ItemContext) -> String {
        self.policy.explain(context)
    }

    /// Get total number of rules loaded
    pub fn total_rules(&self) -> usize {
        self.policy.total_rules()
    }

    /// Get statistics about loaded rules
    pub fn stats(&self) -> &EngineStats {
        &self.stats
    }

    /// Fast hash function for cache keys (Phase 10.8.4): FNV-1a.
    #[inline]
    fn fast_hash(s: &str) -> u64 {
        const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
        const FNV_PRIME: u64 = 0x100000001b3;

        let mut hash = FNV_OFFSET_BASIS;
        for byte in s.as_bytes() {
            hash ^= *byte as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
        }
        hash
    }

    /// Get cache statistics for diagnostics (Phase 10.8.4)
    pub fn cache_stats(&self) -> CacheStats {
        let cache = self.match_cache.borrow();
        CacheStats {
            entries: cache.len(),
            capacity: cache.capacity(),
            memory_bytes: cache.len() * std::mem::size_of::<((usize, u64), bool)>(),
        }
    }

    /// Clear pattern match cache (Phase 10.8.4)
    #[cfg(test)]
    pub fn clear_cache(&self) {
        self.match_cache.borrow_mut().clear();
    }

    /// Access to cache for benchmarking (Phase 10.8.4)
    #[doc(hidden)]
    pub fn match_cache(&self) -> &RefCell<HashMap<(usize, u64), bool>> {
        &self.match_cache
    }

    /// Access to ignore rules for benchmarking (Phase 10.8.4)
    #[doc(hidden)]
    pub fn ignore_rules(&self) -> &[IgnoreRule] {
        self.policy.ignore_rules()
    }
}

/// Cache statistics (Phase 10.8.4)
#[derive(Debug, Clone)]
pub struct CacheStats {
    /// Number of cached entries
    pub entries: usize,
    /// Current capacity (may be larger than entries)
    pub capacity: usize,
    /// Estimated memory usage in bytes
    pub memory_bytes: usize,
}

// --- LEGACY API (DEPRECATED) ---

/// Legacy function for backward compatibility
///
/// # Deprecated
/// Use `IgnoreEngine::build()` instead for better error handling and statistics.
#[deprecated(since = "0.14.9", note = "Use IgnoreEngine::build() instead")]
pub fn load_ignore_rules(base_dir_path: &Path) -> Vec<IgnoreRule> {
    match IgnoreEngine::build(base_dir_path, false) {
        Ok(engine) => engine.ignore_rules().to_vec(),
        Err(e) => {
            eprintln!("⚠️  Warning: Failed to load ignore rules: {}", e);
            // Return only hardcoded rules as fallback
            crate::selection::rule::parse_rule_lines(HARDCODED_IGNORE_RULES)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- SHOULD IGNORE TESTS ---

    #[test]
    fn test_should_ignore_empty_rules() {
        let rules: Vec<IgnoreRule> = vec![];
        assert!(!should_ignore_item("test.txt", "test.txt", false, &rules, false));
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
        assert!(should_ignore_item("debug.log", "debug.log", false, &rules, false));
        assert!(should_ignore_item(
            "path/to/error.log",
            "error.log",
            false,
            &rules,
            false
        ));
        assert!(!should_ignore_item("log.txt", "log.txt", false, &rules, false));
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
        assert!(should_ignore_item("debug.log", "debug.log", false, &rules, false));
        assert!(!should_ignore_item(
            "important.log",
            "important.log",
            false,
            &rules,
            false
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
        assert!(should_ignore_item("keep.log", "keep.log", false, &rules, false));
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
        assert!(!should_ignore_item("main.rs", "main.rs", false, &rules, false));
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
        assert!(!should_ignore_item("setup.py", "setup.py", false, &rules, false));
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
        assert!(should_ignore_item("vendor/lib/.git", ".git", true, &rules, false));

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
        assert!(should_ignore_item("frontend/build", "build", true, &rules, false));
        assert!(should_ignore_item("backend/dist", "dist", true, &rules, false));
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

    #[test]
    fn test_wildcard_extensions_at_multiple_depths() {
        let rules = vec![
            parse_ignore_pattern_line("*.pyc").unwrap(),
            parse_ignore_pattern_line("*.class").unwrap(),
            parse_ignore_pattern_line("*.o").unwrap(),
        ];

        // Root level
        assert!(should_ignore_item("module.pyc", "module.pyc", false, &rules, false));
        assert!(should_ignore_item("Main.class", "Main.class", false, &rules, false));
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
        assert!(should_ignore_item("build/lib.o", "lib.o", false, &rules, false));

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
        assert!(should_ignore_item(".DS_Store", ".DS_Store", false, &rules, false));
        assert!(should_ignore_item("Thumbs.db", "Thumbs.db", false, &rules, false));
        assert!(should_ignore_item(".directory", ".directory", false, &rules, false));

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
        assert!(should_ignore_item("vendor/lib/.git", ".git", true, &rules, false));
        assert!(should_ignore_item(
            "external/legacy/.svn",
            ".svn",
            true,
            &rules,
            false
        ));
        assert!(should_ignore_item("third-party/old/.hg", ".hg", true, &rules, false));
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
        assert!(should_ignore_item(".vscode", ".vscode", true, &rules, false));
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
        assert!(should_ignore_item("dotnet/api/.vs", ".vs", true, &rules, false));
    }

    #[test]
    fn test_glob_patterns_at_multiple_depths() {
        let rules = vec![parse_ignore_pattern_line("**/*.log").unwrap()];

        // Root level
        assert!(should_ignore_item("error.log", "error.log", false, &rules, false));

        // Depth 1
        assert!(should_ignore_item("logs/error.log", "error.log", false, &rules, false));

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
        let has_turbo = engine.ignore_rules().iter().any(|r| r.pattern == ".turbo");
        eprintln!("DEBUG: .turbo pattern in ignore_rules: {}", has_turbo);
        if has_turbo {
            let turbo_rule = engine.ignore_rules().iter().find(|r| r.pattern == ".turbo").unwrap();
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
            engine.ignore_rules().len(), engine.policy().omit_rules().len(), has_turbo);
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
        assert!(!decision.should_recurse(), ".git/ should not be recursed into");
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

        should_ignore_item_cached("logs/error.log", "error.log", false, &rules, false, Some(&engine.match_cache));
        should_ignore_item_cached("logs/debug.log", "debug.log", false, &rules, false, Some(&engine.match_cache));
        should_ignore_item_cached("logs/access.log", "access.log", false, &rules, false, Some(&engine.match_cache));

        // Cache should have 3 entries (different paths)
        let stats = engine.cache_stats();
        assert_eq!(stats.entries, 3);
    }

    #[test]
    fn test_cache_only_caches_glob_patterns() {
        let temp = tempfile::TempDir::new().unwrap();
        let engine = IgnoreEngine::build(temp.path(), false).unwrap();

        let rules = vec![
            parse_ignore_pattern_line("node_modules").unwrap(),  // Basename - no caching
            parse_ignore_pattern_line("*.log").unwrap(),          // Suffix (optimized) - no caching
            parse_ignore_pattern_line("**/test_*.rs").unwrap(),  // Glob - cached
        ];

        should_ignore_item_cached("node_modules", "node_modules", true, &rules, false, Some(&engine.match_cache));
        should_ignore_item_cached("error.log", "error.log", false, &rules, false, Some(&engine.match_cache));
        should_ignore_item_cached("src/test_main.rs", "test_main.rs", false, &rules, false, Some(&engine.match_cache));

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
            should_ignore_item_cached(&path, &name, false, &rules, false, Some(&engine.match_cache));
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
        assert!(should_ignore_item_cached("file.log", "file.log", false, &rules, false, Some(&engine.match_cache)));
        assert!(!should_ignore_item_cached("file.rs", "file.rs", false, &rules, false, Some(&engine.match_cache)));
        assert!(should_ignore_item_cached("file.txt", "file.txt", false, &rules, false, Some(&engine.match_cache)));
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
        should_ignore_item_cached("logs/error.log", "error.log", false, &rules, false, Some(&engine.match_cache));

        let after_stats = engine.cache_stats();
        assert_eq!(after_stats.entries, 1);
        assert!(after_stats.memory_bytes > 0);
    }
}
