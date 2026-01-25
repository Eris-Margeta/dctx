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

        assert!(should_ignore_item("icon.svg", "icon.svg", false, &rules));
        assert!(should_ignore_item(
            "assets/logo.svg",
            "logo.svg",
            false,
            &rules
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
            &rules
        ));
        assert!(should_ignore_item(
            "pnpm-lock.yaml",
            "pnpm-lock.yaml",
            false,
            &rules
        ));
        assert!(should_ignore_item("yarn.lock", "yarn.lock", false, &rules));
        assert!(should_ignore_item(
            "Cargo.lock",
            "Cargo.lock",
            false,
            &rules
        ));
        assert!(should_ignore_item("go.sum", "go.sum", false, &rules));
    }

    #[test]
    fn test_text_files_not_matched() {
        let rules: Vec<IgnoreRule> = HARDCODED_OMIT_CONTENT_RULES
            .iter()
            .filter_map(|&p| parse_ignore_pattern_line(p))
            .collect();

        assert!(!should_ignore_item("README.md", "README.md", false, &rules));
        assert!(!should_ignore_item(
            "package.json",
            "package.json",
            false,
            &rules
        ));
        assert!(!should_ignore_item(
            "Cargo.toml",
            "Cargo.toml",
            false,
            &rules
        ));
    }
}
