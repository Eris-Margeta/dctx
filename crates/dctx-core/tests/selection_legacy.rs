//! Legacy ignore behavior through the pure selection API (D3-LEGACY-REMOVAL §3/§5 D3-07).
//! Depends only on dctx-core's pure policy, rules and domain decisions.
//! Integration evidence retained when the legacy `IgnoreEngine` is removed.
//!
//! Legacy test disposition (names below omit `test_`; all others retain their names):
//! | Legacy test | Disposition |
//! | `error_display` | retired: `IgnoreError` is an IO error; adapter coverage is in `legacy_rules.rs` |
//! | `cache_basic_functionality` | retired: `IgnoreEngine` cache entries have no pure counterpart |
//! | `cache_different_paths_different_entries` | retired: no pure cache entries |
//! | `cache_only_caches_glob_patterns` | retired: no pure cache or cache classification |
//! | `cache_memory_usage_reasonable` | retired: no pure cache allocation/statistics |
//! | `cache_stats_accuracy` | retired: no pure cache capacity or statistics |
//! | `turbo_pattern_loaded` | retained: pure built-ins include `.turbo/` (ADR-015) |
//! | `pnpm_store_pattern_loaded` | retained: pure built-ins include `.pnpm-store/` (ADR-015) |
//! Cache negation/collision tests below retain their decisions; cache hit/key assertions
//! are retired because `SelectionPolicy` has no cache. File loading is in adapter tests;
//! engine layer-count tests here use explicit texts rather than ambient home discovery.

#![forbid(unsafe_code)]

use dctx_core::selection::{policy::RuleTexts, rule::parse_rules};
use dctx_core::{FileHandlingDecision as Decision, ItemContext, SelectionPolicy};

fn policy(text: &str) -> SelectionPolicy {
    SelectionPolicy::from_rules(parse_rules(text), Vec::new())
}

fn item(path: &str, dir: bool, debug: bool) -> ItemContext<'_> {
    ItemContext { relative_path: path, name: path.rsplit('/').next().unwrap(), is_directory: dir, debug_mode: debug }
}

fn pair(p: &SelectionPolicy, denied: &str, kept: &str, dir: bool) {
    assert_eq!(p.decide(&item(denied, dir, false)), Decision::Exclude, "{denied}");
    assert_eq!(p.decide(&item(kept, dir, false)), Decision::Include, "{kept}");
}

macro_rules! cases {
    ($name:ident, $rules:literal, $dir:literal, $kept:literal, [$($path:literal),+ $(,)?]) => {
        #[test]
        fn $name() {
            let p = policy($rules);
            for path in [$($path),+] { pair(&p, path, $kept, $dir); }
        }
    };
}

#[test]
fn test_should_ignore_empty_rules() {
    assert_eq!(policy("").decide(&item("test.txt", false, false)), Decision::Include);
}

cases!(test_should_ignore_basename_match, "node_modules", true, "my_modules", ["node_modules", "path/to/node_modules"]);
cases!(test_should_ignore_suffix_match, "*.log", false, "log.txt", ["debug.log", "path/to/error.log"]);
cases!(test_should_ignore_path_match, "src/generated", true, "other/generated", ["src/generated"]);
cases!(test_should_ignore_prefix_match, "build/*", false, "mybuild/file.txt", ["build/output.o", "build/sub/file.txt"]);

#[test]
fn test_should_ignore_dir_only_rule() {
    let p = policy("cache/");
    pair(&p, "cache", "cache-copy", true);
    assert_eq!(p.decide(&item("cache", false, false)), Decision::Include);
}

cases!(test_should_ignore_negation, "*.log\n!important.log", false, "important.log", ["debug.log"]);
cases!(test_should_ignore_negation_order_matters, "!keep.log\n*.log", false, "keep.txt", ["keep.log"]);
cases!(test_should_ignore_glob_with_path, "**/test_*.rs", false, "main.rs", ["test_main.rs", "src/test_util.rs"]);

#[test]
fn test_should_ignore_multiple_rules() {
    let p = policy("*.pyc\n__pycache__/\n*.egg-info/");
    pair(&p, "module.pyc", "setup.py", false);
    pair(&p, "__pycache__", "src", true);
    pair(&p, "mypackage.egg-info", "mypackage", true);
    assert_eq!(p.decide(&item("mypackage.egg-info", false, false)), Decision::Include);
}

