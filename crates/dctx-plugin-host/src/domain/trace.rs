// SPDX-License-Identifier: Apache-2.0
//! Model-action trace: every host transition names the PL-M TLA+ action it refines.
//! Implements the "Host transition correspondence" table of docs/formal/dctx-plugins/README.md for
//! Authority, Receipts, Resources, Lifecycle and Hooks. Depends on nothing; used by every service so
//! property tests can assert that host behaviour follows the model's action sequences.

/// A PL-M model action, qualified by its module.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ModelAction {
    /// `Authority.Initialize`: session became ready.
    AuthorityInitialize,
    /// `Authority.Configure`: manifest configuration narrowed the request.
    AuthorityConfigure,
    /// `Authority.Consent`: explicit host approval widened a ceiling.
    AuthorityConsent,
    /// `Authority.Narrow`: consent/organization/selection ceiling narrowed; epoch advanced.
    AuthorityNarrow,
    /// `Authority.ChangeSelection`: source or provenance decision changed; epoch advanced.
    AuthorityChangeSelection,
    /// `Authority.Halt`: disable/halt closed admission.
    AuthorityHalt,
    /// `Authority.Admit`: an action entered admission under the current grant.
    AuthorityAdmit,
    /// `Authority.Commit`: commit-time recheck passed.
    AuthorityCommit,
    /// `Authority.Read`: bytes delivered after selection and provenance checks.
    AuthorityRead,
    /// `Authority.Admin`: named administrative recipe resource used.
    AuthorityAdmin,
    /// `Receipts.Request`: durable intent allocated.
    ReceiptsRequest,
    /// `Receipts.Execute`: external effect performed.
    ReceiptsExecute,
    /// `Receipts.Complete`: completion persisted.
    ReceiptsComplete,
    /// `Receipts.CrashOrUncertain`: outcome unknown or partial.
    ReceiptsCrashOrUncertain,
    /// `Receipts.AuditFailure`: audit became unavailable.
    ReceiptsAuditFailure,
    /// `Receipts.Repeat`: replay returned the existing receipt or a conflict.
    ReceiptsRepeat,
    /// `Receipts.Reconcile`: evidence-backed resolution of an uncertain entry.
    ReceiptsReconcile,
    /// `Receipts.Rotate`: completed evidence archived, identity retained.
    ReceiptsRotate,
    /// `Resources.Receive`: a charge was checked against every ceiling.
    ResourcesReceive,
    /// `Resources.Drain`: a charge was released.
    ResourcesDrain,
    /// `Lifecycle.EnableOrRefuse`.
    LifecycleEnableOrRefuse,
    /// `Lifecycle.Start`.
    LifecycleStart,
    /// `Lifecycle.Negotiate`.
    LifecycleNegotiate,
    /// `Lifecycle.Initialize`.
    LifecycleInitialize,
    /// `Lifecycle.Invoke`.
    LifecycleInvoke,
    /// `Lifecycle.Advance`: useful progress.
    LifecycleAdvance,
    /// `Lifecycle.ResolveDependency`.
    LifecycleResolveDependency,
    /// `Lifecycle.Finish`.
    LifecycleFinish,
    /// `Lifecycle.Observe`: no useful progress since the last sample.
    LifecycleObserve,
    /// `Lifecycle.Probe`.
    LifecycleProbe,
    /// `Lifecycle.Classify`.
    LifecycleClassify,
    /// `Lifecycle.ObserveWait`: a waited dependency settled.
    LifecycleObserveWait,
    /// `Lifecycle.CancelStalled`.
    LifecycleCancelStalled,
    /// `Lifecycle.Halt`: user/host cancellation.
    LifecycleHalt,
    /// `Lifecycle.AcknowledgeOrForce`.
    LifecycleAcknowledgeOrForce,
    /// `Lifecycle.Reap`.
    LifecycleReap,
    /// `Lifecycle.ObserveReapPending`.
    LifecycleObserveReapPending,
    /// `Lifecycle.ElapsedObservation`: time passed; never a kill by itself.
    LifecycleElapsedObservation,
    /// `Hooks.Plan`.
    HooksPlan,
    /// `Hooks.RunHook`.
    HooksRunHook,
    /// `Hooks.Publish`.
    HooksPublish,
    /// `Hooks.Report`.
    HooksReport,
}

