// SPDX-License-Identifier: Apache-2.0
//! Fake-clock expiry: overlays, root registration, missing grace, pins and last-use.
//! Implements CACHE-v1 TTLs. One day is `86_400_000` ms. A backward step does not expire state.
//! Depends on [`FakeClock`] and explicit `gc`. Startup cadence is covered beside these clocks.

#[path = "support/mod.rs"]
mod support;

use dctx_adapter_cache::{
    Blocker, CacheError, FakeClock, GcRequest, Lifecycle, Physical, Purpose, Quota, RootId,
    SessionId, SliceBudget, DAY_MS, LAST_USE_COALESCE_MS, MISSING_GRACE_MS, OVERLAY_TTL_MS,
    ROOT_TTL_MS,
};
use dctx_ports::memory::CancelAfter;

fn ledger(hard: u64) -> Lifecycle {
    Lifecycle::new(Quota::scaled(hard)).expect("quota")
}

fn state(life: &Lifecycle, root: RootId, generation: dctx_adapter_cache::GenerationId) -> Option<Physical> {
    life.manifest_of(root, generation).and_then(|id| life.manifest_state(id))
}

#[test]
fn idle_overlay_expires_at_seven_days_and_the_root_at_thirty() {
    let mut life = ledger(10_000);
    let root = RootId::new(1);
    life.register_root(root).expect("root");
    let view = life.publish_view(root, 8, &[]).expect("view");
    let overlay = life.publish_overlay(root, 8, &[]).expect("overlay");
    let first = life.publish_preview(root, 4, &[]).expect("preview");
    let second = life.publish_preview(root, 4, &[]).expect("preview");
    let third = life.publish_preview(root, 4, &[]).expect("preview");
    assert_eq!(state(&life, root, first), Some(Physical::Retired));
    assert_eq!(state(&life, root, second), Some(Physical::Published));
    assert_eq!(state(&life, root, third), Some(Physical::Published));
    let mut clock = FakeClock::at(0);
    clock.advance(OVERLAY_TTL_MS - 1);
    life.follow(&clock);
    support::gc(&mut life, false);
    assert_eq!(state(&life, root, overlay), Some(Physical::Published));
    assert_eq!(state(&life, root, view), Some(Physical::Published));
    clock.advance(1);
    life.follow(&clock);
    support::gc(&mut life, false);
    assert_eq!(state(&life, root, overlay), Some(Physical::Retired));
    assert_eq!(life.current_generation(root), Some(view));

    clock.advance(ROOT_TTL_MS - OVERLAY_TTL_MS - 1);
    life.follow(&clock);
    support::gc(&mut life, false);
    assert!(!life.root_expired(root));
    assert_eq!(life.current_generation(root), Some(view));
    clock.advance(1);
    life.follow(&clock);
    support::gc(&mut life, false);
    assert!(life.root_expired(root));
    assert!(life.current_generation(root).is_none());
    assert!(life.status().roots.iter().any(|row| row.id == root));
    assert!(life.p3_holds());
}

#[test]
fn backward_clock_does_not_expire_an_overlay() {
    let mut clock = FakeClock::at(OVERLAY_TTL_MS);
    let mut life = ledger(1_000);
    life.follow(&clock);
    let root = RootId::new(1);
    life.register_root(root).expect("root");
    let overlay = life.publish_overlay(root, 8, &[]).expect("overlay");
    clock.rewind_wall(OVERLAY_TTL_MS);
    life.follow(&clock);
    assert!(life.clock_fault());
    assert_eq!(life.wall().0, OVERLAY_TTL_MS);
    clock.advance(OVERLAY_TTL_MS - 1);
    life.follow(&clock);
    support::gc(&mut life, false);
    assert_eq!(state(&life, root, overlay), Some(Physical::Published));
    clock.advance(1);
    life.follow(&clock);
    clock.advance(OVERLAY_TTL_MS);
    life.follow(&clock);
    support::gc(&mut life, false);
    assert_eq!(state(&life, root, overlay), Some(Physical::Retired));
}

