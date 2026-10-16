//! Status report: caps, buckets, per-root attribution, leases and maintenance.
//! Implements CACHE-v1 `status` fields. Shared bytes are reported per root and counted once globally.
//! Depends on the ledger maps in [`super::Lifecycle`]. The facade prints this; it does not change it.
//! `history_reset` is set when pack retention retires a snapshot.

use super::ids::Physical;
use super::{Lifecycle, RootId};
use dctx_core::ids::Timestamp;
use std::collections::BTreeSet;

/// Why a sweep could not free a manifest or an object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Blocker {
    /// An explicit pin keeps the manifest.
    Pinned,
    /// An active reader lease keeps the generation.
    ActiveLease,
    /// Missing or offline grace is still running.
    UnavailableGrace,
    /// Unlink grace has not elapsed.
    UnlinkGrace,
    /// The last unlink failed. The bytes stay in the pending bucket.
    FailedUnlink,
    /// Another session holds the maintenance lock.
    MaintenanceBusy,
}

/// Disjoint byte buckets. `admitted = committed + staged + reserved + pending`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Buckets {
    /// Published and retired objects.
    pub committed: u64,
    /// Temp objects not yet swapped in.
    pub staged: u64,
    /// Granted reservations not yet materialised.
    pub reserved: u64,
    /// Objects waiting for unlink, including failed unlinks.
    pub pending: u64,
}

impl Buckets {
    /// `C + Sg + R + P`.
    pub fn admitted(self) -> u64 {
        self.committed
            .saturating_add(self.staged)
            .saturating_add(self.reserved)
            .saturating_add(self.pending)
    }
}

/// One root's explainable usage. Shared bytes are also counted in every other sharing root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootStatus {
    /// Registered id.
    pub id: RootId,
    /// Objects whose only root is this one.
    pub exclusive_bytes: u64,
    /// Objects this root shares with at least one other.
    pub shared_bytes: u64,
    /// Explicit pin.
    pub pinned: bool,
    /// A missing-root probe is outstanding.
    pub missing: bool,
    /// The last probe said the volume or permissions were unavailable.
    pub offline: bool,
    /// Registration TTL has fired. The record stays until it is compacted.
    pub expired: bool,
    /// First missing or offline observation. Repeated probes do not move it.
    pub unavailable_since: Option<Timestamp>,
    /// In-memory last use.
    pub last_use: Timestamp,
    /// A pack snapshot was evicted.
    pub history_reset: bool,
}

/// Point-in-time ledger report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    /// Configured hard cap.
    pub hard_cap: u64,
    /// Soft watermark.
    pub soft_watermark: u64,
    /// Eviction target.
    pub eviction_target: u64,
    /// Disjoint buckets.
    pub buckets: Buckets,
    /// Sum of the buckets.
    pub admitted: u64,
    /// Bytes in the retained closure.
    pub live_bytes: u64,
    /// Bytes no retained manifest or lease currently names.
    pub reclaimable_bytes: u64,
    /// Bytes kept by an explicit pin or an active lease.
    pub pinned_bytes: u64,
    /// Bytes whose manifest was revoked and that have not been unlinked.
    pub authorization_revoked_bytes: u64,
    /// Bytes marked by purge and still present because a lease holds them.
    pub deletion_requested_bytes: u64,
    /// Registered roots, including expired ones that are not compacted yet.
    pub roots: Vec<RootStatus>,
    /// Active reader pins.
    pub active_leases: usize,
    /// A backward clock sample was ignored.
    pub clock_fault: bool,
    /// Startup found the maintenance lock busy.
    pub deferred: bool,
    /// When the last slice finished a pass.
    pub last_maintain: Option<Timestamp>,
    /// When the next daily slice is due.
    pub next_maintain: Option<Timestamp>,
    /// A quarantined object is present.
    pub incompatible: bool,
    /// Pre-existing bytes sit above the cap, so admission is frozen.
    pub legacy_frozen: bool,
    /// Retirement blockers observed from the current maps.
    pub blockers: Vec<Blocker>,
}

