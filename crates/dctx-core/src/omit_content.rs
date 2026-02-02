use crate::config::{
    get_global_omit_content_path, DEFAULT_OMIT_CONTENT_FILENAME, HARDCODED_OMIT_CONTENT_RULES,
};
use crate::datatypes::IgnoreRule;
use crate::ignore::parse_ignore_pattern_line;
use std::fs;
use std::path::Path;

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

/// Load content omission rules with hierarchical override:
/// 1. Hardcoded rules (lockfiles, SVG, etc.)
/// 2. Global rules (~/.config/dctx/.dctxomitcontent)
/// 3. Project rules (.dctxomitcontent in project root)
///
/// Last matching rule wins (same as .dctxignore).
pub fn load_omit_content_rules(base_dir_path: &Path) -> Vec<IgnoreRule> {
    let mut rules = Vec::new();

    // Layer 1: Hardcoded rules
    for &p in HARDCODED_OMIT_CONTENT_RULES {
        if let Some(rule) = parse_ignore_pattern_line(p) {
            rules.push(rule);
        }
    }

    // Layer 2: Global rules
    if let Some(global_path) = get_global_omit_content_path() {
        if global_path.exists() {
            load_rules_from_file(&global_path, &mut rules);
        }
    }

    // Layer 3: Project rules
    let project_omit_path = base_dir_path.join(DEFAULT_OMIT_CONTENT_FILENAME);
    if project_omit_path.exists() {
        load_rules_from_file(&project_omit_path, &mut rules);
    }

    rules
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ignore::should_ignore_item;

    #[test]
    fn test_hardcoded_omit_rules_not_empty() {
        assert!(!HARDCODED_OMIT_CONTENT_RULES.is_empty());
        assert!(HARDCODED_OMIT_CONTENT_RULES.contains(&"*.svg"));
        assert!(HARDCODED_OMIT_CONTENT_RULES.contains(&"package-lock.json"));
    }

    #[test]
    fn test_svg_matched_by_hardcoded() {
        let rules: Vec<IgnoreRule> = HARDCODED_OMIT_CONTENT_RULES
            .iter()
            .filter_map(|&p| parse_ignore_pattern_line(p))
            .collect();

        assert!(should_ignore_item("icon.svg", "icon.svg", false, &rules, false));
        assert!(should_ignore_item(
            "assets/logo.svg",
            "logo.svg",
            false,
            &rules,
            false
        ));
    }

    #[test]
    fn test_lockfiles_matched_by_hardcoded() {
        let rules: Vec<IgnoreRule> = HARDCODED_OMIT_CONTENT_RULES
            .iter()
            .filter_map(|&p| parse_ignore_pattern_line(p))
            .collect();

        assert!(should_ignore_item(
            "package-lock.json",
            "package-lock.json",
            false,
            &rules,
            false
        ));
        assert!(should_ignore_item(
            "pnpm-lock.yaml",
            "pnpm-lock.yaml",
            false,
            &rules,
            false
        ));
        assert!(should_ignore_item("yarn.lock", "yarn.lock", false, &rules, false));
        assert!(should_ignore_item(
            "Cargo.lock",
            "Cargo.lock",
            false,
            &rules,
            false
        ));
        assert!(should_ignore_item("go.sum", "go.sum", false, &rules, false));
    }

    #[test]
    fn test_text_files_not_matched() {
        let rules: Vec<IgnoreRule> = HARDCODED_OMIT_CONTENT_RULES
            .iter()
            .filter_map(|&p| parse_ignore_pattern_line(p))
            .collect();

        assert!(!should_ignore_item("README.md", "README.md", false, &rules, false));
        assert!(!should_ignore_item(
            "package.json",
            "package.json",
            false,
            &rules,
            false
        ));
        assert!(!should_ignore_item(
            "Cargo.toml",
            "Cargo.toml",
            false,
            &rules,
            false
        ));
    }

    // --- DEPTH TESTING: Verify patterns work at multiple directory levels ---

    #[test]
    fn test_lockfiles_at_root_level() {
        let rules: Vec<IgnoreRule> = HARDCODED_OMIT_CONTENT_RULES
            .iter()
            .filter_map(|&p| parse_ignore_pattern_line(p))
            .collect();

        // Root level lockfiles should be matched
        assert!(should_ignore_item("Cargo.lock", "Cargo.lock", false, &rules, false));
        assert!(should_ignore_item(
            "package-lock.json",
            "package-lock.json",
            false,
            &rules,
            false
        ));
        assert!(should_ignore_item("yarn.lock", "yarn.lock", false, &rules, false));
        assert!(should_ignore_item("bun.lock", "bun.lock", false, &rules, false));
        assert!(should_ignore_item("go.sum", "go.sum", false, &rules, false));
        assert!(should_ignore_item("Gemfile.lock", "Gemfile.lock", false, &rules, false));
        assert!(should_ignore_item("poetry.lock", "poetry.lock", false, &rules, false));
        assert!(should_ignore_item("composer.lock", "composer.lock", false, &rules, false));
    }

    #[test]
    fn test_lockfiles_at_depth_one() {
        let rules: Vec<IgnoreRule> = HARDCODED_OMIT_CONTENT_RULES
            .iter()
            .filter_map(|&p| parse_ignore_pattern_line(p))
            .collect();

        // Depth 1: <dir>/lockfile
        assert!(should_ignore_item(
            "frontend/Cargo.lock",
            "Cargo.lock",
            false,
            &rules,
            false
        ));
        assert!(should_ignore_item(
            "backend/package-lock.json",
            "package-lock.json",
            false,
            &rules,
            false
        ));
        assert!(should_ignore_item(
            "web/pnpm-lock.yaml",
            "pnpm-lock.yaml",
            false,
            &rules,
            false
        ));
        assert!(should_ignore_item("api/yarn.lock", "yarn.lock", false, &rules, false));
        assert!(should_ignore_item("cli/bun.lock", "bun.lock", false, &rules, false));
        assert!(should_ignore_item("server/go.sum", "go.sum", false, &rules, false));
    }

    #[test]
    fn test_lockfiles_at_depth_three() {
        let rules: Vec<IgnoreRule> = HARDCODED_OMIT_CONTENT_RULES
            .iter()
            .filter_map(|&p| parse_ignore_pattern_line(p))
            .collect();

        // Depth 3: <dir>/<dir>/<dir>/lockfile
        assert!(should_ignore_item(
            "apps/web/frontend/Cargo.lock",
            "Cargo.lock",
            false,
            &rules,
            false
        ));
        assert!(should_ignore_item(
            "packages/ui/components/package-lock.json",
            "package-lock.json",
            false,
            &rules,
            false
        ));
        assert!(should_ignore_item(
            "crates/core/lib/pnpm-lock.yaml",
            "pnpm-lock.yaml",
            false,
            &rules,
            false
        ));
        assert!(should_ignore_item(
            "services/api/v2/yarn.lock",
            "yarn.lock",
            false,
            &rules,
            false
        ));
    }

    #[test]
    fn test_lockfiles_at_depth_five() {
        let rules: Vec<IgnoreRule> = HARDCODED_OMIT_CONTENT_RULES
            .iter()
            .filter_map(|&p| parse_ignore_pattern_line(p))
            .collect();

        // Depth 5+: Very deeply nested lockfiles
        assert!(should_ignore_item(
            "monorepo/apps/web/src/components/package-lock.json",
            "package-lock.json",
            false,
            &rules,
            false
        ));
        assert!(should_ignore_item(
            "projects/backend/services/auth/lib/Cargo.lock",
            "Cargo.lock",
            false,
            &rules,
            false
        ));
        assert!(should_ignore_item(
            "workspace/packages/ui/design-system/tokens/yarn.lock",
            "yarn.lock",
            false,
            &rules,
            false
        ));
    }

    #[test]
    fn test_svg_files_at_multiple_depths() {
        let rules: Vec<IgnoreRule> = HARDCODED_OMIT_CONTENT_RULES
            .iter()
            .filter_map(|&p| parse_ignore_pattern_line(p))
            .collect();

        // Root level
        assert!(should_ignore_item("logo.svg", "logo.svg", false, &rules, false));

        // Depth 1
        assert!(should_ignore_item("assets/icon.svg", "icon.svg", false, &rules, false));

        // Depth 2
        assert!(should_ignore_item(
            "public/images/logo.svg",
            "logo.svg",
            false,
            &rules,
            false
        ));

        // Depth 3+
        assert!(should_ignore_item(
            "src/assets/icons/social/twitter.svg",
            "twitter.svg",
            false,
            &rules,
            false
        ));
    }

    #[test]
    fn test_env_files_at_multiple_depths() {
        let rules: Vec<IgnoreRule> = HARDCODED_OMIT_CONTENT_RULES
            .iter()
            .filter_map(|&p| parse_ignore_pattern_line(p))
            .collect();

        // Note: .env is not in HARDCODED rules, but *.env pattern would match if present
        // This test verifies basename matching works at any depth
        let env_rules: Vec<IgnoreRule> = vec![parse_ignore_pattern_line(".env").unwrap()];

        // Root level
        assert!(should_ignore_item(".env", ".env", false, &env_rules, false));

        // Depth 1
        assert!(should_ignore_item("backend/.env", ".env", false, &env_rules, false));

        // Depth 2
        assert!(should_ignore_item(
            "apps/api/.env",
            ".env",
            false,
            &env_rules,
            false
        ));

        // Depth 3+
        assert!(should_ignore_item(
            "projects/services/auth/.env",
            ".env",
            false,
            &env_rules,
            false
        ));
    }

    #[test]
    fn test_mixed_patterns_at_various_depths() {
        let rules: Vec<IgnoreRule> = HARDCODED_OMIT_CONTENT_RULES
            .iter()
            .filter_map(|&p| parse_ignore_pattern_line(p))
            .collect();

        // Mix of different lockfile types at different depths
        assert!(should_ignore_item(
            "Cargo.lock",
            "Cargo.lock",
            false,
            &rules,
            false
        ));
        assert!(should_ignore_item(
            "frontend/package-lock.json",
            "package-lock.json",
            false,
            &rules,
            false
        ));
        assert!(should_ignore_item(
            "apps/mobile/ios/Podfile.lock",
            "Podfile.lock",
            false,
            &rules,
            false
        ));
        assert!(should_ignore_item(
            "services/backend/api/go.sum",
            "go.sum",
            false,
            &rules,
            false
        ));

        // Verify non-lockfiles are not matched
        assert!(!should_ignore_item(
            "frontend/package.json",
            "package.json",
            false,
            &rules,
            false
        ));
        assert!(!should_ignore_item(
            "apps/api/Cargo.toml",
            "Cargo.toml",
            false,
            &rules,
            false
        ));
    }

    #[test]
    fn test_all_lockfile_types_at_depth_two() {
        let rules: Vec<IgnoreRule> = HARDCODED_OMIT_CONTENT_RULES
            .iter()
            .filter_map(|&p| parse_ignore_pattern_line(p))
            .collect();

        // JavaScript/Node ecosystem
        assert!(should_ignore_item(
            "apps/web/package-lock.json",
            "package-lock.json",
            false,
            &rules,
            false
        ));
        assert!(should_ignore_item(
            "apps/web/pnpm-lock.yaml",
            "pnpm-lock.yaml",
            false,
            &rules,
            false
        ));
        assert!(should_ignore_item(
            "apps/web/yarn.lock",
            "yarn.lock",
            false,
            &rules,
            false
        ));
        assert!(should_ignore_item(
            "apps/web/bun.lockb",
            "bun.lockb",
            false,
            &rules,
            false
        ));
        assert!(should_ignore_item(
            "apps/web/bun.lock",
            "bun.lock",
            false,
            &rules,
            false
        ));

        // Rust
        assert!(should_ignore_item(
            "crates/core/Cargo.lock",
            "Cargo.lock",
            false,
            &rules,
            false
        ));

        // Go
        assert!(should_ignore_item(
            "services/api/go.sum",
            "go.sum",
            false,
            &rules,
            false
        ));

        // Ruby
        assert!(should_ignore_item(
            "backend/rails/Gemfile.lock",
            "Gemfile.lock",
            false,
            &rules,
            false
        ));

        // Python
        assert!(should_ignore_item(
            "services/ml/Pipfile.lock",
            "Pipfile.lock",
            false,
            &rules,
            false
        ));
        assert!(should_ignore_item(
            "services/ml/poetry.lock",
            "poetry.lock",
            false,
            &rules,
            false
        ));
        assert!(should_ignore_item(
            "services/ml/pdm.lock",
            "pdm.lock",
            false,
            &rules,
            false
        ));

        // PHP
        assert!(should_ignore_item(
            "backend/laravel/composer.lock",
            "composer.lock",
            false,
            &rules,
            false
        ));

        // iOS/Swift
        assert!(should_ignore_item(
            "apps/ios/Podfile.lock",
            "Podfile.lock",
            false,
            &rules,
            false
        ));
        assert!(should_ignore_item(
            "apps/ios/Package.resolved",
            "Package.resolved",
            false,
            &rules,
            false
        ));
        assert!(should_ignore_item(
            "apps/ios/Cartfile.resolved",
            "Cartfile.resolved",
            false,
            &rules,
            false
        ));

        // .NET
        assert!(should_ignore_item(
            "backend/dotnet/packages.lock.json",
            "packages.lock.json",
            false,
            &rules,
            false
        ));
        assert!(should_ignore_item(
            "backend/dotnet/paket.lock",
            "paket.lock",
            false,
            &rules,
            false
        ));

        // Elixir
        assert!(should_ignore_item(
            "backend/phoenix/mix.lock",
            "mix.lock",
            false,
            &rules,
            false
        ));

        // Erlang
        assert!(should_ignore_item(
            "backend/erlang/rebar.lock",
            "rebar.lock",
            false,
            &rules,
            false
        ));

        // Haskell
        assert!(should_ignore_item(
            "services/parser/stack.yaml.lock",
            "stack.yaml.lock",
            false,
            &rules,
            false
        ));
        assert!(should_ignore_item(
            "services/parser/cabal.project.freeze",
            "cabal.project.freeze",
            false,
            &rules,
            false
        ));

        // Dart/Flutter
        assert!(should_ignore_item(
            "apps/mobile/pubspec.lock",
            "pubspec.lock",
            false,
            &rules,
            false
        ));

        // R
        assert!(should_ignore_item(
            "analytics/scripts/renv.lock",
            "renv.lock",
            false,
            &rules,
            false
        ));

        // Lua
        assert!(should_ignore_item(
            "scripts/tools/luarocks.lock",
            "luarocks.lock",
            false,
            &rules,
            false
        ));
    }
}