#[test]
fn missing_root_grace_blocks_eviction_until_it_elapses() {
    let mut open = ledger(80);
    let root = RootId::new(1);
    open.register_root(root).expect("root");
    for _ in 0..4 {
        open.publish_view(root, 20, &[]).expect("view");
    }
    assert_eq!(open.admitted(), 80);
    support::drain(&mut open, true);
    assert!(open.admitted() < 80);
    open.reserve_for(None, Purpose::Payload, 1).expect("reclaimed room");

    let mut life = ledger(80);
    life.register_root(root).expect("root");
    life.note_missing(root).expect("missing");
    let since = life.unavailable_since(root);
    for _ in 0..4 {
        life.publish_view(root, 20, &[]).expect("view");
    }
    assert_eq!(life.admitted(), 80);
    assert!(matches!(
        life.reserve_for(None, Purpose::Payload, 1),
        Err(CacheError::CacheLimit { available: 0, .. })
    ));
    let mut clock = FakeClock::at(0);
    clock.advance(MISSING_GRACE_MS - 1);
    life.follow(&clock);
    life.note_missing(root).expect("probe");
    life.note_offline(root).expect("offline");
    assert_eq!(life.unavailable_since(root), since);
    assert!(life.status().roots[0].offline);
    assert!(life.status().roots[0].missing);
    support::drain(&mut life, true);
    assert_eq!(life.admitted(), 80);
    assert!(life.reserve_for(None, Purpose::Payload, 1).is_err());
    clock.advance(1);
    life.follow(&clock);
    support::drain(&mut life, true);
    assert!(life.admitted() < 80);
    life.reserve_for(None, Purpose::Payload, 1).expect("grace ended");
    assert!(life.p4_holds());
}

#[test]
fn explicit_pin_blocks_expiry_but_not_the_hard_cap() {
    let mut life = ledger(50);
    let root = RootId::new(1);
    life.register_root(root).expect("root");
    life.pin_root(root, true).expect("pin");
    let generation = life.publish_view(root, 50, &[]).expect("view");
    let mut clock = FakeClock::at(0);
    clock.advance(ROOT_TTL_MS);
    life.follow(&clock);
    support::gc(&mut life, true);
    assert_eq!(life.current_generation(root), Some(generation));
    assert!(!life.root_expired(root));
    assert!(matches!(
        life.reserve_for(None, Purpose::Payload, 1),
        Err(CacheError::CacheLimit { .. })
    ));
    life.pin_root(root, false).expect("unpin");
    support::drain(&mut life, true);
    assert!(life.current_generation(root).is_none());
    assert!(life.admitted() < 50);
    life.reserve_for(None, Purpose::Payload, 1).expect("unpinned expiry");
    assert!(life.p3_holds());
}

