//! Planned-output coverage: do the required rules still ignore every dctx export class after negations?
//! Implements plugin-api git.md "Verify planned output coverage ... including negations and authorized
//! nested rules. Missing authorized evidence => coverage-unknown; known negation/custom-name conflict =>
//! rules-conflict" (G-COVERAGE). Pure; nested rule bytes come through the broker. Used by `domain::plan`.

use crate::domain::glob::Glob;
use crate::domain::pattern::{Line, parse_line};
use crate::domain::rules::known_pattern;
use crate::domain::text::IgnoreText;

/// A family of dctx-written paths that the four literals protect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputClass {
    /// `<project>.llmcontext.txt` text exports.
    Text,
    /// `<project>.llmcontext-<version>-diff.txt` diff exports.
    Diff,
    /// The `.dctx` state directory.
    State,
}

impl OutputClass {
    /// Every class; all four literals are required whatever the planned roles are.
    pub const ALL: [Self; 3] = [Self::Text, Self::Diff, Self::State];

    /// Class of a planned output role; `None` is a custom output the v1 rules cannot vouch for.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn of_role(role: &str) -> Option<Self> {
        match role {
            "pack-text" => Some(Self::Text),
            "pack-diff" => Some(Self::Diff),
            _ => None,
        }
    }

    fn names(self) -> Glob {
        let glob = match self {
            Self::Text => Glob::parse("*.llmcontext.txt"),
            Self::Diff => Glob::parse("*.llmcontext-*-diff.txt"),
            Self::State => Ok(Glob::literal(".dctx")),
        };
        glob.unwrap_or_else(|_| Glob::literal(""))
    }

    fn covered_by(self, pattern: usize) -> bool {
        match self {
            Self::Text => pattern == 2,
            Self::Diff => pattern == 3,
            Self::State => pattern <= 1,
        }
    }

    const fn is_directory(self) -> bool {
        matches!(self, Self::State)
    }
}

/// Coverage verdict.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Coverage {
    /// Every class is ignored by a required literal that no later negation can undo.
    Verified,
    /// A negation can re-include an export (or a required literal is absent).
    Conflict,
    /// Evidence is missing or a rule form is unsupported.
    Unknown,
}

/// A nested rule file the host declared relevant to a planned output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NestedRules {
    /// The broker refused it (for example excluded or omitted); never read around.
    Unavailable,
    /// Its authorized, validated text.
    Text(IgnoreText),
}

/// Folds verdicts: a known conflict outranks unknown evidence, which outranks verified.
#[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
pub fn worst(left: Coverage, right: Coverage) -> Coverage {
    match (left, right) {
        (Coverage::Conflict, _) | (_, Coverage::Conflict) => Coverage::Conflict,
        (Coverage::Unknown, _) | (_, Coverage::Unknown) => Coverage::Unknown,
        _ => Coverage::Verified,
    }
}

fn negation_verdict(line: &str, classes: impl Iterator<Item = OutputClass>) -> Coverage {
    let Line::Rule(rule) = parse_line(line) else { return Coverage::Verified };
    if !rule.negated {
        return Coverage::Verified;
    }
    let Ok(glob) = rule.last_component() else { return Coverage::Unknown };
    let mut verdict = Coverage::Verified;
    for class in classes {
        if (!rule.dir_only || class.is_directory()) && glob.intersects(&class.names()) {
            verdict = Coverage::Conflict;
        }
    }
    verdict
}

/// Evaluates the root rules (`root`, the text as it would be committed) plus nested evidence.
pub fn evaluate(root: &IgnoreText, nested: &[NestedRules]) -> Coverage {
    let lines: Vec<&str> = root.lines().collect();
    let mut last_cover = [None::<usize>; 3];
    for (index, line) in lines.iter().enumerate() {
        if let Some(pattern) = known_pattern(line) {
            for (slot, class) in OutputClass::ALL.iter().enumerate() {
                if class.covered_by(pattern) {
                    last_cover[slot] = Some(index);
                }
            }
        }
    }
    if last_cover.iter().any(Option::is_none) {
        return Coverage::Conflict;
    }
    let mut verdict = Coverage::Verified;
    for (index, line) in lines.iter().enumerate() {
        let exposed = OutputClass::ALL.iter().enumerate().filter(|(slot, _)| last_cover[*slot] < Some(index));
        verdict = worst(verdict, negation_verdict(line, exposed.map(|(_, class)| *class)));
    }
    for rules in nested {
        verdict = worst(
            verdict,
            match rules {
                NestedRules::Unavailable => Coverage::Unknown,
                NestedRules::Text(text) => {
                    text.lines().fold(Coverage::Verified, |acc, line| worst(acc, negation_verdict(line, OutputClass::ALL.into_iter())))
                }
            },
        );
    }
    verdict
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::text::MAX_IGNORE_BYTES;

    const COMPLETE: &str = ".dctx\n.dctx/\n*.llmcontext.txt\n*.llmcontext-*-diff.txt\n";

    fn text(body: &str) -> IgnoreText {
        IgnoreText::parse(body.as_bytes(), MAX_IGNORE_BYTES).expect("valid")
    }

    #[test]
    fn later_negations_conflict_and_earlier_ones_are_overridden() {
        assert_eq!(evaluate(&text(COMPLETE), &[]), Coverage::Verified);
        assert_eq!(evaluate(&text(&format!("{COMPLETE}!*.llmcontext.txt\n")), &[]), Coverage::Conflict);
        assert_eq!(evaluate(&text(&format!("!*.llmcontext.txt\n{COMPLETE}")), &[]), Coverage::Verified);
        assert_eq!(evaluate(&text(&format!("{COMPLETE}!out/p.llmcontext-1-diff.txt\n")), &[]), Coverage::Conflict);
        assert_eq!(evaluate(&text(&format!("{COMPLETE}!.dctx\n")), &[]), Coverage::Conflict);
        assert_eq!(evaluate(&text(&format!("{COMPLETE}!*.rs\n!keep/\n")), &[]), Coverage::Verified);
        assert_eq!(evaluate(&text(&format!("{COMPLETE}![bad\n")), &[]), Coverage::Unknown);
        assert_eq!(evaluate(&text(".dctx\n"), &[]), Coverage::Conflict);
    }

    #[test]
    fn nested_evidence_must_be_available_and_clean() {
        let root = text(COMPLETE);
        assert_eq!(evaluate(&root, &[NestedRules::Unavailable]), Coverage::Unknown);
        assert_eq!(evaluate(&root, &[NestedRules::Text(text("*.o\n"))]), Coverage::Verified);
        assert_eq!(evaluate(&root, &[NestedRules::Text(text("!*.txt\n"))]), Coverage::Conflict);
        assert_eq!(worst(Coverage::Unknown, Coverage::Conflict), Coverage::Conflict);
        assert_eq!(OutputClass::of_role("pack-text"), Some(OutputClass::Text));
        assert_eq!(OutputClass::of_role("custom-report"), None);
    }
}
