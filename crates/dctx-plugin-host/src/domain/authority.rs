// SPDX-License-Identifier: Apache-2.0
//! Session authority: grant layers, selection, policy epoch, readiness and halt.
//! Implements Authority.tla `Initialize`, `Configure`, `Consent`, `Narrow`, `ChangeSelection`, `Halt`,
//! `Admit`, `Commit` and `Read` (PP1, PP2, PP4). Admission snapshots the epoch; commit rechecks the
//! current epoch and grant. Depends on grant, selection, binding, trace. Used by the broker service.

use dctx_plugin_protocol::records::capability::Capability;
use dctx_plugin_protocol::records::scalars::ProjectPath;
use dctx_plugin_protocol::{ErrorCode, Phase};

use crate::domain::binding::Binding;
use crate::domain::grant::{GrantLayers, Need, covers};
use crate::domain::selection::Selection;
use crate::domain::trace::{ModelAction, Trace};
use crate::error::HostRefusal;

/// An admitted action: the need and the epoch it was admitted under.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Admission {
    /// The admitted need.
    pub need: Need,
    /// Policy epoch at admission.
    pub epoch: u64,
}

/// Which consent-bearing layer an explicit host narrowing applies to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    /// User consent `U`.
    Consent,
    /// Organization ceiling `O`.
    Organization,
    /// Host/caller ceiling `H`.
    Host,
}

/// Authority state for one bound session.
#[derive(Clone, Debug)]
pub struct Authority {
    layers: GrantLayers,
    selection: Selection,
    binding: Binding,
    epoch: u64,
    ready: bool,
    halted: bool,
    effective: Vec<Capability>,
    trace: Trace,
}

