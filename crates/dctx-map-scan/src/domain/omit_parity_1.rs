//! Omit-rule parity corpus from dctx-core.
//! Implements ADR-015 and SPEC-MAP-MODE section 13a content omission.
//! Uses pure compiled rules; filesystem layering is handled by adapters/rules.

#[cfg(test)]
mod tests {
    use crate::domain::ignore::defaults::HARDCODED_OMIT_CONTENT_RULES;
    use crate::domain::ignore::{IgnoreRule, parse_ignore_pattern_line, should_ignore_item};
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
        assert!(should_ignore_item(
            "api/yarn.lock",
            "yarn.lock",
            false,
            &rules,
            false
        ));
        assert!(should_ignore_item(
            "cli/bun.lock",
            "bun.lock",
            false,
            &rules,
            false
        ));
        assert!(should_ignore_item(
            "server/go.sum",
            "go.sum",
            false,
            &rules,
            false
        ));
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
        assert!(should_ignore_item(
            "logo.svg", "logo.svg", false, &rules, false
        ));

        // Depth 1
        assert!(should_ignore_item(
            "assets/icon.svg",
            "icon.svg",
            false,
            &rules,
            false
        ));

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
        let _rules: Vec<IgnoreRule> = HARDCODED_OMIT_CONTENT_RULES
            .iter()
            .filter_map(|&p| parse_ignore_pattern_line(p))
            .collect();

        // Note: .env is not in HARDCODED rules, but *.env pattern would match if present
        // This test verifies basename matching works at any depth
        let env_rules: Vec<IgnoreRule> = vec![parse_ignore_pattern_line(".env").unwrap()];

        // Root level
        assert!(should_ignore_item(".env", ".env", false, &env_rules, false));

        // Depth 1
        assert!(should_ignore_item(
            "backend/.env",
            ".env",
            false,
            &env_rules,
            false
        ));

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
}
