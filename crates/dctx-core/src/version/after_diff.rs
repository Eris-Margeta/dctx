// SPDX-License-Identifier: Apache-2.0
//! Version selection from a completed pack diff.
//! Implements pack versioning; docs/reviews/dctx-legacy-removal.md F11.
//! Depends on the parent Version and `crate::diff::DiffReport`; pure.
//! Re-exported by version for CLI and pack callers.
//! An unchanged tree retains its previous version.

use super::Version;
use crate::diff::DiffReport;

/// The version after a pack run, and whether it is a major reset (old diffs dropped).
///
/// No previous snapshot starts at V1 with a reset; an unchanged tree keeps the
/// previous version; otherwise [`Version::calculate_next`] decides.
#[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
pub fn version_after_diff(
    previous: Option<&Version>,
    report: &DiffReport,
    max_diff_files: u32,
) -> (Version, bool) {
    match previous {
        None => (Version::new_v1(), true),
        Some(old) if report.has_changes => old.calculate_next(
            report.stats.files_unchanged,
            report.stats.total_files_in_new_state,
            max_diff_files,
        ),
        Some(old) => (old.clone(), false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_after_diff_mirrors_the_cli_rules() {
        let mut report = DiffReport::default();
        assert_eq!(version_after_diff(None, &report, 999), (Version::new_v1(), true));
        let v13 = Version { major: 1, minor: 3 };
        assert_eq!(version_after_diff(Some(&v13), &report, 999), (v13.clone(), false));
        report.has_changes = true;
        report.stats.files_unchanged = 1;
        report.stats.total_files_in_new_state = 2;
        assert_eq!(version_after_diff(Some(&v13), &report, 999), (Version { major: 1, minor: 4 }, false));
        report.stats.files_unchanged = 0;
        assert_eq!(version_after_diff(Some(&v13), &report, 999), (Version { major: 2, minor: 0 }, true));
    }
}
