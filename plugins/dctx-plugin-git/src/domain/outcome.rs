// SPDX-License-Identifier: Apache-2.0
//! Declared reason and diagnostic codes of the dctx/git manifest, and the staging verdicts they report.
//! Implements manifest-git.json `reason_codes`/`diagnostic_codes` and the git.md outcome table
//! ("Required visible truth"): a successful edit keeps its receipt whatever staging does (G-STAGE).
//! Pure; used by `app::protect`, `app::stage` and `app::command`.

/// A declared invoke reason code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    /// Rules written (and staged when requested).
    Completed,
    /// Nothing to do: zero writes, zero Git calls.
    AlreadyProtected,
    /// No repository binding.
    NotARepository,
    /// No existing `.gitignore`; never created.
    MissingIgnore,
    /// Only outside-project exports.
    OutsideProject,
    /// The edit committed; staging did not.
    RulesWrittenNotStaged,
    /// A known negation/custom conflict.
    RulesConflict,
    /// Coverage evidence missing.
    CoverageUnknown,
    /// The repository layout needs an explicit binding.
    UnsupportedRepositoryLayout,
    /// An explicit staging request was not permitted or did not stage.
    StagingDenied,
    /// An explicit staging request found no Git program.
    GitMissing,
}

impl Reason {
    /// Wire spelling (one of the manifest's `reason_codes`).
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::AlreadyProtected => "already-protected",
            Self::NotARepository => "not-a-repository",
            Self::MissingIgnore => "missing-ignore",
            Self::OutsideProject => "outside-project",
            Self::RulesWrittenNotStaged => "rules-written-not-staged",
            Self::RulesConflict => "rules-conflict",
            Self::CoverageUnknown => "coverage-unknown",
            Self::UnsupportedRepositoryLayout => "unsupported-repository-layout",
            Self::StagingDenied => "staging-denied",
            Self::GitMissing => "git-missing",
        }
    }
}

/// A declared diagnostic code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagnosticCode {
    /// Staging was not granted, offered or permitted.
    StagingDenied,
    /// The Git program is missing.
    GitMissing,
    /// Whole-file staging disclosure (the user's own edits are staged too).
    WholeFileStaging,
    /// Exports already tracked stay tracked; no untrack is attempted.
    AlreadyTracked,
    /// The edit committed but staging failed.
    RulesWrittenNotStaged,
    /// A negation or custom output defeats the rules.
    RulesConflict,
    /// Coverage evidence is missing.
    CoverageUnknown,
}

impl DiagnosticCode {
    /// Wire spelling (one of the manifest's `diagnostic_codes`).
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::StagingDenied => "staging-denied",
            Self::GitMissing => "git-missing",
            Self::WholeFileStaging => "whole-file-staging",
            Self::AlreadyTracked => "already-tracked",
            Self::RulesWrittenNotStaged => "rules-written-not-staged",
            Self::RulesConflict => "rules-conflict",
            Self::CoverageUnknown => "coverage-unknown",
        }
    }
}

/// What happened to the staging request after a committed edit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Staging {
    /// `stage=false`: no Git request at all.
    NotRequested,
    /// No program grant, API or permission.
    Denied,
    /// The broker reported the Git program missing.
    GitMissing,
    /// Git ran (or was refused) and did not stage; nothing uncertain.
    Failed,
    /// The broker staged the whole file.
    Staged,
}

impl Staging {
    /// The visible (status is warning?, reason, diagnostic) for a committed edit.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub const fn report(self) -> (bool, Reason, Option<DiagnosticCode>) {
        match self {
            Self::NotRequested => (false, Reason::Completed, None),
            Self::Staged => (false, Reason::Completed, Some(DiagnosticCode::WholeFileStaging)),
            Self::Denied => (true, Reason::RulesWrittenNotStaged, Some(DiagnosticCode::StagingDenied)),
            Self::GitMissing => (true, Reason::RulesWrittenNotStaged, Some(DiagnosticCode::GitMissing)),
            Self::Failed => (true, Reason::RulesWrittenNotStaged, Some(DiagnosticCode::RulesWrittenNotStaged)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_staging_failure_keeps_a_visible_warning() {
        assert_eq!(Staging::NotRequested.report(), (false, Reason::Completed, None));
        for staging in [Staging::Denied, Staging::GitMissing, Staging::Failed] {
            let (warning, reason, diagnostic) = staging.report();
            assert!(warning && reason == Reason::RulesWrittenNotStaged && diagnostic.is_some(), "{staging:?}");
        }
        assert_eq!(Staging::GitMissing.report().2.map(DiagnosticCode::as_str), Some("git-missing"));
        assert_eq!(Reason::UnsupportedRepositoryLayout.as_str(), "unsupported-repository-layout");
    }
}