impl Authority {
    /// Builds a not-yet-ready authority under `binding` at policy `epoch`.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn new(layers: GrantLayers, selection: Selection, binding: Binding, epoch: u64) -> Self {
        let effective = layers.effective(&selection);
        Self {
            layers,
            selection,
            binding,
            epoch,
            ready: false,
            halted: false,
            effective,
            trace: Trace::default(),
        }
    }

    /// `Authority.Initialize`: the session reached `ready`.
    /// # Errors
    /// Refuses initialization after the authority has halted.
    pub fn initialize(&mut self) -> Result<(), HostRefusal> {
        if self.halted {
            return Err(HostRefusal::new(
                ErrorCode::Cancelled,
                Phase::Initialize,
                "halted",
            ));
        }
        self.ready = true;
        self.trace.push(ModelAction::AuthorityInitialize);
        Ok(())
    }

    /// `Authority.Configure`: configuration may only narrow the manifest request, never consent.
    pub fn configure(&mut self, requested: &[Capability]) {
        self.layers.manifest =
            crate::domain::grant::intersect_sets(&self.layers.manifest, requested);
        self.refresh(ModelAction::AuthorityConfigure, false);
    }

    /// `Authority.Consent`: an explicit host-side approval replaces a ceiling; the epoch advances.
    /// Epoch exhaustion permanently halts admission; create a newly bound session to continue.
    pub fn approve(&mut self, layer: Layer, ceiling: Vec<Capability>) {
        *self.layer_mut(layer) = ceiling;
        self.refresh(ModelAction::AuthorityConsent, true);
    }

    /// `Authority.Narrow`: intersect a ceiling with `ceiling`; the epoch advances.
    /// Epoch exhaustion permanently halts admission instead of reusing a stale policy epoch.
    pub fn narrow(&mut self, layer: Layer, ceiling: &[Capability]) {
        let narrowed = crate::domain::grant::intersect_sets(self.layer_mut(layer), ceiling);
        *self.layer_mut(layer) = narrowed;
        self.refresh(ModelAction::AuthorityNarrow, true);
    }

    /// `Authority.ChangeSelection`: install a new selection snapshot; the epoch advances.
    /// Epoch exhaustion permanently halts admission instead of reusing a stale policy epoch.
    pub fn change_selection(&mut self, selection: Selection) {
        self.selection = selection;
        self.refresh(ModelAction::AuthorityChangeSelection, true);
    }

    /// `Authority.Halt`: disable/cancel closes admission before any further broker action.
    pub fn halt(&mut self) {
        if !self.halted {
            self.halted = true;
            self.epoch = self.epoch.saturating_add(1);
            self.trace.push(ModelAction::AuthorityHalt);
        }
    }

    /// `Authority.Admit`: binding, readiness, halt and grant containment under the current epoch.
    /// # Errors
    /// Refuses before initialization/after halt, on a binding mismatch, or when no effective capability covers the need.
    pub fn admit(&mut self, handle: &Binding, need: Need) -> Result<Admission, HostRefusal> {
        if !self.ready {
            return Err(HostRefusal::new(
                ErrorCode::InvalidMessage,
                Phase::Initialize,
                "not-ready",
            ));
        }
        if self.halted {
            return Err(HostRefusal::new(
                ErrorCode::Cancelled,
                Phase::Broker,
                "halted",
            ));
        }
        if *handle != self.binding {
            return Err(HostRefusal::denied("binding-mismatch"));
        }
        if !self.effective.iter().any(|cap| covers(cap, &need)) {
            return Err(HostRefusal::denied("not-granted"));
        }
        self.trace.push(ModelAction::AuthorityAdmit);
        Ok(Admission {
            need,
            epoch: self.epoch,
        })
    }

    /// `Authority.Commit`: refuse when halted, the epoch moved or the need left the grant.
    /// # Errors
    /// Refuses after halt, for a stale policy epoch, or if the grant has been revoked.
    pub fn commit(&mut self, admission: &Admission) -> Result<(), HostRefusal> {
        if self.halted {
            return Err(HostRefusal::new(
                ErrorCode::Cancelled,
                Phase::Broker,
                "halted",
            ));
        }
        if admission.epoch != self.epoch {
            return Err(HostRefusal::denied("stale-policy-epoch"));
        }
        if !self
            .effective
            .iter()
            .any(|cap| covers(cap, &admission.need))
        {
            return Err(HostRefusal::denied("grant-revoked"));
        }
        self.trace.push(ModelAction::AuthorityCommit);
        Ok(())
    }

    /// Content precondition for `Authority.Read`: the path is currently `Include`.
    /// # Errors
    /// Refuses content outside the included source selection.
    pub fn check_content(&self, path: &ProjectPath) -> Result<(), HostRefusal> {
        if self.selection.authorizes(path) {
            Ok(())
        } else {
            Err(HostRefusal::denied("source-not-included"))
        }
    }

    /// Provenance precondition for derived/cached delivery: every constituent is currently `Include`.
    /// # Errors
    /// Refuses if any provenance path is outside the included source selection.
    pub fn check_provenance(&self, provenance: &[ProjectPath]) -> Result<(), HostRefusal> {
        if self.selection.authorizes_all(provenance) {
            Ok(())
        } else {
            Err(HostRefusal::denied("provenance-not-included"))
        }
    }

    /// Records `Authority.Read` (delivery) or `Authority.Admin` after the broker served it.
    pub fn record(&mut self, action: ModelAction) {
        self.trace.push(action);
    }

    /// Current policy epoch.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// Whether admission is closed.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn halted(&self) -> bool {
        self.halted
    }

    /// The bound context.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn binding(&self) -> &Binding {
        &self.binding
    }

    /// Current effective grant `G(now)`.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn effective(&self) -> &[Capability] {
        &self.effective
    }

    /// Model actions taken.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn trace(&self) -> &Trace {
        &self.trace
    }

    fn layer_mut(&mut self, layer: Layer) -> &mut Vec<Capability> {
        match layer {
            Layer::Consent => &mut self.layers.consent,
            Layer::Organization => &mut self.layers.organization,
            Layer::Host => &mut self.layers.host,
        }
    }

    fn refresh(&mut self, action: ModelAction, bump: bool) {
        self.effective = self.layers.effective(&self.selection);
        if bump {
            if let Some(epoch) = self.epoch.checked_add(1) {
                self.epoch = epoch;
            } else {
                // A reused epoch could authorize an admission made before this policy change.
                self.halt();
            }
        }
        self.trace.push(action);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::binding::tests::context;
    use crate::domain::selection::Decision;

    fn path() -> ProjectPath {
        ProjectPath::new(".gitignore").expect("path")
    }

    fn read_cap() -> Capability {
        Capability::FileRead {
            path: path(),
            max_bytes: 100,
        }
    }

    fn authority() -> Authority {
        let all = vec![read_cap()];
        let layers = GrantLayers {
            manifest: all.clone(),
            consent: all.clone(),
            organization: all.clone(),
            host: all.clone(),
            adapter: all,
            denies: vec![],
        };
        let mut authority = Authority::new(
            layers,
            Selection::new([(path(), Decision::Include)]),
            context("op:1"),
            0,
        );
        authority.initialize().expect("ready");
        authority
    }

    fn need() -> Need {
        Need::FileRead {
            path: path(),
            bytes: 10,
        }
    }

    #[test]
    fn admit_and_commit_with_positive_control() {
        let mut authority = authority();
        let admission = authority.admit(&context("op:1"), need()).expect("admitted");
        authority.commit(&admission).expect("committed");
        assert!(
            authority
                .trace()
                .precedes(ModelAction::AuthorityAdmit, ModelAction::AuthorityCommit)
        );
    }

    #[test]
    fn revocation_between_admit_and_commit_refuses() {
        let mut authority = authority();
        let admission = authority.admit(&context("op:1"), need()).expect("admitted");
        authority.narrow(Layer::Consent, &[]);
        assert_eq!(
            authority.commit(&admission).map_err(|e| e.reason),
            Err("stale-policy-epoch")
        );
        let fresh = authority.admit(&context("op:1"), need());
        assert_eq!(fresh.map_err(|e| e.reason), Err("not-granted"));
    }

    #[test]
    fn halt_closes_admission_and_not_ready_refuses() {
        let mut authority = authority();
        authority.halt();
        assert_eq!(
            authority
                .admit(&context("op:1"), need())
                .map_err(|e| e.code),
            Err(ErrorCode::Cancelled)
        );
        let mut fresh = Authority::new(
            GrantLayers::default(),
            Selection::default(),
            context("op:1"),
            0,
        );
        assert_eq!(
            fresh.admit(&context("op:1"), need()).map_err(|e| e.reason),
            Err("not-ready")
        );
        fresh.initialize().expect("ready");
        assert_eq!(
            fresh.admit(&context("op:1"), need()).map_err(|e| e.reason),
            Err("not-granted")
        );
    }

    #[test]
    fn foreign_binding_refuses() {
        let mut authority = authority();
        assert_eq!(
            authority
                .admit(&context("op:2"), need())
                .map_err(|e| e.reason),
            Err("binding-mismatch")
        );
        assert!(authority.admit(&context("op:1"), need()).is_ok());
    }

    #[test]
    fn configuration_never_widens_consent() {
        let mut authority = authority();
        let wider = Capability::FileRead {
            path: path(),
            max_bytes: 1_000,
        };
        authority.configure(&[wider]);
        assert_eq!(authority.effective(), &[read_cap()]);
    }
}
