//! Pure layered ignore and omission decisions.
//! Implements ADR-014/015b and SPEC-MAP-MODE section 6.1.
//! Consumes compiled rules supplied by adapters; no model dependency.

use std::fmt::Write as _;
use super::diagnostics::matches_rule;
use super::{IgnoreRule, MatchCache, should_ignore_item_cached};
use crate::domain::{FileHandlingDecision, ItemContext};
use std::{cell::RefCell, collections::HashMap};

/// Compiled rule counts by source layer.
#[derive(Debug, Clone, Default)]
pub struct EngineStats {
    /// Number of hardcoded ignore count rules.
    pub hardcoded_ignore_count: usize,
    /// Number of global ignore count rules.
    pub global_ignore_count: usize,
    /// Number of local ignore count rules.
    pub local_ignore_count: usize,
    /// Number of hardcoded omit count rules.
    pub hardcoded_omit_count: usize,
    /// Number of global omit count rules.
    pub global_omit_count: usize,
    /// Number of local omit count rules.
    pub local_omit_count: usize,
}
/// Ordered model-independent privacy decision engine.
pub struct IgnoreEngine {
    pub(super) ignore_rules: Vec<IgnoreRule>,
    pub(super) omit_rules: Vec<IgnoreRule>,
    pub(super) stats: EngineStats,
    pub(super) debug_verbose: bool,
    pub(super) match_cache: MatchCache,
    pub(super) omit_cache: MatchCache,
}
impl IgnoreEngine {
    /// Constructs a pure engine from ordered ignore and omit rules.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn from_rules(ignore_rules: Vec<IgnoreRule>, omit_rules: Vec<IgnoreRule>) -> Self {
        let stats = EngineStats {
            local_ignore_count: ignore_rules.len(),
            local_omit_count: omit_rules.len(),
            ..EngineStats::default()
        };
        Self {
            ignore_rules,
            omit_rules,
            stats,
            debug_verbose: false,
            match_cache: RefCell::new(HashMap::with_capacity(10_000)),
            omit_cache: RefCell::new(HashMap::with_capacity(10_000)),
        }
    }
    /// Records source counts supplied by the rule loader.
    pub(crate) fn with_stats(mut self, stats: EngineStats) -> Self {
        self.stats = stats;
        self
    }

    /// Preserves an ignored ancestor's privacy when traversing debug structure.
    /// Excluded ancestors remain excluded; their children need no traversal.
    pub fn decide_with_parent(
        &self,
        context: &ItemContext<'_>,
        parent: FileHandlingDecision,
    ) -> FileHandlingDecision {
        match parent {
            FileHandlingDecision::Exclude => FileHandlingDecision::Exclude,
            FileHandlingDecision::DebugVisible => FileHandlingDecision::DebugVisible,
            FileHandlingDecision::Include | FileHandlingDecision::OmitContent => {
                self.decide(context)
            }
        }
    }
    /// Decides permissions before content IO.
    pub fn decide(&self, context: &ItemContext) -> FileHandlingDecision {
        if should_ignore_item_cached(
            context.relative_path,
            context.name,
            context.is_directory,
            &self.ignore_rules,
            self.debug_verbose,
            Some(&self.match_cache),
        ) {
            return if context.debug_mode {
                FileHandlingDecision::DebugVisible
            } else {
                FileHandlingDecision::Exclude
            };
        }

        if should_ignore_item_cached(
            context.relative_path,
            context.name,
            context.is_directory,
            &self.omit_rules,
            self.debug_verbose,
            Some(&self.omit_cache),
        ) {
            return FileHandlingDecision::OmitContent;
        }

        FileHandlingDecision::Include
    }

    /// Explains matching rules and sources.
    pub fn explain(&self, context: &ItemContext) -> String {
        let decision = self.decide(context);
        let mut output = String::new();

        writeln!(output, "Item: {}", context.relative_path).expect("writing to a String cannot fail");
        writeln!(output, "Decision: {decision:?}").expect("writing to a String cannot fail");
        output.push_str("Matching Rules:\n");

        for (i, rule) in self.ignore_rules.iter().enumerate() {
            if matches_rule(context, rule) {
                let source = if i < self.stats.hardcoded_ignore_count {
                    "hardcoded"
                } else if i < self.stats.hardcoded_ignore_count + self.stats.global_ignore_count {
                    "global"
                } else {
                    "local"
                };
                writeln!(output,
                    "  [IGNORE/{}] {} ({:?})",
                    source, rule.pattern, rule.pattern_type
                ).expect("writing to a String cannot fail");
            }
        }

        for (i, rule) in self.omit_rules.iter().enumerate() {
            if matches_rule(context, rule) {
                let source = if i < self.stats.hardcoded_omit_count {
                    "hardcoded"
                } else if i < self.stats.hardcoded_omit_count + self.stats.global_omit_count {
                    "global"
                } else {
                    "local"
                };
                writeln!(output,
                    "  [OMIT/{}] {} ({:?})",
                    source, rule.pattern, rule.pattern_type
                ).expect("writing to a String cannot fail");
            }
        }

        output
    }

    /// Returns compiled rule count.
    pub fn total_rules(&self) -> usize {
        self.ignore_rules.len() + self.omit_rules.len()
    }

    /// Returns source counts.
    pub fn stats(&self) -> &EngineStats {
        &self.stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ignore::parse_ignore_pattern_line;

    #[test]
    fn ignore_and_omit_globs_have_independent_results() {
        let engine = IgnoreEngine::from_rules(
            vec![parse_ignore_pattern_line("**/*.secret").unwrap()],
            vec![parse_ignore_pattern_line("**/*.lock").unwrap()],
        );
        for _ in 0..2 {
            for (name, expected) in [
                ("deps.lock", FileHandlingDecision::OmitContent),
                ("key.secret", FileHandlingDecision::Exclude),
                ("main.rs", FileHandlingDecision::Include),
            ] {
                assert_eq!(
                    engine.decide(&ItemContext {
                        relative_path: name,
                        name,
                        is_directory: false,
                        debug_mode: false
                    }),
                    expected
                );
            }
        }
    }

    #[test]
    fn every_parent_state_preserves_privacy_or_allows_independent_decisions() {
        let engine = IgnoreEngine::from_rules(Vec::new(), Vec::new());
        let child = ItemContext {
            relative_path: "ignored/child.rs",
            name: "child.rs",
            is_directory: false,
            debug_mode: true,
        };
        for parent in [
            FileHandlingDecision::Exclude,
            FileHandlingDecision::DebugVisible,
        ] {
            let decision = engine.decide_with_parent(&child, parent);
            assert_eq!(decision, parent);
            assert!(!decision.should_read_content());
        }
        for parent in [
            FileHandlingDecision::Include,
            FileHandlingDecision::OmitContent,
        ] {
            assert!(
                engine
                    .decide_with_parent(&child, parent)
                    .should_read_content()
            );
        }
    }
}
