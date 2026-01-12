use crate::config::{get_global_ignore_path, DEFAULT_IGNORE_FILENAME, HARDCODED_IGNORE_RULES};
use crate::datatypes::{IgnoreRule, PatternType};
use std::fs;
use std::path::Path;

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
    } else {
        if has_internal_slash {
            rule.pattern_type = PatternType::Path;
            rule.pattern = trimmed.to_string();
        } else {
            rule.pattern_type = PatternType::Basename;
            rule.pattern = trimmed.to_string();
        }
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

        if matched {
            is_ignored = !rule.is_negation;
        }
    }
    is_ignored
}

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

pub fn load_ignore_rules(base_dir_path: &Path) -> Vec<IgnoreRule> {
    let mut rules = Vec::new();

    for &p in HARDCODED_IGNORE_RULES {
        if let Some(rule) = parse_ignore_pattern_line(p) {
            rules.push(rule);
        }
    }

    if let Some(global_path) = get_global_ignore_path() {
        if global_path.exists() {
            load_rules_from_file(&global_path, &mut rules);
        }
    }

    let project_ignore_path = base_dir_path.join(DEFAULT_IGNORE_FILENAME);
    if project_ignore_path.exists() {
        load_rules_from_file(&project_ignore_path, &mut rules);
    }

    rules
}
