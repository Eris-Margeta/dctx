// SPDX-License-Identifier: Apache-2.0
//! The staging request: `program.run git.add-ignore` with handles only, and how its answer is reported.
//! Implements PLUGINS.md §6.2 steps 7–8 and git.md "Missing Git / denied stage / nonzero exit =>
//! warning/rules-written-not-staged ... Crash/uncertain staging => `EffectUnknown`, no automatic retry"
//! (G-STAGE, G-RECIPE). The broker fixes operand, cwd and executable; the plugin sends no argv.
//! Depends on `ports::Broker` and `app::failure`; used by `app::protect`.

use dctx_plugin_protocol::records::broker::GitAddIgnore;
use dctx_plugin_protocol::records::events::ProgressUnit;
use dctx_plugin_protocol::records::results::ProgramStatus;
use dctx_plugin_protocol::records::scalars::Token;
use dctx_plugin_protocol::{EffectState, ErrorCode, Phase};

use crate::app::failure::Failure;
use crate::app::protect::{Run, action};
use crate::domain::outcome::Staging;
use crate::ports::{Broker, BrokerFailure};

/// Replay key of the staging action.
pub const STAGE_ACTION: &str = "action:stage";

/// The bound file and repository to stage.
#[derive(Clone, Copy, Debug)]
pub struct StageRequest<'a> {
    /// Ignore-file handle.
    pub resource: &'a Token,
    /// Precondition of the just-written (or just-verified) content.
    pub precondition: &'a Token,
    /// Repository administrative handle.
    pub repository: &'a Token,
}

fn uncertain(receipts: &[Token], mut extra: Vec<Token>, reason: &'static str) -> Failure {
    let mut all = receipts.to_vec();
    all.append(&mut extra);
    Failure {
        code: ErrorCode::EffectUnknown,
        phase: Phase::Broker,
        reason,
        effect: EffectState::Unknown,
        receipts: all,
        limit: None,
        host: None,
    }
}

/// Requests staging when granted; `receipts` are the effects already committed in this invocation.
/// # Errors
/// Returns cancellation or uncertain-effect failures when staging cannot be safely classified; ordinary denial is a staging result.
pub fn stage(
    broker: &mut dyn Broker,
    run: &Run<'_>,
    request: &StageRequest<'_>,
    receipts: &[Token],
) -> Result<(Staging, Option<Token>), Failure> {
    if !run.grants.program {
        return Ok((Staging::Denied, None));
    }
    let run_request = GitAddIgnore {
        resource: request.resource.clone(),
        precondition: request.precondition.clone(),
        repository: request.repository.clone(),
        action_id: action(STAGE_ACTION)?,
    };
    let answer = broker.program_run(&run_request);
    broker.progress("stage-ignore", 1, Some(1), ProgressUnit::Actions);
    match answer {
        Ok(ran) if ran.status == ProgramStatus::Succeeded => Ok((Staging::Staged, Some(ran.receipt))),
        Ok(ran) => Ok((Staging::Failed, Some(ran.receipt))),
        Err(BrokerFailure::Refused(error)) => {
            if matches!(error.effect_state, EffectState::Partial | EffectState::Unknown) || error.code == ErrorCode::EffectUnknown {
                return Err(uncertain(receipts, error.receipt_ids.clone(), "staging-unknown"));
            }
            let staging = match error.code {
                ErrorCode::ExternalProgramMissing => Staging::GitMissing,
                ErrorCode::Denied
                | ErrorCode::ConsentRequired
                | ErrorCode::UnsupportedCapability
                | ErrorCode::UnsupportedPlatform
                | ErrorCode::AuditUnavailable => Staging::Denied,
                _ => Staging::Failed,
            };
            Ok((staging, error.receipt_ids.first().cloned()))
        }
        Err(BrokerFailure::Cancelled) => Err(Failure {
            code: ErrorCode::Cancelled,
            phase: Phase::Cancel,
            ..uncertain(receipts, Vec::new(), "cancelled")
        }),
        Err(BrokerFailure::Protocol(_)) => Err(uncertain(receipts, Vec::new(), "staging-unknown")),
    }
}
