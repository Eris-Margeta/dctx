// SPDX-License-Identifier: Apache-2.0
//! Worker lifecycle: pre-exec admission, negotiation, invocation, cancellation and reaping.
//! Implements Lifecycle.tla `EnableOrRefuse`, `Start`, `Negotiate`, `Initialize`, `Invoke`, `Finish`,
//! `CancelStalled`, `Halt`, `AcknowledgeOrForce`, `Reap`, `ObserveReapPending` (PP4, PP8 safety) and the
//! decision 080 disposition: proven confinement, or the authenticated bundled-git origin only. Depends on trace.
//! Used by the supervisor service; broker admission is open only while `Running`.

use dctx_plugin_protocol::{ErrorCode, Phase as WirePhase};

use crate::domain::trace::{ModelAction, Trace};
use crate::error::HostRefusal;

/// Authenticated origin of the artifact, established by the host (never by the worker).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// Installed third-party plugin.
    ThirdParty,
    /// The git plugin bundled in, and authenticated by, the containing distribution.
    BundledGit,
    /// An independently installed official build overriding the bundled one.
    OfficialOverride,
}

/// Host-established facts checked before exec.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LaunchFacts {
    /// Authenticated origin.
    pub origin: Origin,
    /// Artifact digest/signature/trust verification passed.
    pub verified: bool,
    /// Protocol and target compatible.
    pub compatible: bool,
    /// A tested confinement provider establishes the boundary pre-exec.
    pub confined: bool,
}

impl LaunchFacts {
    /// decision 080 admitted disposition: proven confinement, or the authenticated bundled-git exception.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn disposition_admitted(&self) -> bool {
        self.confined || self.origin == Origin::BundledGit
    }
}

/// Why a worker stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopReason {
    /// Finished its admitted work.
    Completed,
    /// User/host cancellation or disable.
    Cancelled,
    /// Evidence showed a stall.
    Stalled,
    /// Unknown activity under a fail-closed unattended policy.
    UnresolvedSuspicion,
    /// Resource limit or backpressure overflow.
    ResourceLimit,
    /// Protocol violation.
    InvalidMessage,
}

/// Lifecycle phase (Lifecycle.tla `phase`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkerPhase {
    /// Installed, not admitted.
    Installed,
    /// Pre-exec gates refused.
    Refused,
    /// Admitted for exec.
    Enabled,
    /// Process started.
    Spawned,
    /// Version negotiated.
    Negotiated,
    /// `initialize` returned ready.
    Ready,
    /// An invocation is active.
    Running,
    /// Cancellation requested; admission closed.
    Cancelling(StopReason),
    /// Process exited or was forced; not yet reaped.
    Exited(StopReason),
    /// Process tree reaped.
    Reaped(StopReason),
}

/// One worker's lifecycle.
#[derive(Clone, Debug)]
pub struct Worker {
    facts: LaunchFacts,
    phase: WorkerPhase,
    reap_pending: bool,
    trace: Trace,
}

