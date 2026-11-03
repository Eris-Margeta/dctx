// SPDX-License-Identifier: Apache-2.0
//! Run one hook boundary through the host session and return a truthful commit result.
//! Implements PLUGINS.md §3.1 and PP10. Required failures before publish block the engine.
//! Depends on [`super::plan`], the request gate and the host supervisor. The session calls [`run_plan`].
//! A failed launch is a hook failure, never an unrestricted worker.

use dctx_plugin_host::ports::{AuditPort, MAX_RECOVERY_RECORDS};
use dctx_plugin_host::{
    Broker, Budget, HandleKind, HookRun, HookStatus, HostRefusal, HostSession, LaunchFacts, Ledger, Need, Reply, StopReason, Supervisor,
    WorkerPhase,
};
use dctx_plugin_protocol::records::broker::{ChunkRead, FileReplace, FileStage, GitAddIgnore};
use dctx_plugin_protocol::records::bytes::Chunk;
use dctx_plugin_protocol::records::capability::Liveness as WireLiveness;
use dctx_plugin_protocol::records::requests::Request;
use dctx_plugin_protocol::records::scalars::{PluginId, ProjectPath, Token};
use dctx_plugin_protocol::{ErrorCode, Phase};
use dctx_template::{Liveness as PlanLiveness, PluginBinding, PluginPlan, Resolution};

use super::plan;
use super::ports::LiveSession;
use super::{Boundary, HookEffect, HookWarning, PluginHost, RequestGate};
use crate::domain::DctxError;

/// One boundary the session is about to run. The host stays outside so the caller can split borrows.
pub(crate) struct BoundaryInput<'a> {
    /// Compiled plugin section.
    pub plan: &'a PluginPlan,
    /// Hook boundary.
    pub boundary: Boundary,
    /// Whether the effective policy includes `.gitignore`.
    pub included: bool,
    /// Policy epoch copied into the worker binding.
    pub epoch: u64,
    /// Injected clock, in milliseconds.
    pub now: u64,
    /// True after the engine has published its result.
    pub committed: bool,
}

#[allow(clippy::needless_pass_by_value, reason = "The invocation consumes its assembled boundary input at the hook transaction boundary.")]
pub(crate) fn run_plan(host: &mut PluginHost, input: BoundaryInput<'_>, warnings: &mut Vec<HookWarning>) -> Result<(), DctxError> {
    let BoundaryInput { plan, boundary, included, epoch, now, committed } = input;
    let plugins = plan::participants(plan, boundary);
    if plugins.is_empty() {
        return Ok(());
    }
    let entries = plan::entries(&plugins).map_err(|refusal| hook_err(boundary, refusal.reason, committed))?;
    let edges = plan::edges(&plugins).map_err(|refusal| hook_err(boundary, refusal.reason, committed))?;
    let mut run = HookRun::plan(&entries, &edges).map_err(|refusal| hook_err(boundary, refusal.reason, committed))?;
    if committed {
        run.published();
    }
    let mut failure: Option<&'static str> = None;
    loop {
        let step = run.next().map(|(plugin, runs)| (plugin.clone(), runs));
        let Some((plugin_id, runs)) = step else { break };
        if !runs {
            let _ = run.record(false, Vec::new(), Vec::new());
            continue;
        }
        let Some(plugin) = find_plugin(&plugins, &plugin_id) else {
            let _ = run.record(false, Vec::new(), Vec::new());
            failure.get_or_insert("plugin-id");
            continue;
        };
        let outcome = invoke(host, plan, plugin, boundary, included, epoch, now);
        let (ok, reason, receipts) = match outcome {
            Ok(receipts) => (true, "ok", receipts),
            Err((reason, receipts)) => (false, reason, receipts),
        };
        let status = run.record(ok, receipts, Vec::new()).map_err(|refusal| hook_err(boundary, refusal.reason, committed))?;
        if status == HookStatus::AdvisoryFailure {
            warnings.push(HookWarning { hook: boundary.as_str(), reason, output_committed: committed });
        }
        if status == HookStatus::RequiredFailure {
            failure.get_or_insert(reason);
        }
    }
    let _ = run.report();
    match failure {
        Some(reason) => Err(hook_err(boundary, reason, committed)),
        None => Ok(()),
    }
}

