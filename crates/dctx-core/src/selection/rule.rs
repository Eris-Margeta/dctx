//! Ignore/omit rule parsing and single-rule matching (gitignore-style plain text, last match wins).
//! Implements the selection rule syntax preserved by ARCHITECTURE-PROPOSAL §2 ("last-match semantics").
//! Depends on [`crate::datatypes::IgnoreRule`] and [`super::glob`]; pure, no IO.
//! Used by [`super::policy`], by `crate::ignore` and `crate::omit_content`, and by dctx-pack.

use super::glob::glob_match;
use crate::datatypes::{IgnoreRule, PatternType};

/// Parses one rule line. Blank lines and `#` comments yield `None`.
///
/// A leading `!` negates, a trailing `/` restricts to directories and inline `#`
/// comments are stripped. Simple shapes are classified as `Basename`, `Path`,
/// `Suffix` (`*.ext`) or `Prefix` (`dir/*`); everything else with wildcards is `Glob`.
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

/// Parses every rule line of a rule file's text, in order.
pub fn parse_rules(text: &str) -> Vec<IgnoreRule> {
    text.lines().filter_map(parse_ignore_pattern_line).collect()
}

/// Parses a static list of rule lines, in order.
pub fn parse_rule_lines(lines: &[&str]) -> Vec<IgnoreRule> {
    lines.iter().filter_map(|line| parse_ignore_pattern_line(line)).collect()
}

/// The text a glob rule is matched against: the relative path when the pattern
/// contains `/`, otherwise the item name.
pub fn glob_subject<'a>(rule: &IgnoreRule, relative_path: &'a str, name: &'a str) -> &'a str {
    if rule.pattern.contains('/') {
        relative_path
    } else {
        name
    }
}

/// Drops one leading `/` so a gitignore pattern anchored at the repository root
/// matches the walker's root-relative path (`/corpus/` against `corpus`).
fn anchored(pattern: &str) -> &str {
    pattern.strip_prefix('/').unwrap_or(pattern)
}

/// Returns true if `rule` matches the item, ignoring negation.
pub fn rule_matches(rule: &IgnoreRule, relative_path: &str, name: &str, is_dir: bool) -> bool {
    if rule.is_dir_only && !is_dir {
        return false;
    }
    match rule.pattern_type {
        PatternType::Invalid => false,
        PatternType::Path => relative_path == anchored(&rule.pattern),
        PatternType::Prefix => {
            let pattern = anchored(&rule.pattern);
            if pattern.is_empty() {
                !relative_path.is_empty() && !relative_path.contains('/')
            } else {
                relative_path.starts_with(pattern)
            }
        }
        PatternType::Basename => name == rule.pattern,
        PatternType::Suffix => name.ends_with(&rule.pattern),
        PatternType::Glob => {
            let (pattern, subject) = match rule.pattern.strip_prefix('/') {
                Some(pattern) => (pattern, relative_path),
                None => (rule.pattern.as_str(), glob_subject(rule, relative_path, name)),
            };
            glob_match(pattern.as_bytes(), subject.as_bytes())
        }
    }
}

/// Applies `rules` in order with last-match-wins semantics: a matching rule sets the
/// item selected, a matching negation clears it. Returns the final state.
pub fn last_match(rules: &[IgnoreRule], relative_path: &str, name: &str, is_dir: bool) -> bool {
    let mut selected = false;
    for rule in rules {
        if rule_matches(rule, relative_path, name, is_dir) {
            selected = !rule.is_negation;
        }
    }
    selected
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn inline_comment_is_stripped_and_comment_only_tail_is_dropped() {
        assert_eq!(parse_ignore_pattern_line("*.tmp # scratch").unwrap().pattern, ".tmp");
        assert!(parse_ignore_pattern_line(" #only").is_none());
    }

    #[test]
    fn parse_rules_keeps_order_and_skips_comments() {
        let rules = parse_rules("# header\nfirst\n\n!second\n");
        let patterns: Vec<&str> = rules.iter().map(|r| r.pattern.as_str()).collect();
        assert_eq!(patterns, ["first", "second"]);
        assert!(rules[1].is_negation);
    }

    #[test]
    fn last_match_wins_and_negation_reincludes() {
        let rules = parse_rules("*.log\n!keep.log\n");
        assert!(last_match(&rules, "a.log", "a.log", false));
        assert!(!last_match(&rules, "keep.log", "keep.log", false));
        let reordered = parse_rules("!keep.log\n*.log\n");
        assert!(last_match(&reordered, "keep.log", "keep.log", false));
    }

    #[test]
    fn dir_only_rule_never_matches_files() {
        let rules = parse_rules("build/\n");
        assert!(last_match(&rules, "build", "build", true));
        assert!(!last_match(&rules, "build", "build", false));
    }

    #[test]
    fn glob_with_slash_matches_the_path_and_without_matches_the_name() {
        let path_rule = parse_ignore_pattern_line("src/**/gen_*.rs").unwrap();
        assert!(rule_matches(&path_rule, "src/a/gen_x.rs", "gen_x.rs", false));
        assert!(!rule_matches(&path_rule, "lib/gen_x.rs", "gen_x.rs", false));
        let name_rule = parse_ignore_pattern_line("gen_?.rs").unwrap();
        assert!(rule_matches(&name_rule, "deep/dir/gen_x.rs", "gen_x.rs", false));
    }

    #[test]
    fn a_leading_slash_anchors_the_pattern_at_the_repository_root() {
        let corpus = parse_ignore_pattern_line("/corpus/").expect("rule");
        assert!(rule_matches(&corpus, "corpus", "corpus", true));
        assert!(!rule_matches(&corpus, "src/corpus", "corpus", true));
        assert!(!rule_matches(&corpus, "corpus/ready.json", "ready.json", false));
        let rooted = parse_rules("/*.log\n");
        assert!(last_match(&rooted, "a.log", "a.log", false));
        assert!(!last_match(&rooted, "sub/a.log", "a.log", false));
        let anywhere = parse_rules("*.log\n");
        assert!(last_match(&anywhere, "a.log", "a.log", false));
        assert!(last_match(&anywhere, "sub/a.log", "a.log", false));
        let negated = parse_rules("/corpus/*\n!/corpus/keep.json\n");
        assert!(last_match(&negated, "corpus/drop.json", "drop.json", false));
        assert!(!last_match(&negated, "corpus/keep.json", "keep.json", false));
        assert!(!last_match(&negated, "other/drop.json", "drop.json", false));
        let root_children = parse_rules("/*\n");
        assert!(last_match(&root_children, "README", "README", false));
        assert!(!last_match(&root_children, "src/main.rs", "main.rs", false));
    }
}