impl Worker {
    /// An installed worker with host-established `facts`.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn new(facts: LaunchFacts) -> Self {
        Self { facts, phase: WorkerPhase::Installed, reap_pending: false, trace: Trace::default() }
    }

    /// `EnableOrRefuse`: verified, compatible and an admitted confinement disposition.
    /// # Errors
    /// Refuses the wrong lifecycle phase, unverified/incompatible artifacts, or an inadmissible confinement disposition.
    pub fn enable(&mut self) -> Result<(), HostRefusal> {
        self.expect(WorkerPhase::Installed)?;
        self.trace.push(ModelAction::LifecycleEnableOrRefuse);
        let refusal = if !self.facts.verified {
            Some(HostRefusal::new(ErrorCode::IntegrityFailure, WirePhase::Verify, "artifact-not-verified"))
        } else if !self.facts.compatible {
            Some(HostRefusal::new(ErrorCode::UnsupportedVersion, WirePhase::Resolve, "incompatible-artifact"))
        } else if !self.facts.disposition_admitted() {
            Some(HostRefusal::new(ErrorCode::UnsupportedPlatform, WirePhase::Confine, "confinement-not-proven"))
        } else {
            None
        };
        if let Some(refusal) = refusal {
            self.phase = WorkerPhase::Refused;
            Err(refusal)
        } else {
            self.phase = WorkerPhase::Enabled;
            Ok(())
        }
    }

    /// `Start`: the process was spawned (confinement established pre-exec by the adapter).
    /// # Errors
    /// Refuses unless the worker is enabled.
    pub fn started(&mut self) -> Result<(), HostRefusal> {
        self.step(WorkerPhase::Enabled, WorkerPhase::Spawned, ModelAction::LifecycleStart)
    }

    /// `Negotiate`: hello/answer selected a version.
    /// # Errors
    /// Refuses unless the worker has spawned.
    pub fn negotiated(&mut self) -> Result<(), HostRefusal> {
        self.step(WorkerPhase::Spawned, WorkerPhase::Negotiated, ModelAction::LifecycleNegotiate)
    }

    /// `Initialize`: `initialize` returned ready.
    /// # Errors
    /// Refuses unless protocol negotiation has completed.
    pub fn initialized(&mut self) -> Result<(), HostRefusal> {
        self.step(WorkerPhase::Negotiated, WorkerPhase::Ready, ModelAction::LifecycleInitialize)
    }

    /// `Invoke`: one invocation becomes active (one in flight per worker in v1).
    /// # Errors
    /// Refuses an active invocation or any phase other than ready.
    pub fn invoke(&mut self) -> Result<(), HostRefusal> {
        if matches!(self.phase, WorkerPhase::Running) {
            return Err(HostRefusal::new(ErrorCode::Busy, WirePhase::Invoke, "invocation-active"));
        }
        self.step(WorkerPhase::Ready, WorkerPhase::Running, ModelAction::LifecycleInvoke)
    }

    /// `Finish`: the invocation's terminal response arrived; the session returns to ready.
    /// # Errors
    /// Refuses unless an invocation is running.
    pub fn finish(&mut self) -> Result<(), HostRefusal> {
        self.step(WorkerPhase::Running, WorkerPhase::Ready, ModelAction::LifecycleFinish)
    }

    /// `CancelStalled`/`Halt`: close admission and request cancellation; idempotent once cancelling.
    /// # Errors
    /// Refuses lifecycle phases in which cancellation is not admitted.
    pub fn cancel(&mut self, reason: StopReason) -> Result<(), HostRefusal> {
        match self.phase {
            WorkerPhase::Enabled
            | WorkerPhase::Spawned
            | WorkerPhase::Negotiated
            | WorkerPhase::Ready
            | WorkerPhase::Running => {
                self.trace.push(match reason {
                    StopReason::Stalled | StopReason::UnresolvedSuspicion => ModelAction::LifecycleCancelStalled,
                    _ => ModelAction::LifecycleHalt,
                });
                self.phase = WorkerPhase::Cancelling(reason);
                Ok(())
            }
            WorkerPhase::Cancelling(_) => Ok(()),
            _ => Err(self.wrong_phase()),
        }
    }

    /// `AcknowledgeOrForce`: the worker acknowledged, exited or was forced after grace.
    /// # Errors
    /// Refuses unless the worker is cancelling or its exit is otherwise admissible.
    pub fn exited(&mut self) -> Result<(), HostRefusal> {
        let reason = match self.phase {
            WorkerPhase::Cancelling(reason) => reason,
            WorkerPhase::Ready => StopReason::Completed,
            _ => return Err(self.wrong_phase()),
        };
        self.phase = WorkerPhase::Exited(reason);
        self.trace.push(ModelAction::LifecycleAcknowledgeOrForce);
        Ok(())
    }

    /// `Reap` when the OS reaped the tree, else `ObserveReapPending`: visible, never fabricated.
    /// # Errors
    /// Refuses unless an exit has been recorded.
    pub fn reap(&mut self, reaped: bool) -> Result<(), HostRefusal> {
        let WorkerPhase::Exited(reason) = self.phase else { return Err(self.wrong_phase()) };
        if reaped {
            self.phase = WorkerPhase::Reaped(reason);
            self.reap_pending = false;
            self.trace.push(ModelAction::LifecycleReap);
        } else {
            self.reap_pending = true;
            self.trace.push(ModelAction::LifecycleObserveReapPending);
        }
        Ok(())
    }

    /// Whether broker admission is open (PP8: never after cancel, halt or exit).
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn admits(&self) -> bool {
        self.phase == WorkerPhase::Running
    }

    /// Current phase.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn phase(&self) -> WorkerPhase {
        self.phase
    }

    /// Whether the OS has not yet reaped an exited tree.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn reap_pending(&self) -> bool {
        self.reap_pending
    }

    /// Model actions taken.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn trace(&self) -> &Trace {
        &self.trace
    }

    fn expect(&self, phase: WorkerPhase) -> Result<(), HostRefusal> {
        if self.phase == phase { Ok(()) } else { Err(self.wrong_phase()) }
    }

    fn step(&mut self, from: WorkerPhase, to: WorkerPhase, action: ModelAction) -> Result<(), HostRefusal> {
        self.expect(from)?;
        self.phase = to;
        self.trace.push(action);
        Ok(())
    }

    #[allow(clippy::unused_self, reason = "Keep lifecycle refusal construction on the worker alongside its phase-check helpers.")]
    fn wrong_phase(&self) -> HostRefusal {
        HostRefusal::new(ErrorCode::InvalidMessage, WirePhase::Invoke, "wrong-lifecycle-phase")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(origin: Origin, confined: bool) -> LaunchFacts {
        LaunchFacts { origin, verified: true, compatible: true, confined }
    }

    #[test]
    fn third_party_requires_proven_confinement() {
        assert!(Worker::new(facts(Origin::ThirdParty, true)).enable().is_ok());
        let refused = Worker::new(facts(Origin::ThirdParty, false)).enable().expect_err("unconfined");
        assert_eq!(refused.code, ErrorCode::UnsupportedPlatform);
        let override_refused = Worker::new(facts(Origin::OfficialOverride, false)).enable();
        assert_eq!(override_refused.map_err(|e| e.reason), Err("confinement-not-proven"));
        assert!(Worker::new(facts(Origin::BundledGit, false)).enable().is_ok());
    }

    #[test]
    fn unverified_or_incompatible_never_enable() {
        let mut unverified = facts(Origin::BundledGit, true);
        unverified.verified = false;
        assert_eq!(Worker::new(unverified).enable().map_err(|e| e.code), Err(ErrorCode::IntegrityFailure));
        let mut incompatible = facts(Origin::BundledGit, true);
        incompatible.compatible = false;
        assert!(Worker::new(incompatible).enable().is_err());
    }

    #[test]
    fn no_invoke_before_ready_and_cancel_closes_admission() {
        let mut worker = Worker::new(facts(Origin::BundledGit, false));
        worker.enable().expect("enabled");
        worker.started().expect("spawned");
        assert!(worker.invoke().is_err());
        worker.negotiated().expect("negotiated");
        worker.initialized().expect("ready");
        worker.invoke().expect("running");
        assert!(worker.admits());
        assert_eq!(worker.invoke().map_err(|e| e.code), Err(ErrorCode::Busy));
        worker.cancel(StopReason::Cancelled).expect("cancel");
        assert!(!worker.admits());
        assert!(worker.invoke().is_err());
        worker.exited().expect("exited");
        worker.reap(false).expect("pending");
        assert!(worker.reap_pending());
        assert_eq!(worker.phase(), WorkerPhase::Exited(StopReason::Cancelled));
        worker.reap(true).expect("reaped");
        assert_eq!(worker.phase(), WorkerPhase::Reaped(StopReason::Cancelled));
    }
}
