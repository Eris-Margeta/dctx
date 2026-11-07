// SPDX-License-Identifier: Apache-2.0
//! Selection policy: the layered ignore/omit rule sets and the single decision point.
//! Implements PRODUCT-CONTRACTS C03 (decision precedes any content read) over the
//! three rule layers (hardcoded → global → local), last match wins inside a set.
//! Depends on [`super::rule`], [`super::defaults`] and [`crate::domain`]; pure, no IO.
//! Used by filesystem adapters and by the dctx-pack snapshot engine.

use super::defaults::{HARDCODED_IGNORE_RULES, HARDCODED_OMIT_CONTENT_RULES};
use super::rule::{last_match, parse_rule_lines, parse_rules, rule_matches};
use crate::datatypes::IgnoreRule;
use crate::domain::{FileHandlingDecision, ItemContext};
use std::fmt::Write as _;

/// Where a rule came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleSource {
    /// Compiled into the binary ([`super::defaults`]).
    Hardcoded,
    /// The user's global rule file, supplied by the caller.
    Global,
    /// The project's local rule file, supplied by the caller.
    Local,
}

impl RuleSource {
    /// Lower-case label used in explanations.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn label(self) -> &'static str {
        match self {
            Self::Hardcoded => "hardcoded",
            Self::Global => "global",
            Self::Local => "local",
        }
    }
}

/// Number of rules each layer contributed to one rule set, in layer order.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LayerCounts {
    /// Rules from the built-in layer.
    pub hardcoded: usize,
    /// Rules from the global layer.
    pub global: usize,
    /// Rules from the local layer.
    pub local: usize,
}

impl LayerCounts {
    /// Total rules across all layers.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn total(&self) -> usize {
        self.hardcoded + self.global + self.local
    }

    /// The layer the rule at `index` belongs to.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn source_of(&self, index: usize) -> RuleSource {
        if index < self.hardcoded {
            RuleSource::Hardcoded
        } else if index < self.hardcoded + self.global {
            RuleSource::Global
        } else {
            RuleSource::Local
        }
    }
}

/// Rule file texts supplied by the caller. `None` means the file is absent.
/// The policy never discovers or reads these files itself.
#[derive(Debug, Clone, Copy, Default)]
pub struct RuleTexts<'a> {
    /// Global ignore rules text.
    pub global_ignore: Option<&'a str>,
    /// Global omit-content rules text.
    pub global_omit: Option<&'a str>,
    /// Local (project) ignore rules text.
    pub local_ignore: Option<&'a str>,
    /// Local (project) omit-content rules text.
    pub local_omit: Option<&'a str>,
}

/// Ordered ignore and omit rule sets with their layer bookkeeping.
#[derive(Debug, Clone, Default)]
pub struct SelectionPolicy {
    ignore: Vec<IgnoreRule>,
    omit: Vec<IgnoreRule>,
    ignore_layers: LayerCounts,
    omit_layers: LayerCounts,
}

impl SelectionPolicy {
    /// Builds the three-layer policy: built-in rules, then the supplied global and local texts.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn from_texts(texts: &RuleTexts<'_>) -> Self {
        let (ignore, ignore_layers) = layered(
            HARDCODED_IGNORE_RULES,
            texts.global_ignore,
            texts.local_ignore,
        );
        let (omit, omit_layers) = layered(
            HARDCODED_OMIT_CONTENT_RULES,
            texts.global_omit,
            texts.local_omit,
        );
        Self {
            ignore,
            omit,
            ignore_layers,
            omit_layers,
        }
    }

    /// Builds a policy from explicit rule sets only (no built-in layer); all rules count as local.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn from_rules(ignore: Vec<IgnoreRule>, omit: Vec<IgnoreRule>) -> Self {
        let ignore_layers = LayerCounts {
            local: ignore.len(),
            ..LayerCounts::default()
        };
        let omit_layers = LayerCounts {
            local: omit.len(),
            ..LayerCounts::default()
        };
        Self {
            ignore,
            omit,
            ignore_layers,
            omit_layers,
        }
    }

    /// Exclusion rules, in evaluation order.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn ignore_rules(&self) -> &[IgnoreRule] {
        &self.ignore
    }

    /// Content-omission rules, in evaluation order.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn omit_rules(&self) -> &[IgnoreRule] {
        &self.omit
    }

    /// Layer counts of the exclusion rules.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn ignore_layers(&self) -> LayerCounts {
        self.ignore_layers
    }

    /// Layer counts of the omission rules.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn omit_layers(&self) -> LayerCounts {
        self.omit_layers
    }

    /// Total number of rules in both sets.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn total_rules(&self) -> usize {
        self.ignore.len() + self.omit.len()
    }

    /// Decides how to handle one item: ignore rules first (Exclude, or `DebugVisible` in
    /// debug mode), then omit rules (`OmitContent`), otherwise Include.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn decide(&self, context: &ItemContext<'_>) -> FileHandlingDecision {
        self.decide_with(context, |rules| {
            last_match(
                rules,
                context.relative_path,
                context.name,
                context.is_directory,
            )
        })
    }

    /// Same decision as [`Self::decide`] with a caller-supplied rule-set matcher,
    /// for example one that memoizes glob results. `matches` must keep last-match semantics.
    pub fn decide_with(
        &self,
        context: &ItemContext<'_>,
        mut matches: impl FnMut(&[IgnoreRule]) -> bool,
    ) -> FileHandlingDecision {
        if matches(&self.ignore) {
            return if context.debug_mode {
                FileHandlingDecision::DebugVisible
            } else {
                FileHandlingDecision::Exclude
            };
        }
        if matches(&self.omit) {
            return FileHandlingDecision::OmitContent;
        }
        FileHandlingDecision::Include
    }

    /// Human-readable explanation: the decision and every matching rule with its layer.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn explain(&self, context: &ItemContext<'_>) -> String {
        let mut output = String::new();
        let _ = writeln!(output, "Item: {}", context.relative_path);
        let _ = writeln!(output, "Decision: {:?}", self.decide(context));
        output.push_str("Matching Rules:\n");
        explain_set(
            &mut output,
            "IGNORE",
            &self.ignore,
            self.ignore_layers,
            context,
        );
        explain_set(&mut output, "OMIT", &self.omit, self.omit_layers, context);
        output
    }
}

