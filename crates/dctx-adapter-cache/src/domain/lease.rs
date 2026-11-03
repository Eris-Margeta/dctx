// SPDX-License-Identifier: Apache-2.0
//! Reader pin handshake.
//! Implements the DX-A2 model: begin observes one pointer, confirm pins it only while that
//! generation is still retained and complete, release drops the pin.
//! The last release finishes a purge that was waiting on the lease.
//! Depends on [`super::Lifecycle`] pointers and manifests. GC treats only an active lease as a root.
//! An acquiring lease is not yet a pin, so a generation can disappear before confirm.

use super::ids::{GenerationId, LeaseId, Physical, SessionId};
use super::restart::{Put, Take};
use super::Lifecycle;
use crate::error::CacheError;
use std::collections::BTreeMap;

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
    /// # Errors
    /// Refuses unknown roots, unavailable generations, or exhausted lease IDs.
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
    /// # Errors
    /// Refuses unknown/nonacquiring leases or generations no longer retained.
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

    /// Ends an active pin. A purge marked on this generation drops logical retention
    /// when no other lease still holds it. Physical bytes wait for a later sweep.
    /// # Errors
    /// Refuses an unknown lease ID.
    pub fn reader_release(&mut self, id: LeaseId) -> Result<(), CacheError> {
        let Some(lease) = self.leases.get(&id) else {
            return Err(CacheError::InvalidRequest { reason: "unknown lease" });
        };
        let generation = match lease.phase {
            LeasePhase::Active { generation } => Some(generation),
            LeasePhase::Acquiring { .. } | LeasePhase::Released | LeasePhase::OwnerProvenDead => None,
        };
        if let Some(lease) = self.leases.get_mut(&id) {
            lease.phase = LeasePhase::Released;
        }
        if let Some(generation) = generation
            && !self.lease_holds(generation)
        {
            self.finish_requested_deletion(generation);
            self.retire_superseded_generation(generation);
        }
        self.bump();
        self.debug_check();
        Ok(())
    }

    /// Leases that still protect a generation: confirmed pins and handshakes in flight.
    #[allow(clippy::must_use_candidate, reason = "Read-only list for the adapter's liveness probe.")]
    pub fn open_leases(&self) -> Vec<LeaseId> {
        self.leases
            .iter()
            .filter(|(_, lease)| matches!(lease.phase, LeasePhase::Active { .. } | LeasePhase::Acquiring { .. }))
            .map(|(id, _)| *id)
            .collect()
    }

    /// The adapter proved the process that took this lease is gone. The lease stops protecting its
    /// generation, and a superseded view it held becomes reclaimable.
    /// # Errors
    /// Refuses an unknown lease ID.
    pub fn reader_owner_dead(&mut self, id: LeaseId) -> Result<(), CacheError> {
        let Some(lease) = self.leases.get_mut(&id) else {
            return Err(CacheError::InvalidRequest { reason: "unknown lease" });
        };
        let held = match lease.phase {
            LeasePhase::Active { generation } => Some(generation),
            LeasePhase::Acquiring { .. } | LeasePhase::Released | LeasePhase::OwnerProvenDead => None,
        };
        lease.phase = LeasePhase::OwnerProvenDead;
        if let Some(generation) = held
            && !self.lease_holds(generation)
        {
            self.finish_requested_deletion(generation);
            self.retire_superseded_generation(generation);
        }
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

pub(super) struct Holds {
    pub leases: BTreeMap<LeaseId, Lease>,
    pub reservations: BTreeMap<u64, super::Hold>,
}

pub(super) fn write_holds(life: &Lifecycle, out: &mut Put) -> Result<(), CacheError> {
    out.count(life.leases.len())?;
    for (id, lease) in &life.leases {
        out.u64(id.raw());
        out.u64(lease.root.raw());
        out.u64(lease.session.raw());
        let (tag, generation) = match lease.phase {
            LeasePhase::Acquiring { observed } => (1u8, observed.raw()),
            LeasePhase::Active { generation } => (2, generation.raw()),
            LeasePhase::Released => (3, 0),
            LeasePhase::OwnerProvenDead => (4, 0),
        };
        out.u8(tag);
        out.u64(generation);
    }
    out.count(life.reservations.len())?;
    for (id, hold) in &life.reservations {
        out.u64(*id);
        out.u64(hold.left);
        out.u64(hold.original);
        out.opt_u64(hold.root.map(super::RootId::raw));
        out.u8(hold.class.tag());
        out.u8(hold.purpose.tag());
    }
    Ok(())
}

pub(super) fn read_holds(inp: &mut Take<'_>) -> Result<Holds, CacheError> {
    let mut leases = BTreeMap::new();
    for _ in 0..inp.count()? {
        let id = LeaseId::new(inp.u64()?);
        let root = super::RootId::new(inp.u64()?);
        let session = SessionId::new(inp.u64()?);
        let tag = inp.u8()?;
        let generation = inp.u64()?;
        let phase = match tag {
            1 => LeasePhase::Acquiring { observed: GenerationId::new(generation) },
            2 => LeasePhase::Active { generation: GenerationId::new(generation) },
            3 => LeasePhase::Released,
            4 => LeasePhase::OwnerProvenDead,
            _ => return Err(super::restart::corrupt("lease")),
        };
        if leases.insert(id, Lease { root, session, phase }).is_some() {
            return Err(super::restart::corrupt("duplicate"));
        }
    }
    let mut reservations = BTreeMap::new();
    for _ in 0..inp.count()? {
        let id = inp.u64()?;
        let left = inp.u64()?;
        let original = inp.u64()?;
        let root = inp.opt_u64()?.map(super::RootId::new);
        let class = super::ids::ChargeClass::from_tag(inp.u8()?).ok_or_else(|| {
            super::restart::corrupt("reservation")
        })?;
        let purpose = super::ids::Purpose::from_tag(inp.u8()?).ok_or_else(|| {
            super::restart::corrupt("reservation")
        })?;
        if reservations.insert(id, super::Hold { left, original, root, class, purpose }).is_some() {
            return Err(super::restart::corrupt("duplicate"));
        }
    }
    Ok(Holds { leases, reservations })
}