impl ModelAction {
    /// Every model action the host refines, in declaration order.
    pub const ALL: [Self; 42] = [
        Self::AuthorityInitialize,
        Self::AuthorityConfigure,
        Self::AuthorityConsent,
        Self::AuthorityNarrow,
        Self::AuthorityChangeSelection,
        Self::AuthorityHalt,
        Self::AuthorityAdmit,
        Self::AuthorityCommit,
        Self::AuthorityRead,
        Self::AuthorityAdmin,
        Self::ReceiptsRequest,
        Self::ReceiptsExecute,
        Self::ReceiptsComplete,
        Self::ReceiptsCrashOrUncertain,
        Self::ReceiptsAuditFailure,
        Self::ReceiptsRepeat,
        Self::ReceiptsReconcile,
        Self::ReceiptsRotate,
        Self::ResourcesReceive,
        Self::ResourcesDrain,
        Self::LifecycleEnableOrRefuse,
        Self::LifecycleStart,
        Self::LifecycleNegotiate,
        Self::LifecycleInitialize,
        Self::LifecycleInvoke,
        Self::LifecycleAdvance,
        Self::LifecycleResolveDependency,
        Self::LifecycleFinish,
        Self::LifecycleObserve,
        Self::LifecycleProbe,
        Self::LifecycleClassify,
        Self::LifecycleObserveWait,
        Self::LifecycleCancelStalled,
        Self::LifecycleHalt,
        Self::LifecycleAcknowledgeOrForce,
        Self::LifecycleReap,
        Self::LifecycleObserveReapPending,
        Self::LifecycleElapsedObservation,
        Self::HooksPlan,
        Self::HooksRunHook,
        Self::HooksPublish,
        Self::HooksReport,
    ];
}

/// Most trace entries retained; older entries are dropped first.
pub const TRACE_CAP: usize = 4096;

/// A bounded, append-only record of model actions taken by one host component.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Trace {
    actions: Vec<ModelAction>,
    dropped: u64,
}

impl Trace {
    /// Records `action`, dropping the oldest entry when full.
    pub fn push(&mut self, action: ModelAction) {
        if self.actions.len() >= TRACE_CAP {
            self.actions.remove(0);
            self.dropped = self.dropped.saturating_add(1);
        }
        self.actions.push(action);
    }

    /// Retained actions in order.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn actions(&self) -> &[ModelAction] {
        &self.actions
    }

    /// Whether `action` was recorded.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn contains(&self, action: ModelAction) -> bool {
        self.actions.contains(&action)
    }

    /// Number of times `action` was recorded.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn count(&self, action: ModelAction) -> usize {
        self.actions.iter().filter(|a| **a == action).count()
    }

    /// Whether `first` occurs before the first later `second`.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn precedes(&self, first: ModelAction, second: ModelAction) -> bool {
        match (self.actions.iter().position(|a| *a == first), self.actions.iter().rposition(|a| *a == second)) {
            (Some(a), Some(b)) => a < b,
            _ => false,
        }
    }

    /// Entries dropped because the trace was full.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn dropped(&self) -> u64 {
        self.dropped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trace_is_bounded_and_ordered() {
        let mut trace = Trace::default();
        trace.push(ModelAction::ReceiptsRequest);
        trace.push(ModelAction::ReceiptsExecute);
        assert!(trace.precedes(ModelAction::ReceiptsRequest, ModelAction::ReceiptsExecute));
        assert!(!trace.precedes(ModelAction::ReceiptsExecute, ModelAction::ReceiptsRequest));
        for _ in 0..TRACE_CAP {
            trace.push(ModelAction::ResourcesReceive);
        }
        assert_eq!(trace.actions().len(), TRACE_CAP);
        assert_eq!(trace.dropped(), 2);
        assert_eq!(trace.count(ModelAction::ResourcesReceive), TRACE_CAP);
    }
}