fn find_plugin<'a>(plugins: &[&'a PluginBinding], id: &PluginId) -> Option<&'a PluginBinding> {
    plugins.iter().copied().find(|plugin| plugin.id == id.as_str())
}

fn invoke(
    host: &mut PluginHost,
    plan: &PluginPlan,
    plugin: &PluginBinding,
    boundary: Boundary,
    included: bool,
    epoch: u64,
    now: u64,
) -> Result<Vec<Token>, (&'static str, Vec<Token>)> {
    let invocation = host.next_token("inv").map_err(|refusal| (refusal.reason, Vec::new()))?;
    let authority = plan::authority(host, plugin, &invocation, included, epoch).map_err(|refusal| (refusal.reason, Vec::new()))?;
    let gate = RequestGate::new(plugin, &authority, included, epoch);
    let origin = plan::origin_of(plugin.origin);
    host.set_launch(plan::launch_kind(origin));
    let facts = LaunchFacts {
        origin,
        verified: plan::verified(plugin),
        compatible: matches!(plugin.resolution, Resolution::Available),
        confined: host.confined(),
    };
    let budget = Budget::new(
        &[plan.limits.read_bytes],
        &[plan.limits.actions],
        &[plan.limits.upload_bytes],
        &[plan.limits.diagnostics_bytes],
        &[plan.limits.outstanding_requests.max(1)],
    );
    let outstanding = usize::try_from(plan.limits.outstanding_requests.min(16)).unwrap_or(1).max(1);
    let mut adapters = host.adapters();
    let recovered = adapters.audit.recover(MAX_RECOVERY_RECORDS).map_err(|_| "recovery-unavailable").and_then(|records| {
        let mut ledger = Ledger::new(64);
        ledger.restore(&records).map_err(|refusal| refusal.reason)?;
        Ok(ledger)
    });
    let mut supervisor = Supervisor::new(facts, liveness(&plan.liveness), adapters.process);
    if let Err(refusal) = supervisor.launch() {
        return Err((refusal.reason, Vec::new()));
    }
    if let Err(refusal) = supervisor.negotiated().and_then(|()| supervisor.initialized()) {
        return Err((refusal.reason, Vec::new()));
    }
    let ledger = match recovered {
        Ok(ledger) => ledger,
        Err(reason) => {
            let _ = supervisor.cancel(StopReason::Completed, now);
            let _ = supervisor.tick(now.saturating_add(60_000));
            return Err((reason, Vec::new()));
        }
    };
    let broker = Broker::new(authority, ledger, budget, plan.limits.frame_bytes.max(1), (adapters.audit, adapters.files, adapters.recipes));
    let mut session = HostSession::new(broker, supervisor, outstanding);
    if let Err(refusal) = session.supervisor_mut().invoke(now) {
        shutdown(&mut session, now);
        return Err((refusal.reason, Vec::new()));
    }
    let effects = host.effects_for(&plugin.id, boundary);
    let applied = apply(&gate, &mut session, host, &effects, now);
    let receipts = session.broker().ledger().committed(&invocation);
    shutdown(&mut session, now);
    match applied {
        Ok(()) => Ok(receipts),
        Err(refusal) => Err((refusal.reason, receipts)),
    }
}

