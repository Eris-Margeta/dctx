//! The protection use case: bind, read, plan, upload, compare-and-replace, then optionally stage.
//! Implements PLUGINS.md §6.2 steps 2–8 and the git.md outcome table through the broker port only:
//! no repository => skipped with zero reads; no `.gitignore` => skipped, never created; conflict/unknown
//! coverage => warning with zero writes; a committed edit keeps its receipt whatever staging does.
//! Depends on domain, ports, app::{binding, transfer, stage}; used by app::hook and app::command.

use dctx_plugin_protocol::records::broker::FileReplace;
use dctx_plugin_protocol::records::common::{Limits, Severity};
use dctx_plugin_protocol::records::events::ProgressUnit;
use dctx_plugin_protocol::records::results::InvokeStatus;
use dctx_plugin_protocol::records::scalars::Token;
use dctx_plugin_protocol::{ErrorCode, Phase};

use crate::app::binding::{Binding, Grants};
use crate::app::failure::Failure;
use crate::app::stage::{StageRequest, stage};
use crate::app::transfer::{read_file, upload};
use crate::domain::coverage::NestedRules;
use crate::domain::outcome::{DiagnosticCode, Reason, Staging};
use crate::domain::plan::{Plan, plan};
use crate::domain::text::{IgnoreText, TextRefusal};
use crate::ports::{Broker, BrokerFailure};

/// Replay key of the ignore-file replacement (the host binds it to artifact, project and invocation).
pub const REPLACE_ACTION: &str = "action:replace";

/// How staging is decided for this run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StageMode {
    /// Stage after a committed edit when config says so.
    AfterEdit(bool),
    /// `protect --stage`: a new explicit request for the verified current file, edited or not.
    Explicit,
    /// `status`: read and plan only; never write or stage.
    Inspect,
}

/// One protection run's inputs.
#[derive(Clone, Copy, Debug)]
pub struct Run<'a> {
    /// Session binding.
    pub binding: &'a Binding,
    /// Grants in force.
    pub grants: Grants,
    /// Authoritative host version for the marker.
    pub host_version: &'a str,
    /// Repository handle bound for this run.
    pub repository: Option<&'a Token>,
    /// A planned output whose name the v1 rules cannot vouch for.
    pub custom_output: bool,
    /// Staging decision.
    pub mode: StageMode,
    /// Effective limits.
    pub limits: &'a Limits,
}

/// A successful (possibly degraded) protection result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    /// Terminal status.
    pub status: InvokeStatus,
    /// Declared reason.
    pub reason: &'static str,
    /// Host receipts of committed effects, in order.
    pub receipts: Vec<Token>,
    /// Declared diagnostics.
    pub diagnostics: Vec<(DiagnosticCode, Severity)>,
    /// Whether rules, marker and coverage are now verified.
    pub protected: bool,
    /// Whether the broker staged the file.
    pub staged: bool,
}

impl Report {
    /// A result with no effects.
    pub fn plain(status: InvokeStatus, reason: Reason) -> Self {
        Self { status, reason: reason.as_str(), receipts: Vec::new(), diagnostics: Vec::new(), protected: false, staged: false }
    }

    fn warning(reason: Reason, diagnostic: DiagnosticCode) -> Self {
        Self { diagnostics: vec![(diagnostic, Severity::Warning)], ..Self::plain(InvokeStatus::Warning, reason) }
    }
}

fn text_failure(refusal: TextRefusal) -> Failure {
    match refusal {
        TextRefusal::Oversized => Failure::limit(refusal.reason(), "max_bytes"),
        _ => Failure::refused(ErrorCode::InvalidConfig, Phase::Invoke, refusal.reason()),
    }
}

fn nested_evidence(broker: &mut dyn Broker, run: &Run<'_>, max: u64) -> Result<Vec<NestedRules>, Failure> {
    let mut evidence = Vec::new();
    for handle in &run.binding.nested {
        let rules = match read_file(broker, handle, max, run.limits, None) {
            Ok(delivered) => IgnoreText::parse(&delivered.bytes, max).map_or(NestedRules::Unavailable, NestedRules::Text),
            Err(failure) if matches!(failure.code, ErrorCode::Cancelled | ErrorCode::InvalidMessage) => return Err(failure),
            Err(_) => NestedRules::Unavailable,
        };
        evidence.push(rules);
    }
    Ok(evidence)
}

