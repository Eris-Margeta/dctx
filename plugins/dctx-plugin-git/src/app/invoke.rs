//! Invoke dispatch: the `before-pack-output-written` hook, the `git status|protect` command, and the
//! typed invoke result. Implements PLUGINS.md §6.2 step 1 (project-local text exports only; outside-project
//! => skipped without paths) and §3.2 results (`command_output` only for explicit commands, S-RESULT).
//! Depends on `app::{binding`, protect}, `domain::{command`, coverage}; used by `adapters::worker`.

use std::collections::BTreeMap;

use dctx_plugin_protocol::records::common::{CommandRecord, ConfigValue, Diagnostic, Scalar};
use dctx_plugin_protocol::records::invocation::{HookPayload, Invocation, OutputLocation, PlannedOutput};
use dctx_plugin_protocol::records::results::{InvokeStatus, ResultBody};
use dctx_plugin_protocol::records::scalars::{Name, Text};
use dctx_plugin_protocol::{ErrorCode, Phase, Refusal};

use crate::app::binding::{Binding, parse_config};
use crate::app::failure::Failure;
use crate::app::protect::{Report, Run, StageMode, protect};
use crate::domain::command::{Command, parse};
use crate::domain::coverage::OutputClass;
use crate::domain::outcome::Reason;
use crate::ports::Broker;

/// A finished invoke: the report and whether it answers an explicit command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Answer {
    /// Protection report.
    pub report: Report,
    /// `Some(verb)` for an explicit command (typed command output), `None` for a hook.
    pub command: Option<&'static str>,
}

struct Scope {
    project: bool,
    outside: bool,
    custom: bool,
}

fn scope(outputs: &[PlannedOutput], more_pages: bool) -> Scope {
    let mut scope = Scope { project: false, outside: false, custom: more_pages };
    for output in outputs {
        match output.location {
            OutputLocation::Project => {
                scope.project = true;
                scope.custom |= OutputClass::of_role(output.role.as_str()).is_none();
            }
            OutputLocation::OutsideProject => scope.outside = true,
            OutputLocation::Stdout | OutputLocation::Clipboard => {}
        }
    }
    scope
}

/// Runs one invocation against the session binding.
/// # Errors
/// Refuses invalid command/configuration/bindings, unavailable project context, undeclared hooks, or protection failures.
pub fn invoke(broker: &mut dyn Broker, binding: &Binding, invocation: &Invocation) -> Result<Answer, Failure> {
    let config = parse_config(&invocation.config)?;
    let grants = binding.grants(&invocation.grants, invocation.policy_epoch);
    let mut run = Run {
        binding,
        grants,
        host_version: invocation.host_version.as_str(),
        repository: None,
        custom_output: false,
        mode: StageMode::AfterEdit(config.stage),
        limits: &invocation.limits,
    };
    match &invocation.payload {
        HookPayload::BeforePackOutputWritten { outputs, next, repository } => {
            let scope = scope(outputs, next.is_some());
            if !scope.project {
                if scope.outside {
                    return Ok(Answer { report: Report::plain(InvokeStatus::Skipped, Reason::OutsideProject), command: None });
                }
                return Err(Failure::refused(ErrorCode::InvalidMessage, Phase::Invoke, "no-project-output"));
            }
            run.repository = repository.as_ref();
            run.custom_output = scope.custom;
            Ok(Answer { report: protect(broker, &run)?, command: None })
        }
        HookPayload::Command { alias, verb, args } => {
            let args: Vec<&str> = args.iter().map(Text::as_str).collect();
            let command = parse(alias.as_str(), verb.as_str(), &args)
                .map_err(|refusal| Failure::refused(ErrorCode::InvalidConfig, Phase::Invoke, refusal.reason()))?;
            if invocation.project.is_none() {
                return Err(Failure::refused(ErrorCode::Denied, Phase::Invoke, "no-project-context"));
            }
            run.repository = binding.repositories.first().map(|repository| &repository.handle);
            let (mode, verb) = match command {
                Command::Status => (StageMode::Inspect, "status"),
                Command::Protect { stage: true } => (StageMode::Explicit, "protect"),
                Command::Protect { stage: false } => (StageMode::AfterEdit(config.stage), "protect"),
            };
            run.mode = mode;
            Ok(Answer { report: protect(broker, &run)?, command: Some(verb) })
        }
        _ => Err(Failure::refused(ErrorCode::UnsupportedCapability, Phase::Invoke, "undeclared-hook")),
    }
}

/// The typed invoke result for an answer.
/// # Errors
/// Refuses generated diagnostic/reason names or command text outside protocol scalar bounds.
pub fn result(answer: &Answer) -> Result<ResultBody, Refusal> {
    let report = &answer.report;
    let diagnostics = report
        .diagnostics
        .iter()
        .map(|(code, severity)| -> Result<Diagnostic, Refusal> {
            Ok(Diagnostic { code: Name::new(code.as_str())?, severity: *severity, resource_handle: None })
        })
        .collect::<Result<Vec<_>, Refusal>>()?;
    let command_output = match answer.command {
        None => Vec::new(),
        Some(verb) => {
            let mut fields = BTreeMap::new();
            fields.insert(Name::new("protected")?, ConfigValue::Scalar(Scalar::Bool(report.protected)));
            fields.insert(Name::new("staged")?, ConfigValue::Scalar(Scalar::Bool(report.staged)));
            vec![
                CommandRecord::Text(Text::new(&format!("dctx/git {verb}: {}", report.reason))?),
                CommandRecord::Record { name: Name::new("protection")?, fields },
            ]
        }
    };
    Ok(ResultBody::Invoke {
        status: report.status,
        reason: Name::new(report.reason)?,
        receipts: report.receipts.clone(),
        diagnostics,
        command_output,
    })
}