fn apply(
    gate: &RequestGate<'_>,
    session: &mut LiveSession,
    host: &mut PluginHost,
    effects: &[HookEffect],
    now: u64,
) -> Result<(), HostRefusal> {
    let path = ProjectPath::new(".gitignore").map_err(|_| super::ports::bad_token())?;
    let file = session.broker_mut().register(HandleKind::File { path: path.clone() })?;
    let repository = session.broker_mut().register(HandleKind::Repository)?;
    for effect in effects {
        match effect {
            HookEffect::Fail => return Err(HostRefusal::new(ErrorCode::ExternalProgramFailed, Phase::Invoke, "hook-failed")),
            HookEffect::Warn => return Err(HostRefusal::new(ErrorCode::ExternalProgramFailed, Phase::Invoke, "hook-warning")),
            _ => {}
        }
        let read = Request::FileRead(ChunkRead { handle: file.clone(), offset: 0, length: 64, precondition: None });
        let need = Need::FileRead { path: path.clone(), bytes: 64 };
        let Reply::Chunk { precondition: Some(precondition), size, .. } = gate.call(session, host, read, need, now)? else { return Err(super::ports::bad_token()) };
        let (request, need) = match effect {
            HookEffect::ReadGitignore | HookEffect::Fail | HookEffect::Warn => continue,
            HookEffect::ReplaceGitignore(bytes) => {
                let need = Need::FileWrite {
                    path: path.clone(),
                    size: bytes.len() as u64,
                    growth: (bytes.len() as u64).saturating_sub(size),
                    create: false,
                };
                let stage = Request::FileStage(FileStage {
                    resource: file.clone(),
                    precondition: precondition.clone(),
                    upload: None,
                    offset: 0,
                    bytes: Chunk::new(bytes).map_err(|_| HostRefusal::limit("chunk"))?,
                    final_chunk: true,
                });
                let Reply::Staged { upload, sealed: true } = gate.call(session, host, stage, need.clone(), now)? else { return Err(super::ports::bad_token()) };
                (Request::FileReplace(FileReplace { resource: file.clone(), precondition, upload, action_id: host.next_token("a")? }), need)
            }
            HookEffect::StageGitignore => (
                Request::ProgramRun(GitAddIgnore {
                    resource: file.clone(),
                    precondition,
                    repository: repository.clone(),
                    action_id: host.next_token("a")?,
                }),
                Need::GitAddIgnore { path: path.clone() },
            ),
        };
        if !matches!(gate.call(session, host, request, need, now)?, Reply::Receipt { .. }) {
            return Err(super::ports::bad_token());
        }
    }
    Ok(())
}

fn shutdown(session: &mut LiveSession, now: u64) {
    if session.supervisor().worker().phase() == WorkerPhase::Running {
        let _ = session.supervisor_mut().finish();
    }
    session.cancel(StopReason::Completed, now);
    let _ = session.tick(now.saturating_add(60_000));
}

fn liveness(policy: &PlanLiveness) -> WireLiveness {
    match *policy {
        PlanLiveness::Interactive { sample_ms, cancel_grace_ms } => WireLiveness::Interactive { sample_ms, cancel_grace_ms },
        PlanLiveness::UnattendedFailClosed { sample_ms, cancel_grace_ms } => {
            WireLiveness::UnattendedFailClosed { sample_ms, cancel_grace_ms }
        }
        PlanLiveness::UnattendedBounded { sample_ms, cancel_grace_ms, inactivity_ms } => {
            WireLiveness::UnattendedBoundedObservation { sample_ms, cancel_grace_ms, inactivity_ms }
        }
    }
}

fn hook_err(boundary: Boundary, reason: &'static str, committed: bool) -> DctxError {
    DctxError::Hook { hook: boundary.as_str(), reason, output_committed: committed }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dctx_plugin_host::Origin;

    #[test]
    fn bundled_origin_is_not_marked_confined_by_the_mapper() {
        assert_eq!(plan::origin_of(Some(dctx_template::PluginOrigin::Bundled)), Origin::BundledGit);
        assert!(!LaunchFacts { origin: Origin::BundledGit, verified: true, compatible: true, confined: false }.confined);
    }
}