cases!(
    test_node_modules_at_multiple_depths,
    "node_modules/",
    true,
    "modules",
    ["node_modules", "frontend/node_modules", "apps/web/node_modules", "monorepo/packages/ui/node_modules"]
);
cases!(
    test_target_directory_at_multiple_depths,
    "target/",
    true,
    "targeted",
    ["target", "crates/target", "rust/backend/target", "workspace/crates/core/target"]
);
cases!(test_git_directory_at_multiple_depths, ".git/", true, "vendor/lib/git", [".git", "vendor/lib/.git", "external/projects/foo/.git"]);
cases!(
    test_pycache_at_multiple_depths,
    "__pycache__/",
    true,
    "src/pycache",
    ["__pycache__", "src/__pycache__", "backend/api/__pycache__", "services/ml/models/training/__pycache__"]
);
cases!(
    test_build_directories_at_multiple_depths,
    "build/\ndist/\nout/",
    true,
    "src",
    ["build", "dist", "out", "frontend/build", "backend/dist", "cli/out", "apps/web/frontend/build", "packages/ui/lib/dist"]
);
cases!(
    test_wildcard_extensions_at_multiple_depths,
    "*.pyc\n*.class\n*.o",
    false,
    "src/main.py",
    [
        "module.pyc",
        "Main.class",
        "lib.o",
        "src/module.pyc",
        "bin/Main.class",
        "build/lib.o",
        "backend/services/api/module.pyc",
        "java/src/com/example/Main.class"
    ]
);
cases!(
    test_dotfiles_at_multiple_depths,
    ".DS_Store\nThumbs.db\n.directory",
    false,
    "assets/image.png",
    [".DS_Store", "Thumbs.db", ".directory", "assets/.DS_Store", "images/Thumbs.db", "public/assets/images/.DS_Store"]
);
cases!(
    test_vcs_directories_at_multiple_depths,
    ".git/\n.svn/\n.hg/",
    true,
    "vendor/lib/src",
    [".git", ".svn", ".hg", "vendor/lib/.git", "external/legacy/.svn", "third-party/old/.hg"]
);
cases!(
    test_ide_directories_at_multiple_depths,
    ".idea/\n.vscode/\n.vs/",
    true,
    "dotnet/api/src",
    [".idea", ".vscode", ".vs", "frontend/.idea", "backend/.vscode", "dotnet/api/.vs"]
);
cases!(
    test_glob_patterns_at_multiple_depths,
    "**/*.log",
    false,
    "apps/backend/services/api/logs/error.txt",
    ["error.log", "logs/error.log", "var/logs/error.log", "apps/backend/services/api/logs/error.log"]
);

#[test]
fn test_complex_real_world_scenario() {
    let p = policy("node_modules/\ntarget/\n.git/\n__pycache__/\n*.pyc\nbuild/\ndist/\n.DS_Store");
    for path in ["apps/web/node_modules", "crates/core/target", "backend/api/__pycache__", "frontend/dist"] {
        pair(&p, path, "apps/web/src", true);
    }
    for path in ["services/ml/models/trainer.pyc", "public/assets/.DS_Store"] {
        pair(&p, path, "backend/api/main.py", false);
    }
    for path in ["apps/web/package.json", "crates/core/Cargo.toml", "backend/api/main.py"] {
        assert_eq!(p.decide(&item(path, false, false)), Decision::Include);
    }
}

fn builtin() -> SelectionPolicy {
    SelectionPolicy::from_texts(&RuleTexts::default())
}

#[test]
fn test_engine_build_success() {
    let p = builtin();
    assert!(p.total_rules() > 0);
    assert_eq!(p.total_rules(), p.ignore_rules().len() + p.omit_rules().len());
    pair(&p, "node_modules", "src", true);
}

#[test]
fn test_engine_loads_hardcoded_rules() {
    let p = builtin();
    assert!(p.ignore_layers().hardcoded > 0);
    assert!(p.omit_layers().hardcoded > 0);
    assert_eq!(p.ignore_layers().total(), p.ignore_rules().len());
    assert_eq!(p.omit_layers().total(), p.omit_rules().len());
    pair(&p, ".git", "src", true);
}

