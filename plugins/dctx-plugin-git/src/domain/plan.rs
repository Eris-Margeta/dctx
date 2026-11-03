// SPDX-License-Identifier: Apache-2.0
//! The protection plan: already protected, a bounded append, or a refusal to claim protection.
//! Implements PLUGINS.md §6.2 steps 4–5 and git.md "If rules, current marker and coverage are complete,
//! return ok/already-protected with zero writes" / "append missing literals in order; separator if
//! needed; header only when adding rules; current marker only when absent" (G-IDEMPOTENT, G-REPLACE).
//! Pure; depends on rules, text and coverage. Used by `app::protect`.

use crate::domain::coverage::{Coverage, NestedRules, evaluate};
use crate::domain::rules::{HEADER, REQUIRED_PATTERNS, current_marker, is_current_marker, known_pattern};
use crate::domain::text::{IgnoreText, MAX_IGNORE_BYTES};

/// What the committed file would gain.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Append {
    /// The complete replacement: the original bytes followed by the appended suffix.
    pub replacement: Vec<u8>,
    /// Required literals appended, in order.
    pub appended_rules: Vec<&'static str>,
    /// Whether the current marker was appended.
    pub marker_added: bool,
    /// Appended byte count (replacement minus original).
    pub growth: u64,
}

/// The plan for one `.gitignore`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Plan {
    /// Current marker, complete rules and verified coverage: nothing to write or stage.
    AlreadyProtected,
    /// Append the suffix, then commit with compare-and-replace.
    Append(Append),
    /// A known negation or custom-name conflict: no write, no protection claim.
    RulesConflict,
    /// Evidence is missing or unsupported: no write, no protection claim.
    CoverageUnknown,
}

/// Whether the four literals and the current host marker are all present (the old fast path, plus rules).
#[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
pub fn missing(text: &IgnoreText, host_version: &str) -> (Vec<&'static str>, bool) {
    let mut present = [false; REQUIRED_PATTERNS.len()];
    let mut marker = false;
    for line in text.lines() {
        if let Some(index) = known_pattern(line) {
            present[index] = true;
        }
        marker |= is_current_marker(line, host_version);
    }
    let rules = REQUIRED_PATTERNS.iter().zip(present).filter(|(_, seen)| !seen).map(|(rule, _)| *rule).collect();
    (rules, !marker)
}

/// Computes the plan; coverage is evaluated on the text as it would be committed.
#[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
pub fn plan(text: &IgnoreText, host_version: &str, nested: &[NestedRules], custom_output: bool) -> Plan {
    let (appended_rules, marker_added) = missing(text, host_version);
    let newline = text.newline().as_str();
    let mut suffix = String::new();
    if (!appended_rules.is_empty() || marker_added) && text.needs_separator() {
        suffix.push_str(newline);
    }
    if !appended_rules.is_empty() {
        for line in std::iter::once(HEADER).chain(appended_rules.iter().copied()) {
            suffix.push_str(line);
            suffix.push_str(newline);
        }
    }
    if marker_added {
        suffix.push_str(&current_marker(host_version));
        suffix.push_str(newline);
    }
    let mut replacement = text.as_str().as_bytes().to_vec();
    replacement.extend_from_slice(suffix.as_bytes());
    let committed = IgnoreText::parse(&replacement, MAX_IGNORE_BYTES + u64::try_from(suffix.len()).unwrap_or(u64::MAX));
    let coverage = match committed {
        Ok(committed) => evaluate(&committed, nested),
        Err(_) => Coverage::Unknown,
    };
    match (coverage, custom_output) {
        (Coverage::Conflict, _) => Plan::RulesConflict,
        (Coverage::Unknown, _) | (_, true) => Plan::CoverageUnknown,
        (Coverage::Verified, false) if suffix.is_empty() => Plan::AlreadyProtected,
        (Coverage::Verified, false) => Plan::Append(Append {
            growth: u64::try_from(suffix.len()).unwrap_or(u64::MAX),
            replacement,
            appended_rules,
            marker_added,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(body: &str) -> IgnoreText {
        IgnoreText::parse(body.as_bytes(), MAX_IGNORE_BYTES).expect("valid")
    }

    fn appended(body: &str) -> String {
        match plan(&text(body), "0.26.0", &[], false) {
            Plan::Append(append) => String::from_utf8(append.replacement).expect("utf8"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn appends_only_what_is_missing() {
        let full = "# [dctx] Automated Rules\n.dctx\n.dctx/\n*.llmcontext.txt\n*.llmcontext-*-diff.txt\n# [dctx v0.26.0]\n";
        assert_eq!(appended(""), full);
        assert_eq!(plan(&text(full), "0.26.0", &[], false), Plan::AlreadyProtected);
        assert_eq!(appended("notes"), format!("notes\n{full}"));
        assert_eq!(appended("notes\r\n"), format!("notes\r\n{}", full.replace('\n', "\r\n")));
        let rules = ".dctx\n.dctx/\n*.llmcontext.txt\n*.llmcontext-*-diff.txt\n";
        assert_eq!(appended(rules), format!("{rules}# [dctx v0.26.0]\n"));
        let removed = ".dctx\n.dctx/\n*.llmcontext.txt\n# [dctx v0.26.0]\n";
        assert_eq!(appended(removed), format!("{removed}# [dctx] Automated Rules\n*.llmcontext-*-diff.txt\n"));
    }

    #[test]
    fn conflicts_unknowns_and_custom_outputs_never_claim_protection() {
        let full = "# [dctx] Automated Rules\n.dctx\n.dctx/\n*.llmcontext.txt\n*.llmcontext-*-diff.txt\n# [dctx v0.26.0]\n";
        assert_eq!(plan(&text(&format!("{full}!*.llmcontext.txt\n")), "0.26.0", &[], false), Plan::RulesConflict);
        assert_eq!(plan(&text("notes\n"), "0.26.0", &[NestedRules::Unavailable], false), Plan::CoverageUnknown);
        assert_eq!(plan(&text(full), "0.26.0", &[], true), Plan::CoverageUnknown);
        match plan(&text(full), "0.27.0", &[], false) {
            Plan::Append(append) => assert!(append.appended_rules.is_empty() && append.marker_added),
            other => panic!("{other:?}"),
        }
    }
}