impl Lifecycle {
    /// The four admission buckets.
    pub fn buckets(&self) -> Buckets {
        Buckets {
            committed: self.committed_bytes(),
            staged: self.staged_bytes(),
            reserved: self.reserved_bytes(),
            pending: self.pending_bytes(),
        }
    }

    /// True when legacy oversize has frozen growth.
    pub fn legacy_frozen(&self) -> bool {
        self.legacy_frozen
    }

    /// Status. This does not touch last-use or the cursor.
    pub fn status(&self) -> Status {
        let live = self.live_objects(true);
        let live_bytes = self.sum_iter(
            live.iter().filter_map(|id| self.objects.get(id).map(|object| object.bytes)),
        );
        let reclaimable_bytes = self.sum_iter(self.objects.iter().filter_map(|(id, object)| {
            if object.corrupt || live.contains(id) {
                None
            } else {
                Some(object.bytes)
            }
        }));
        let authorization_revoked_bytes = self.closure_bytes(|manifest| !manifest.available);
        let deletion_requested_bytes = self.sum_iter(
            self.objects
                .values()
                .filter(|object| object.deletion_requested)
                .map(|object| object.bytes),
        );
        let next_maintain = Some(match self.last_maintain {
            Some(stamp) => Timestamp(stamp.0.saturating_add(super::quota::DAY_MS)),
            None => self.wall,
        });
        let mut blockers: BTreeSet<Blocker> = self.collect_blockers().into_iter().collect();
        if self.deferred {
            blockers.insert(Blocker::MaintenanceBusy);
        }
        Status {
            hard_cap: self.quota.hard_cap,
            soft_watermark: self.quota.soft_watermark,
            eviction_target: self.quota.eviction_target,
            buckets: self.buckets(),
            admitted: self.admitted(),
            live_bytes,
            reclaimable_bytes,
            pinned_bytes: self.pinned_bytes(),
            authorization_revoked_bytes,
            deletion_requested_bytes,
            roots: self.roots.keys().copied().filter_map(|id| self.root_status(id)).collect(),
            active_leases: self.lease_active_count(),
            clock_fault: self.clock_fault,
            deferred: self.deferred,
            last_maintain: self.last_maintain,
            next_maintain,
            incompatible: self.incompatible,
            legacy_frozen: self.legacy_frozen,
            blockers: blockers.into_iter().collect(),
        }
    }

    fn root_status(&self, id: RootId) -> Option<RootStatus> {
        let record = self.roots.get(&id)?;
        let (exclusive_bytes, shared_bytes) = self.attribution(id);
        Some(RootStatus {
            id,
            exclusive_bytes,
            shared_bytes,
            pinned: record.pinned,
            missing: record.missing,
            offline: record.offline,
            expired: record.expired,
            unavailable_since: record.unavailable_since,
            last_use: record.last_use,
            history_reset: self.history_reset.contains(&id),
        })
    }

    fn attribution(&self, root: RootId) -> (u64, u64) {
        let mut exclusive = 0u64;
        let mut shared = 0u64;
        for object in self.objects.values() {
            if !object.roots.contains(&root) {
                continue;
            }
            if object.roots.len() == 1 {
                exclusive = exclusive.saturating_add(object.bytes);
            } else {
                shared = shared.saturating_add(object.bytes);
            }
        }
        (exclusive, shared)
    }

    fn closure_bytes(&self, include: impl Fn(&super::Manifest) -> bool) -> u64 {
        let mut stack = Vec::new();
        for manifest in self.manifests.values() {
            if include(manifest) {
                stack.push(manifest.object);
                stack.extend(manifest.refs.iter().copied());
            }
        }
        let mut seen = BTreeSet::new();
        let mut total = 0u64;
        while let Some(id) = stack.pop() {
            if !seen.insert(id) {
                continue;
            }
            if let Some(object) = self.objects.get(&id) {
                if object.state != Physical::PendingUnlink || object.bytes == object.born {
                    total = total.saturating_add(object.bytes);
                }
                stack.extend(object.refs.iter().copied());
            }
        }
        total
    }
}