#[test]
fn test_engine_loads_global_rules() {
    let p = SelectionPolicy::from_texts(&RuleTexts { global_ignore: Some("# global\n\nglobal-only/\n"), ..RuleTexts::default() });
    assert_eq!(p.ignore_layers().global, 1);
    assert_eq!(p.ignore_layers().local, 0);
    assert_eq!(p.total_rules(), builtin().total_rules() + 1);
    pair(&p, "global-only", "local-only", true);
}

#[test]
fn test_engine_loads_local_rules() {
    let p = SelectionPolicy::from_texts(&RuleTexts { local_ignore: Some("custom_pattern/\ntest_*.txt\n"), ..RuleTexts::default() });
    assert_eq!(p.ignore_layers().local, 2);
    assert_eq!(p.total_rules(), builtin().total_rules() + 2);
    pair(&p, "custom_pattern", "custom", true);
    pair(&p, "test_one.txt", "one.txt", false);
}

#[test]
fn test_decide_exclude_for_ignored() {
    let p = builtin();
    let context = ItemContext { name: "node_modules", ..item("node_modules/package", true, false) };
    assert_eq!(p.decide(&context), Decision::Exclude);
    pair(&p, "node_modules", "src", true);
}

#[test]
fn test_decide_debug_visible_in_debug_mode() {
    let p = builtin();
    let context = ItemContext { name: ".git", ..item(".git/hooks", true, true) };
    assert_eq!(p.decide(&context), Decision::DebugVisible);
    assert_eq!(p.decide(&item("src", true, true)), Decision::Include);
}

#[test]
fn test_decide_omit_content_for_lockfiles() {
    let p = builtin();
    assert_eq!(p.decide(&item("package-lock.json", false, false)), Decision::OmitContent);
    assert_eq!(p.decide(&item("package.json", false, false)), Decision::Include);
}

#[test]
fn test_decide_include_for_normal_files() {
    assert_eq!(builtin().decide(&item("src/main.rs", false, false)), Decision::Include);
}

#[test]
fn test_explain_produces_output() {
    let p =
        SelectionPolicy::from_texts(&RuleTexts { global_ignore: Some("*.log"), local_ignore: Some("!keep.log"), ..RuleTexts::default() });
    let context = ItemContext { name: "node_modules", ..item("node_modules/package", true, false) };
    let explanation = p.explain(&context);
    assert!(explanation.contains("node_modules"));
    assert!(explanation.contains("Decision: Exclude"));
    assert!(explanation.contains("[IGNORE/hardcoded]"));
    let explanation = p.explain(&item("keep.log", false, false));
    assert!(explanation.contains("Decision: Include"));
    assert!(explanation.contains("[IGNORE/global]"));
    assert!(explanation.contains("[IGNORE/local]"));
    pair(&p, "error.log", "keep.log", false);
}

#[test]
fn test_turbo_pattern_loaded() {
    let p = builtin();
    assert!(p.ignore_rules().iter().any(|r| r.pattern == ".turbo"));
    let context = ItemContext { name: ".turbo", ..item(".turbo/cache", true, false) };
    assert_eq!(p.decide(&context), Decision::Exclude);
    pair(&p, ".turbo", "turbo", true);
}

#[test]
fn test_pnpm_store_pattern_loaded() {
    let p = builtin();
    let context = ItemContext { name: ".pnpm-store", ..item(".pnpm-store/v3", true, false) };
    assert_eq!(p.decide(&context), Decision::Exclude);
    pair(&p, ".pnpm-store", "pnpm-store", true);
}

#[test]
fn test_ignored_directories_never_recursed() {
    let p = builtin();
    let context = ItemContext { name: ".git", ..item(".git/objects", true, false) };
    let decision = p.decide(&context);
    assert_eq!(decision, Decision::Exclude);
    assert!(!decision.should_recurse());
    let decision = p.decide(&item("src", true, false));
    assert_eq!(decision, Decision::Include);
    assert!(decision.should_recurse());
}

#[test]
fn test_cache_respects_negation_semantics() {
    let p = policy("*.log\n!important.log");
    for _ in 0..2 {
        pair(&p, "debug.log", "important.log", false);
    }
}

#[test]
fn test_cache_hash_collisions_dont_break_correctness() {
    let p = policy("*.log\n*.txt");
    for path in ["file.log", "file.txt"] {
        pair(&p, path, "file.rs", false);
    }
}
