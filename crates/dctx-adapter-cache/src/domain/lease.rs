//! Reader pin handshake.
//! Implements the DX-A2 model: begin observes one pointer, confirm pins it only while that
//! generation is still retained and complete, release drops the pin.
//! Depends on [`super::Lifecycle`] pointers and manifests. GC treats only an active lease as a root.
//! An acquiring lease is not yet a pin, so a generation can disappear before confirm.

use super::ids::{GenerationId, LeaseId, Physical, SessionId};
use super::Lifecycle;
use crate::error::CacheError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LeasePhase {
    Acquiring { observed: GenerationId },
    Active { generation: GenerationId },
    Released,
    OwnerProvenDead,
}

#[derive(Debug, Clone)]
pub(super) struct Lease {
    pub root: super::RootId,
    pub session: SessionId,
    pub phase: LeasePhase,
}

impl Lifecycle {
    /// Observes the current pointer. The lease does not protect bytes until confirm.
    pub fn reader_begin(
        &mut self,
        root: super::RootId,
        session: SessionId,
    ) -> Result<LeaseId, CacheError> {
        if !self.roots.contains_key(&root) {
            return Err(CacheError::InvalidRequest { reason: "unknown root" });
        }
        let generation = self
            .pointers
            .get(&root)
            .and_then(|pointer| pointer.current)
            .ok_or(CacheError::InvalidRequest { reason: "no generation" })?;
        let id = LeaseId::new(self.ids.lease);
        self.ids.lease = self.ids.lease.checked_add(1).ok_or(CacheError::InvalidRequest {
            reason: "lease id space exhausted",
        })?;
        self.leases.insert(id, Lease { root, session, phase: LeasePhase::Acquiring { observed: generation } });
        self.bump();
        Ok(id)
    }

    /// Pins the observed generation when it is still complete. Otherwise the handshake aborts.
    pub fn reader_confirm(&mut self, id: LeaseId) -> Result<GenerationId, CacheError> {
        let Some(lease) = self.leases.get(&id) else {
            return Err(CacheError::InvalidRequest { reason: "unknown lease" });
        };
        let LeasePhase::Acquiring { observed } = lease.phase else {
            return Err(CacheError::InvalidRequest { reason: "lease is not acquiring" });
        };
        let root = lease.root;
        if !self.generation_confirmable(root, observed) {
            if let Some(lease) = self.leases.get_mut(&id) {
                lease.phase = LeasePhase::Released;
            }
            self.bump();
            return Err(CacheError::InvalidRequest { reason: "generation no longer retained" });
        }
        if let Some(lease) = self.leases.get_mut(&id) {
            lease.phase = LeasePhase::Active { generation: observed };
        }
        self.bump();
        Ok(observed)
    }

    /// Ends an active pin. Bytes can be reclaimed on a later sweep once nothing else retains them.
    pub fn reader_release(&mut self, id: LeaseId) -> Result<(), CacheError> {
        let Some(lease) = self.leases.get_mut(&id) else {
            return Err(CacheError::InvalidRequest { reason: "unknown lease" });
        };
        lease.phase = LeasePhase::Released;
        self.bump();
        Ok(())
    }

    pub(super) fn generation_confirmable(&self, root: super::RootId, generation: GenerationId) -> bool {
        self.manifests.values().any(|manifest| {
            if manifest.root != root || manifest.generation != generation || !self.retained(manifest, false)
            {
                return false;
            }
            self.objects.get(&manifest.object).is_some_and(|object| {
                object.validated && object.complete && !object.corrupt && object.state != Physical::PendingUnlink
            })
        })
    }

    pub(super) fn root_has_open_lease(&self, root: super::RootId) -> bool {
        self.leases.values().any(|lease| {
            lease.root == root
                && matches!(lease.phase, LeasePhase::Acquiring { .. } | LeasePhase::Active { .. })
        })
    }

    pub(super) fn lease_holds(&self, generation: GenerationId) -> bool {
        self.leases.values().any(|lease| {
            matches!(lease.phase, LeasePhase::Active { generation: held } if held == generation)
        })
    }

    pub(super) fn lease_active_count(&self) -> usize {
        self.leases.values().filter(|lease| matches!(lease.phase, LeasePhase::Active { .. })).count()
    }
}