/// Runs protection for one invocation.
pub fn protect(broker: &mut dyn Broker, run: &Run<'_>) -> Result<Report, Failure> {
    let Some(repository) = run.repository else { return Ok(Report::plain(InvokeStatus::Skipped, Reason::NotARepository)) };
    match run.binding.repository(repository) {
        Some(bound) if bound.ordinary => {}
        Some(_) => return Ok(Report::plain(InvokeStatus::Skipped, Reason::UnsupportedRepositoryLayout)),
        None => return Err(Failure::refused(ErrorCode::InvalidMessage, Phase::Invoke, "unbound-repository")),
    }
    let Some(ignore) = run.binding.ignore.as_ref() else {
        return Ok(Report::plain(InvokeStatus::Skipped, Reason::MissingIgnore));
    };
    let Some(max_read) = run.grants.read else {
        return Err(Failure::refused(ErrorCode::Denied, Phase::Invoke, "read-not-granted"));
    };
    let delivered = read_file(broker, ignore, max_read, run.limits, Some("read-ignore"))?;
    let text = IgnoreText::parse(&delivered.bytes, max_read).map_err(text_failure)?;
    let nested = nested_evidence(broker, run, max_read)?;
    let append = match plan(&text, run.host_version, &nested, run.custom_output) {
        Plan::RulesConflict => return Ok(Report::warning(Reason::RulesConflict, DiagnosticCode::RulesConflict)),
        Plan::CoverageUnknown => return Ok(Report::warning(Reason::CoverageUnknown, DiagnosticCode::CoverageUnknown)),
        Plan::AlreadyProtected => {
            let mut report = Report { protected: true, ..Report::plain(InvokeStatus::Ok, Reason::AlreadyProtected) };
            if run.mode == StageMode::Explicit {
                let request = StageRequest { resource: ignore, precondition: &delivered.precondition, repository };
                let (staging, receipt) = stage(broker, run, &request, &[])?;
                apply_explicit(&mut report, staging, receipt);
            }
            return Ok(report);
        }
        Plan::Append(_) if run.mode == StageMode::Inspect => return Ok(Report::plain(InvokeStatus::Ok, Reason::Completed)),
        Plan::Append(append) => append,
    };
    let Some((max_growth, max_bytes)) = run.grants.write else {
        return Err(Failure::refused(ErrorCode::Denied, Phase::Invoke, "write-not-granted"));
    };
    if append.growth > max_growth {
        return Err(Failure::limit("growth-limit", "max_growth_bytes"));
    }
    if u64::try_from(append.replacement.len()).unwrap_or(u64::MAX) > max_bytes {
        return Err(Failure::limit("size-limit", "max_bytes"));
    }
    let sealed = upload(broker, ignore, &delivered.precondition, &append.replacement, run.limits)?;
    let replace = FileReplace {
        resource: ignore.clone(),
        precondition: delivered.precondition.clone(),
        upload: sealed,
        action_id: action(REPLACE_ACTION)?,
    };
    let replaced = broker.file_replace(&replace).map_err(|failure| match failure {
        BrokerFailure::Refused(error) => Failure::from_broker(BrokerFailure::Refused(error), false, &[]),
        other => Failure::from_broker(other, true, &[]),
    })?;
    broker.progress("replace-ignore", 1, Some(1), ProgressUnit::Actions);
    let receipts = vec![replaced.receipt.clone()];
    let staging = match run.mode {
        StageMode::AfterEdit(false) | StageMode::Inspect => (Staging::NotRequested, None),
        StageMode::AfterEdit(true) | StageMode::Explicit => {
            let request = StageRequest { resource: ignore, precondition: &replaced.precondition, repository };
            stage(broker, run, &request, &receipts)?
        }
    };
    Ok(committed(receipts, staging))
}

fn committed(mut receipts: Vec<Token>, (staging, receipt): (Staging, Option<Token>)) -> Report {
    let (warning, reason, diagnostic) = staging.report();
    receipts.extend(receipt);
    let severity = if warning { Severity::Warning } else { Severity::Info };
    Report {
        status: if warning { InvokeStatus::Warning } else { InvokeStatus::Ok },
        reason: reason.as_str(),
        receipts,
        diagnostics: diagnostic.map(|code| (code, severity)).into_iter().collect(),
        protected: true,
        staged: staging == Staging::Staged,
    }
}

fn apply_explicit(report: &mut Report, staging: Staging, receipt: Option<Token>) {
    report.receipts.extend(receipt);
    match staging {
        Staging::Staged => {
            report.reason = Reason::Completed.as_str();
            report.staged = true;
            report.diagnostics.push((DiagnosticCode::WholeFileStaging, Severity::Info));
        }
        Staging::NotRequested => {}
        Staging::GitMissing => {
            report.status = InvokeStatus::Warning;
            report.reason = Reason::GitMissing.as_str();
            report.diagnostics.push((DiagnosticCode::GitMissing, Severity::Warning));
        }
        Staging::Denied | Staging::Failed => {
            report.status = InvokeStatus::Warning;
            report.reason = Reason::StagingDenied.as_str();
            report.diagnostics.push((DiagnosticCode::StagingDenied, Severity::Warning));
        }
    }
}

/// A fixed replay key token.
pub fn action(text: &'static str) -> Result<Token, Failure> {
    Token::new(text).map_err(|_| Failure::refused(ErrorCode::InvalidMessage, Phase::Invoke, "invalid-action-id"))
}