fn layered(
    builtin: &[&str],
    global: Option<&str>,
    local: Option<&str>,
) -> (Vec<IgnoreRule>, LayerCounts) {
    let mut rules = parse_rule_lines(builtin);
    let hardcoded = rules.len();
    rules.extend(global.map(parse_rules).unwrap_or_default());
    let global_count = rules.len() - hardcoded;
    rules.extend(local.map(parse_rules).unwrap_or_default());
    let local_count = rules.len() - hardcoded - global_count;
    (
        rules,
        LayerCounts {
            hardcoded,
            global: global_count,
            local: local_count,
        },
    )
}

fn explain_set(
    output: &mut String,
    label: &str,
    rules: &[IgnoreRule],
    layers: LayerCounts,
    context: &ItemContext<'_>,
) {
    for (index, rule) in rules.iter().enumerate() {
        if rule_matches(
            rule,
            context.relative_path,
            context.name,
            context.is_directory,
        ) {
            let _ = writeln!(
                output,
                "  [{}/{}] {} ({:?})",
                label,
                layers.source_of(index).label(),
                rule.pattern,
                rule.pattern_type
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item<'a>(path: &'a str, name: &'a str, dir: bool, debug: bool) -> ItemContext<'a> {
        ItemContext {
            relative_path: path,
            name,
            is_directory: dir,
            debug_mode: debug,
        }
    }

    #[test]
    fn builtin_layer_excludes_omits_and_includes() {
        let policy = SelectionPolicy::from_texts(&RuleTexts::default());
        assert_eq!(
            policy.decide(&item("node_modules", "node_modules", true, false)),
            FileHandlingDecision::Exclude
        );
        assert_eq!(
            policy.decide(&item(".git", ".git", true, true)),
            FileHandlingDecision::DebugVisible
        );
        assert_eq!(
            policy.decide(&item("Cargo.lock", "Cargo.lock", false, false)),
            FileHandlingDecision::OmitContent
        );
        assert_eq!(
            policy.decide(&item("src/main.rs", "main.rs", false, false)),
            FileHandlingDecision::Include
        );
    }

    #[test]
    fn layers_are_counted_in_order_and_local_can_negate_builtin() {
        let texts = RuleTexts {
            global_ignore: Some("*.tmp\n# note\n"),
            local_ignore: Some("secret/\n!dist/\n"),
            ..RuleTexts::default()
        };
        let policy = SelectionPolicy::from_texts(&texts);
        let layers = policy.ignore_layers();
        assert_eq!((layers.global, layers.local), (1, 2));
        assert_eq!(layers.total(), policy.ignore_rules().len());
        assert_eq!(layers.source_of(0), RuleSource::Hardcoded);
        assert_eq!(layers.source_of(layers.hardcoded), RuleSource::Global);
        assert_eq!(layers.source_of(layers.total() - 1), RuleSource::Local);
        assert_eq!(
            policy.decide(&item("dist", "dist", true, false)),
            FileHandlingDecision::Include
        );
        assert_eq!(
            policy.decide(&item("secret", "secret", true, false)),
            FileHandlingDecision::Exclude
        );
    }

    #[test]
    fn ignore_wins_over_omit() {
        let policy = SelectionPolicy::from_rules(parse_rules("*.lock\n"), parse_rules("*.lock\n"));
        assert_eq!(
            policy.decide(&item("a.lock", "a.lock", false, false)),
            FileHandlingDecision::Exclude
        );
    }

    #[test]
    fn decide_with_uses_the_supplied_matcher() {
        let policy = SelectionPolicy::from_rules(parse_rules("x\n"), Vec::new());
        let mut calls = 0;
        let decision = policy.decide_with(&item("y", "y", false, false), |_| {
            calls += 1;
            false
        });
        assert_eq!(decision, FileHandlingDecision::Include);
        assert_eq!(calls, 2);
    }

    #[test]
    fn explain_names_decision_and_rule_sources() {
        let texts = RuleTexts {
            local_ignore: Some("node_modules/\n"),
            ..RuleTexts::default()
        };
        let policy = SelectionPolicy::from_texts(&texts);
        let text = policy.explain(&item("node_modules", "node_modules", true, false));
        assert!(text.starts_with("Item: node_modules\nDecision: Exclude\nMatching Rules:\n"));
        assert!(text.contains("  [IGNORE/hardcoded] node_modules (Basename)\n"));
        assert!(text.contains("  [IGNORE/local] node_modules (Basename)\n"));
    }
}
