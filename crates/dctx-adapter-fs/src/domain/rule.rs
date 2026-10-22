//! One plain ignore or omit rule.
//! Implements ADR-015b last-match lines, including `!`, directory `/`, and wildcards.
//! Depends on [`super::glob`]. Policy and the rule loader share [`parse_rule`].

use super::glob::glob_match;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Pattern {
    Basename(String),
    Suffix(String),
    Prefix(String),
    Path(String),
    Glob(String),
}

/// A compiled rule. Later rules in a [`super::SelectionPolicy`] override earlier ones.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    negation: bool,
    dir_only: bool,
    pattern: Pattern,
}

impl Rule {
    pub(crate) fn matches(&self, relative: &str, name: &str, is_directory: bool) -> bool {
        if self.dir_only && !is_directory {
            return false;
        }
        match &self.pattern {
            Pattern::Basename(pattern) => name == pattern,
            Pattern::Suffix(pattern) => name.ends_with(pattern.as_str()),
            Pattern::Prefix(pattern) => relative.starts_with(pattern.as_str()),
            Pattern::Path(pattern) => relative == pattern,
            Pattern::Glob(pattern) => {
                let text = if pattern.contains('/') { relative } else { name };
                glob_match(pattern.as_bytes(), text.as_bytes())
            }
        }
    }

    pub(crate) fn negation(&self) -> bool {
        self.negation
    }
}

/// Parses one rule line. Blank lines and comments are `None`.
/// `[` is a literal byte, matching the existing plain matcher.
#[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
pub fn parse_rule(line: &str) -> Option<Rule> {
    let mut text = line.trim();
    if text.is_empty() || text.starts_with('#') {
        return None;
    }
    if let Some(hash) = text.find('#') {
        text = text[..hash].trim_end();
        if text.is_empty() {
            return None;
        }
    }
    let negation = if let Some(rest) = text.strip_prefix('!') {
        text = rest;
        true
    } else {
        false
    };
    let dir_only = if let Some(rest) = text.strip_suffix('/') {
        text = rest;
        true
    } else {
        false
    };
    if text.is_empty() {
        return None;
    }
    Some(Rule {
        negation,
        dir_only,
        pattern: classify(text),
    })
}

fn classify(text: &str) -> Pattern {
    let wild = text.contains('*') || text.contains('?') || text.contains('[');
    if wild {
        if let Some(rest) = text.strip_prefix('*')
            && !rest.contains('*')
            && !text.contains('/')
            && !text.contains('?')
        {
            return Pattern::Suffix(rest.to_string());
        }
        if let Some(rest) = text.strip_suffix("/*")
            && !rest.contains('*')
        {
            return Pattern::Prefix(format!("{rest}/"));
        }
        return Pattern::Glob(text.to_string());
    }
    if text.contains('/') {
        Pattern::Path(text.to_string())
    } else {
        Pattern::Basename(text.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_comments_negation_directory_and_suffix() {
        assert!(parse_rule("  # note").is_none());
        let directory = parse_rule(".git/ # metadata").unwrap();
        assert!(directory.dir_only);
        assert!(directory.matches(".git", ".git", true));
        assert!(!directory.matches(".git", ".git", false));
        let suffix = parse_rule("*.omit").unwrap();
        assert!(suffix.matches("note.omit", "note.omit", false));
        assert!(!suffix.matches("note.txt", "note.txt", false));
        let negation = parse_rule("!kept.rs").unwrap();
        assert!(negation.negation());
        assert!(negation.matches("kept.rs", "kept.rs", false));
    }
}
