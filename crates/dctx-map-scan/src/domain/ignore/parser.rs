//! Legacy ignore line parsing.
//! Implements SPEC-MAP-MODE section 6.1 and ADR-014/015b.
//! Uses standard-library types; consumed by scan clients.

use super::{IgnoreRule, PatternType};

/// Parses legacy gitignore-style rules, including inline comments.
pub fn parse_ignore_pattern_line(line: &str) -> Option<IgnoreRule> {
    let mut trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }

    if let Some(comment_pos) = trimmed.find('#') {
        trimmed = trimmed[..comment_pos].trim_end();
        if trimmed.is_empty() {
            return None;
        }
    }

    let mut rule = IgnoreRule::new();

    if let Some(rest) = trimmed.strip_prefix('!') {
        rule.is_negation = true;
        trimmed = rest;
    }

    if let Some(rest) = trimmed.strip_suffix('/') {
        rule.is_dir_only = true;
        trimmed = rest;
    }

    let has_internal_slash = trimmed.contains('/');
    let has_glob_chars = trimmed.contains('*') || trimmed.contains('?') || trimmed.contains('[');

    if has_glob_chars {
        if let Some(rest) = trimmed.strip_prefix('*')
            && !rest.contains('*')
            && !trimmed.contains('/')
            && !trimmed.contains('?')
        {
            rule.pattern_type = PatternType::Suffix;
            rule.pattern = rest.to_string();
        } else if let Some(rest) = trimmed.strip_suffix("/*")
            && !rest.contains('*')
        {
            rule.pattern_type = PatternType::Prefix;
            rule.pattern = format!("{rest}/");
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_comments_do_not_disable_directory_privacy() {
        let rule = parse_ignore_pattern_line("  .turbo/  # generated cache ").unwrap();
        assert_eq!(rule.pattern, ".turbo");
        assert!(rule.is_dir_only);
        assert!(!rule.is_negation);
        assert!(parse_ignore_pattern_line(" # only a comment").is_none());
    }
}
