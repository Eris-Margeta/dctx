//! Restart reconciliation without process-id guessing.
//! Implements CACHE-v1 and DX-A2 Recover: an OS lock plus session identity prove the holder is alive.
//! Depends on writer locks and leases in [`super::Lifecycle`]. Startup passes this in on the first slice.
//! A boot-id change reclaims only staged objects that no live lock owns. Complete objects stay.

use super::ids::SessionId;
use super::lease::LeasePhase;
use super::Lifecycle;
use std::collections::BTreeSet;

/// Evidence from the host after a restart. There is no process-id field on purpose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestartEvidence {
    /// Identity of the current boot. A change accelerates incomplete unowned staging only.
    pub boot_id: u64,
    /// Sessions whose OS locks are still held. A reused session id without a lock is dead.
    pub held_sessions: BTreeSet<SessionId>,
}

impl Lifecycle {
    /// Applies lock and boot evidence. Unpublished temps of a dead writer are dropped.
    pub fn reconcile(&mut self, evidence: &RestartEvidence) {
        let boot_changed = evidence.boot_id != self.boot_id;
        if boot_changed {
            self.boot_id = evidence.boot_id;
            self.boot_accelerates = true;
        }
        let roots: Vec<super::RootId> = self.writers.keys().copied().collect();
        for root in roots {
            let session = self.writers.get(&root).and_then(|writer| writer.session);
            let held = session.is_some_and(|session| evidence.held_sessions.contains(&session));
            self.recover_writer(root, held);
        }
        let lease_ids: Vec<super::LeaseId> = self.leases.keys().copied().collect();
        for id in lease_ids {
            let Some(lease) = self.leases.get(&id) else {
                continue;
            };
            let open = matches!(
                lease.phase,
                LeasePhase::Active { .. } | LeasePhase::Acquiring { .. }
            );
            if open
                && !evidence.held_sessions.contains(&lease.session)
                && let Some(lease) = self.leases.get_mut(&id)
            {
                lease.phase = LeasePhase::OwnerProvenDead;
            }
        }
        if boot_changed {
            self.reclaim_abandoned_now();
            self.boot_accelerates = false;
        }
        self.bump();
        self.debug_check();
    }

    fn reclaim_abandoned_now(&mut self) {
        let doomed: Vec<super::ObjectId> = self
            .objects
            .keys()
            .copied()
            .filter(|id| self.staged_reclaimable(*id))
            .collect();
        for id in doomed {
            self.remove_object(id);
        }
    }
}