#[test]
fn last_use_persists_at_most_once_per_five_minutes() {
    let mut clock = FakeClock::at(0);
    let mut life = ledger(200);
    life.follow(&clock);
    let left = RootId::new(1);
    let right = RootId::new(2);
    life.register_root(left).expect("root");
    life.register_root(right).expect("root");
    assert_eq!(life.persist_writes(left), Some(1));
    life.touch(left).expect("touch");
    assert_eq!(life.persist_writes(left), Some(1));
    assert_eq!(life.last_use(left).expect("use").0, 0);
    clock.advance(LAST_USE_COALESCE_MS - 1);
    life.follow(&clock);
    life.touch(left).expect("touch");
    assert_eq!(life.persist_writes(left), Some(1));
    assert_eq!(life.last_use(left).expect("use").0, LAST_USE_COALESCE_MS - 1);
    clock.advance(1);
    life.follow(&clock);
    life.touch(left).expect("touch");
    assert_eq!(life.persist_writes(left), Some(2));

    // Overlays: a superseded view is retired at publish (P-C10), so LRU pressure is shown on overlays.
    let mut pressured = ledger(200);
    pressured.register_root(left).expect("root");
    pressured.register_root(right).expect("root");
    let older = pressured.publish_overlay(left, 40, &[]).expect("view");
    let other = pressured.publish_overlay(right, 40, &[]).expect("view");
    pressured.publish_overlay(left, 40, &[]).expect("view");
    pressured.publish_overlay(right, 40, &[]).expect("view");
    let once = GcRequest {
        root: None,
        budget: SliceBudget { max_objects: 1, max_millis: u64::MAX, millis_per_object: 0 },
        bypass_grace: false,
        max_bytes: None,
        cancel: None,
    };
    pressured.gc(once).expect("gc");
    assert_eq!(state(&pressured, left, older), Some(Physical::Retired));
    assert_eq!(state(&pressured, right, other), Some(Physical::Published));

    let mut touched = ledger(200);
    touched.register_root(left).expect("root");
    touched.register_root(right).expect("root");
    let older = touched.publish_overlay(left, 40, &[]).expect("view");
    let other = touched.publish_overlay(right, 40, &[]).expect("view");
    touched.publish_overlay(left, 40, &[]).expect("view");
    touched.publish_overlay(right, 40, &[]).expect("view");
    let mut clock = FakeClock::at(0);
    clock.advance(1);
    touched.follow(&clock);
    touched.touch(left).expect("touch");
    touched.gc(once).expect("gc");
    assert_eq!(state(&touched, left, older), Some(Physical::Published));
    assert_eq!(state(&touched, right, other), Some(Physical::Retired));
}

#[test]
fn startup_is_idle_until_the_next_day_or_soft_pressure() {
    let mut life = ledger(100);
    let mut clock = FakeClock::at(1_000);
    let first = life.startup(clock.sample(), None).expect("startup");
    assert!(first.ran);
    assert!(first.completed_pass);
    let revision = life.logical_revision();
    let second = life.startup(clock.sample(), None).expect("startup");
    assert!(!second.ran);
    assert_eq!(life.logical_revision(), revision);
    life.register_root(RootId::new(1)).expect("root");
    life.insert_fact(80, &[RootId::new(1)]).expect("pressure");
    let pressured = life.startup(clock.sample(), None).expect("startup");
    assert!(pressured.ran);
    clock.advance(DAY_MS);
    let next_day = life.startup(clock.sample(), None).expect("startup");
    assert!(next_day.ran);
}

#[test]
fn startup_defers_when_maintenance_is_busy() {
    let mut life = Lifecycle::product_default();
    let session = SessionId::new(3);
    life.hold_maintenance(session).expect("lock");
    let report = life.startup(FakeClock::at(0).sample(), None).expect("startup");
    assert!(report.deferred);
    assert!(!report.ran);
    assert!(life.status().blockers.contains(&Blocker::MaintenanceBusy));
    assert!(matches!(
        life.gc(GcRequest {
            root: None,
            budget: SliceBudget::explicit(4),
            bypass_grace: false,
            max_bytes: None,
            cancel: None,
        }),
        Err(CacheError::Busy)
    ));
    life.release_maintenance(session).expect("release");
    let report = life.startup(FakeClock::at(0).sample(), None).expect("startup");
    assert!(report.ran);
    assert!(!life.maintenance_held());
}

#[test]
fn cancelled_gc_leaves_admission_unchanged() {
    let mut life = ledger(100);
    let root = RootId::new(1);
    life.register_root(root).expect("root");
    let fact = life.insert_fact(10, &[root]).expect("fact");
    let before = life.admitted();
    let revision = life.logical_revision();
    let cancel = CancelAfter::new(0);
    let request = GcRequest {
        root: None,
        budget: SliceBudget::explicit(32),
        bypass_grace: true,
        max_bytes: None,
        cancel: Some(&cancel),
    };
    assert!(matches!(life.gc(request), Err(CacheError::Cancelled)));
    assert_eq!(life.admitted(), before);
    assert_eq!(life.object_bytes(fact), Some(10));
    assert_eq!(life.logical_revision(), revision);
    let cancel = CancelAfter::new(8);
    life.gc(GcRequest { cancel: Some(&cancel), ..request }).expect("gc");
    assert!(life.p4_holds());
}
