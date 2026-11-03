// SPDX-License-Identifier: Apache-2.0
//! Omit-rule parity corpus from dctx-core.
//! Implements ADR-015 and SPEC-MAP-MODE section 13a content omission.
//! Uses pure compiled rules; filesystem layering is handled by adapters/rules.

#[cfg(test)]
mod tests {
    use crate::domain::ignore::defaults::HARDCODED_OMIT_CONTENT_RULES;
    use crate::domain::ignore::{IgnoreRule, parse_ignore_pattern_line, should_ignore_item};
    #[test]
    fn test_hardcoded_omit_rules_not_empty() {
        assert_ne!(HARDCODED_OMIT_CONTENT_RULES, &[] as &[&str]);
        assert!(HARDCODED_OMIT_CONTENT_RULES.contains(&"*.svg"));
        assert!(HARDCODED_OMIT_CONTENT_RULES.contains(&"package-lock.json"));
    }

    #[test]
    fn test_svg_matched_by_hardcoded() {
        let rules: Vec<IgnoreRule> = HARDCODED_OMIT_CONTENT_RULES
            .iter()
            .filter_map(|&p| parse_ignore_pattern_line(p))
            .collect();

        assert!(should_ignore_item(
            "icon.svg", "icon.svg", false, &rules, false
        ));
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
        assert!(should_ignore_item(
            "yarn.lock",
            "yarn.lock",
            false,
            &rules,
            false
        ));
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

        assert!(!should_ignore_item(
            "README.md",
            "README.md",
            false,
            &rules,
            false
        ));
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
        assert!(should_ignore_item(
            "Cargo.lock",
            "Cargo.lock",
            false,
            &rules,
            false
        ));
        assert!(should_ignore_item(
            "package-lock.json",
            "package-lock.json",
            false,
            &rules,
            false
        ));
        assert!(should_ignore_item(
            "yarn.lock",
            "yarn.lock",
            false,
            &rules,
            false
        ));
        assert!(should_ignore_item(
            "bun.lock", "bun.lock", false, &rules, false
        ));
        assert!(should_ignore_item("go.sum", "go.sum", false, &rules, false));
        assert!(should_ignore_item(
            "Gemfile.lock",
            "Gemfile.lock",
            false,
            &rules,
            false
        ));
        assert!(should_ignore_item(
            "poetry.lock",
            "poetry.lock",
            false,
            &rules,
            false
        ));
        assert!(should_ignore_item(
            "composer.lock",
            "composer.lock",
            false,
            &rules,
            false
        ));
    }
}
