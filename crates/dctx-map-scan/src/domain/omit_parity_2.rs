//! Omit-rule parity corpus from dctx-core.
//! Implements ADR-015 and SPEC-MAP-MODE section 13a content omission.
//! Uses pure compiled rules; filesystem layering is handled by adapters/rules.

#[cfg(test)]
mod tests {
    use crate::domain::ignore::defaults::HARDCODED_OMIT_CONTENT_RULES;
    use crate::domain::ignore::{IgnoreRule, parse_ignore_pattern_line, should_ignore_item};
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
    #[allow(clippy::too_many_lines, reason = "This table-driven test keeps the complete lockfile-extension matrix beside its shared depth-two checks.")]
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
