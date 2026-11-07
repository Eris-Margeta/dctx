// SPDX-License-Identifier: Apache-2.0
//! The restored ADR-005 rule set: four required literals, the automated-rules header and the host marker.
//! Implements PLUGINS.md §6.1 and plugin-api git.md "Required literals, in order": the marker carries the
//! **host** dctx version, never the plugin version. Pure; no IO.
//! Used by `domain::text` (line recognition), `domain::plan` (append) and `domain::coverage` (output classes).

/// The required literal patterns, in append order.
pub const REQUIRED_PATTERNS: [&str; 4] = [
    ".dctx",
    ".dctx/",
    "*.llmcontext.txt",
    "*.llmcontext-*-diff.txt",
];

/// Header written before appended rules (only when at least one rule is appended).
pub const HEADER: &str = "# [dctx] Automated Rules";

/// Prefix shared by every dctx version marker, current or old.
pub const MARKER_PREFIX: &str = "# [dctx v";

/// Suffix shared by every dctx version marker.
pub const MARKER_SUFFIX: &str = "]";

/// The exact current marker line for `host_version` (without a line ending).
#[allow(
    clippy::must_use_candidate,
    reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
)]
pub fn current_marker(host_version: &str) -> String {
    format!("{MARKER_PREFIX}{host_version}{MARKER_SUFFIX}")
}

/// Whether `line` is the current marker: trimmed exact comparison, as in the historical fast path.
#[allow(
    clippy::must_use_candidate,
    reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
)]
pub fn is_current_marker(line: &str, host_version: &str) -> bool {
    line.trim()
        .strip_prefix(MARKER_PREFIX)
        .and_then(|rest| rest.strip_suffix(MARKER_SUFFIX))
        == Some(host_version)
}

/// Whether `line` is any dctx marker (current or old); old markers are preserved, never removed.
#[allow(
    clippy::must_use_candidate,
    reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
)]
pub fn is_any_marker(line: &str) -> bool {
    line.trim()
        .strip_prefix(MARKER_PREFIX)
        .and_then(|rest| rest.strip_suffix(MARKER_SUFFIX))
        .is_some_and(|v| !v.is_empty())
}

/// The required literal this line spells, if it is a complete known pattern line.
///
/// Git keeps leading whitespace significant and drops only unescaped trailing spaces, so a known
/// line is the literal followed by nothing but spaces. (The historical code trimmed both sides.)
#[allow(
    clippy::must_use_candidate,
    reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
)]
pub fn known_pattern(line: &str) -> Option<usize> {
    let body = line.trim_end_matches(' ');
    REQUIRED_PATTERNS
        .iter()
        .position(|pattern| *pattern == body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_uses_the_given_host_version_exactly() {
        assert_eq!(current_marker("0.26.0"), "# [dctx v0.26.0]");
        assert!(is_current_marker("  # [dctx v0.26.0]  ", "0.26.0"));
        assert!(!is_current_marker("# [dctx v0.25.0]", "0.26.0"));
        assert!(!is_current_marker("# [dctx v0.26.0-rc.1]", "0.26.0"));
        assert!(!is_current_marker("# [dctx v0.26.0] extra", "0.26.0"));
        assert!(is_any_marker("# [dctx v0.25.0]"));
        assert!(!is_any_marker("# [dctx] Automated Rules"));
        assert!(!is_any_marker("# [dctx v]"));
    }

    #[test]
    fn known_patterns_keep_leading_whitespace_significant() {
        assert_eq!(known_pattern(".dctx"), Some(0));
        assert_eq!(known_pattern(".dctx/"), Some(1));
        assert_eq!(known_pattern("*.llmcontext.txt  "), Some(2));
        assert_eq!(known_pattern("  .dctx"), None);
        assert_eq!(known_pattern(".dctx\\ "), None);
        assert_eq!(known_pattern("\t.dctx"), None);
        assert_eq!(known_pattern("!.dctx"), None);
    }
}
