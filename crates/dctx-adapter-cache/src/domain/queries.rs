// SPDX-License-Identifier: Apache-2.0
//! Read-only ledger introspection: revision, clock, maintenance, cursors and TTLs.
//! Implements the CACHE-v1 status inputs over [`super::Lifecycle`]. The facade and tests only read state here.
//! Dry-runs do not move the revision and a rejected backward sample does not move the wall clock.
//! TTL helpers fall back to the product constants when no maintenance slice overrides them.

use super::ids::ObjectId;
use super::quota;
use super::Lifecycle;
use dctx_core::ids::Timestamp;
use std::collections::BTreeSet;

impl Lifecycle {
    /// Ledger revision. Dry-runs do not move it.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn logical_revision(&self) -> u64 {
        self.logical_revision
    }

    /// How many legacy apply removals have run.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn touches(&self) -> u64 {
        self.touches
    }

    /// Wall time the ledger is using. A rejected backward sample does not change this.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn wall(&self) -> Timestamp {
        self.wall
    }

    /// True after a backward wall sample.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn clock_fault(&self) -> bool {
        self.clock_fault
    }

    /// Boot identity last accepted by reconciliation.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn boot_id(&self) -> u64 {
        self.boot_id
    }

    /// True while a maintenance slice holds the non-blocking lock.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn maintenance_held(&self) -> bool {
        self.maintenance.is_some()
    }

    /// Cursor for the next startup sweep. Zero means the next pass starts at the first object.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn cursor(&self) -> u64 {
        self.cursor_mark
    }

    /// Objects currently in the ledger, in id order.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn object_ids(&self) -> BTreeSet<ObjectId> {
        self.objects.keys().copied().collect()
    }

    /// Wall time of the last auto-clean slice that ran. `None` until the first one.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn last_auto_clean(&self) -> Option<Timestamp> {
        self.last_auto_clean
    }

    pub(super) fn overlay_ttl(&self) -> u64 {
        self.clean_ttl.map_or(quota::OVERLAY_TTL_MS, |ttl| ttl.overlay)
    }

    pub(super) fn preview_ttl(&self) -> u64 {
        self.clean_ttl.map_or(quota::PREVIEW_TTL_MS, |ttl| ttl.preview)
    }

    pub(super) fn root_ttl(&self) -> u64 {
        self.clean_ttl.map_or(quota::ROOT_TTL_MS, |ttl| ttl.root)
    }

    pub(super) fn staging_ttl(&self) -> u64 {
        self.clean_ttl.map_or(quota::STAGING_TTL_MS, |ttl| ttl.staging)
    }

    pub(super) fn debug_check(&self) {
        debug_assert!(self.p4_holds(), "P4 admitted={} cap={}", self.admitted(), self.quota.hard_cap);
        debug_assert!(self.p3_holds(), "P3");
    }
}
