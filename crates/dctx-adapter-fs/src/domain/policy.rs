//! Ordered ignore and omit policy.
//! Implements ADR-014: the decision is complete before any content IO.
//! Depends on [`super::rule`] and [`super::selection`]. The filesystem loader supplies text.

use super::rule::{Rule, parse_rule};
use super::selection::{FileHandlingDecision, ItemContext};

/// Last-match ignore rules, then last-match omit rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectionPolicy {
    ignore: Vec<Rule>,
    omit: Vec<Rule>,
}

impl SelectionPolicy {
    pub(crate) fn from_rules(ignore: Vec<Rule>, omit: Vec<Rule>) -> Self {
        Self { ignore, omit }
    }

    /// Compiles explicit lines with no built-in patterns.
    /// Unreadable files are the loader's job; blank and comment lines are skipped.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn from_lines(ignore: &[&str], omit: &[&str]) -> Self {
        Self::from_rules(parse_lines(ignore), parse_lines(omit))
    }

    /// Decides one path with no ancestor constraint.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn decide(&self, context: &ItemContext<'_>) -> FileHandlingDecision {
        if last_match(&self.ignore, context) {
            return if context.debug_mode {
                FileHandlingDecision::DebugVisible
            } else {
                FileHandlingDecision::Exclude
            };
        }
        if last_match(&self.omit, context) {
            return FileHandlingDecision::OmitContent;
        }
        FileHandlingDecision::Include
    }

    /// Keeps an excluded or debug-visible ancestor closed, including negated children.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn decide_with_parent(
        &self,
        context: &ItemContext<'_>,
        parent: FileHandlingDecision,
    ) -> FileHandlingDecision {
        match parent {
            FileHandlingDecision::Exclude => FileHandlingDecision::Exclude,
            FileHandlingDecision::DebugVisible => FileHandlingDecision::DebugVisible,
            FileHandlingDecision::Include | FileHandlingDecision::OmitContent => self.decide(context),
        }
    }
}

fn parse_lines(lines: &[&str]) -> Vec<Rule> {
    lines.iter().filter_map(|line| parse_rule(line)).collect()
}

fn last_match(rules: &[Rule], context: &ItemContext<'_>) -> bool {
    let mut matched = false;
    for rule in rules {
        if rule.matches(context.relative_path, context.name, context.is_directory) {
            matched = !rule.negation();
        }
    }
    matched
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item<'a>(relative: &'a str, name: &'a str, is_directory: bool) -> ItemContext<'a> {
        ItemContext {
            relative_path: relative,
            name,
            is_directory,
            debug_mode: false,
        }
    }

    #[test]
    fn later_rule_wins_and_excluded_parents_stay_closed() {
        let policy = SelectionPolicy::from_lines(&["*.rs", "!kept.rs", "from-dctx.txt"], &["*.omit"]);
        assert_eq!(
            policy.decide(&item("kept.rs", "kept.rs", false)),
            FileHandlingDecision::Include
        );
        assert_eq!(
            policy.decide(&item("other.rs", "other.rs", false)),
            FileHandlingDecision::Exclude
        );
        assert_eq!(
            policy.decide(&item("note.omit", "note.omit", false)),
            FileHandlingDecision::OmitContent
        );
        let parent = policy.decide(&item("private", "private", true));
        let child = policy.decide_with_parent(&item("private/kept.rs", "kept.rs", false), parent);
        assert_eq!(child, FileHandlingDecision::Include);
        let closed = SelectionPolicy::from_lines(&["private/"], &[]);
        let directory = closed.decide(&item("private", "private", true));
        assert_eq!(directory, FileHandlingDecision::Exclude);
        let hidden = closed.decide_with_parent(&item("private/kept.rs", "kept.rs", false), directory);
        assert_eq!(hidden, FileHandlingDecision::Exclude);
    }
}
