// SPDX-License-Identifier: Apache-2.0
//! Quota, admission, compaction and disk-full tests for the cache lifecycle.
//! Implements CACHE-v1 accounting: one global cap, root ceilings inside it, and no double counting.
//! Depends on the public ledger API. The property names P4 live in `p4_global_reservations`.

#[path = "support/mod.rs"]
mod support;

use dctx_adapter_cache::{
    CacheError, CompactOutcome, Lifecycle, Physical, Purpose, Quota, RootId, EMERGENCY_METADATA_BYTES,
    HARD_CAP_BYTES,
};
use dctx_core::budget::Reservation;
use dctx_core::error::DctxError;
use dctx_ports::Cache;

fn root() -> RootId {
    RootId::new(1)
}

fn ledger(hard: u64) -> Lifecycle {
    let mut life = Lifecycle::new(Quota::scaled(hard)).expect("quota");
    life.register_root(root()).expect("root");
    life
}

#[test]
fn p4_global_reservations() {
    let mut life = Lifecycle::new(Quota::scaled(10)).expect("quota");
    let first_root = RootId::new(1);
    let second_root = RootId::new(2);
    life.register_root(first_root).expect("root");
    life.register_root(second_root).expect("root");
    let first = life.reserve_for(Some(first_root), Purpose::View, 6).expect("reserve");
    match life.reserve_for(Some(second_root), Purpose::View, 5) {
        Err(CacheError::CacheLimit { requested, available, .. }) => {
            assert_eq!(requested, 5);
            assert_eq!(available, 4);
        }
        other => panic!("expected cache limit, got {other:?}"),
    }
    let fitting = life.reserve_for(Some(second_root), Purpose::Fact, 4).expect("room for 4");
    life.abort_reservation(fitting);
    life.abort_reservation(first);
    let again = life.reserve_for(Some(second_root), Purpose::Pack, 5).expect("cap is free");
    life.abort_reservation(again);
    assert_eq!(life.admitted(), 0);
    assert!(life.p4_holds());

    let mut legacy = Lifecycle::new(Quota::scaled(10)).expect("quota");
    let small = legacy.observe_legacy_wal(4).expect("fits");
    assert!(!legacy.legacy_frozen());
    legacy.reclaim_legacy(small).expect("reclaim");
    let big = legacy.observe_legacy_wal(11).expect("oversize");
    assert!(legacy.legacy_frozen());
    assert!(matches!(
        legacy.reserve_for(None, Purpose::Payload, 1),
        Err(CacheError::CacheLimit { available: 0, .. })
    ));
    legacy.reclaim_legacy(big).expect("reclaim");
    assert!(!legacy.legacy_frozen());
    legacy.reserve_for(None, Purpose::Payload, 4).expect("growth resumes");
    assert!(legacy.p4_holds());

    let mut product = Lifecycle::product_default();
    let payload = HARD_CAP_BYTES - EMERGENCY_METADATA_BYTES;
    let hold = product.reserve(payload).expect("payload ceiling");
    assert!(matches!(
        product.reserve(1),
        Err(DctxError::CacheLimit { available: 0, .. })
    ));
    product.abort(hold);
    assert_eq!(product.admitted(), 0);
    assert!(product.p4_holds());
}

#[test]
fn commit_partial_releases_the_unused_reservation() {
    let mut life = Lifecycle::new(Quota::scaled(20)).expect("quota");
    let reservation = life.reserve_for(None, Purpose::Payload, 6).expect("reserve");
    let id = reservation.id();
    let bytes = reservation.bytes();
    assert!(life.commit_reservation(reservation, 7).is_err());
    assert_eq!(life.buckets().reserved, 6);
    life.abort_reservation(Reservation::new(id, bytes));
    assert_eq!(life.buckets().reserved, 0);
    let reservation = life.reserve(6).expect("reserve");
    life.commit(reservation, 4).expect("commit");
    assert_eq!(life.admitted(), 4);
    assert_eq!(life.buckets().reserved, 0);
    assert!(life.commit(Reservation::new(1, 6), 1).is_err());
    assert!(life.p4_holds());
}

#[test]
fn stage_consumes_the_reservation_without_double_counting() {
    let mut life = ledger(100);
    life.writer_acquire(root(), dctx_adapter_cache::SessionId::new(1)).expect("acquire");
    life.writer_reserve(root(), 10).expect("reserve");
    assert_eq!(life.buckets().reserved, 10);
    assert_eq!(life.admitted(), 10);
    life.writer_stage_segment(root(), 4).expect("stage");
    assert_eq!(life.buckets().staged, 4);
    assert_eq!(life.buckets().reserved, 6);
    assert_eq!(life.admitted(), 10);
    assert!(life.p4_holds());
}

#[test]
fn lowering_the_cap_freezes_until_bytes_fit() {
    let mut life = ledger(100);
    let fact = life.insert_fact(80, &[root()]).expect("fact");
    life.set_hard_cap(90).expect("cap");
    assert!(!life.legacy_frozen());
    let hold = life.reserve_for(None, Purpose::Payload, 10).expect("still fits");
    assert!(life.set_hard_cap(70).is_err());
    assert!(!life.legacy_frozen());
    life.abort_reservation(hold);
    life.set_hard_cap(50).expect("lower");
    assert!(life.legacy_frozen());
    assert!(matches!(
        life.reserve_for(None, Purpose::Payload, 1),
        Err(CacheError::CacheLimit { available: 0, .. })
    ));
    assert!(life.reclaim_legacy(fact).is_err());
    support::drain(&mut life, true);
    assert!(!life.legacy_frozen());
    life.reserve_for(None, Purpose::Payload, 1).expect("growth resumes");
    assert!(life.p4_holds());
}

#[test]
fn root_ceilings_sit_inside_the_global_cap() {
    let quota = Quota::scaled(1_000).with_ceilings(100, 50, 30);
    let mut life = Lifecycle::new(quota).expect("quota");
    life.register_root(root()).expect("root");
    let other = RootId::new(2);
    life.register_root(other).expect("root");
    life.publish_view(root(), 80, &[]).expect("view");
    assert!(life.publish_view(root(), 30, &[]).is_err());
    assert_eq!(life.admitted(), 80);
    life.insert_fact(200, &[root()]).expect("facts use the global pool");
    assert_eq!(life.admitted(), 280);
    life.publish_pack(root(), 40, &[]).expect("pack");
    assert!(life.publish_pack(root(), 20, &[]).is_err());
    life.publish_preview(root(), 30, &[]).expect("preview");
    assert!(life.publish_preview(root(), 1, &[]).is_err());
    let shared = life.insert_fact(50, &[root(), other]).expect("shared");
    assert_eq!(life.object_bytes(shared), Some(50));
    assert_eq!(life.admitted(), 280 + 40 + 30 + 50);
    let status = life.status();
    let row = status.roots.iter().find(|row| row.id == root()).expect("root status");
    let other_row = status.roots.iter().find(|row| row.id == other).expect("other");
    assert_eq!(row.shared_bytes, 50);
    assert_eq!(other_row.shared_bytes, 50);
    assert_eq!(other_row.exclusive_bytes, 0);
    assert!(life.p4_holds());
}

#[test]
fn compaction_refuses_without_headroom_and_counts_the_peak() {
    let mut tight = ledger(100);
    let segment = tight.insert_segment(80, 500_000, &[root()]).expect("segment");
    assert!(matches!(tight.compact_segment(segment), Err(CacheError::CacheLimit { .. })));
    assert_eq!(tight.object_bytes(segment), Some(80));
    assert_eq!(tight.admitted(), 80);
    assert!(tight.p4_holds());

    let mut life = ledger(100);
    let segment = life.insert_segment(40, 500_000, &[root()]).expect("segment");
    let outcome = life.compact_segment(segment).expect("headroom");
    assert_eq!(outcome.old_bytes, 40);
    assert_eq!(outcome.peak_admitted, 60);
    assert!(!outcome.old_retained);
    assert!(life.object_bytes(segment).is_none());
    assert_eq!(life.object_bytes(outcome.new_segment), Some(20));
    assert!(outcome.peak_admitted <= 100);
    assert!(life.p3_holds() && life.p4_holds());

    let mut pinned = ledger(100);
    let segment = pinned.insert_segment(40, 500_000, &[root()]).expect("segment");
    let generation = pinned.publish_view(root(), 10, &[segment]).expect("view");
    let lease = pinned.reader_begin(root(), dctx_adapter_cache::SessionId::new(2)).expect("begin");
    pinned.reader_confirm(lease).expect("confirm");
    let CompactOutcome { old_retained, peak_admitted, new_segment, old_bytes } =
        pinned.compact_segment(segment).expect("leased copy");
    assert!(old_retained);
    assert_eq!(old_bytes, 40);
    assert_eq!(pinned.object_bytes(segment), Some(40));
    assert_eq!(pinned.object_bytes(new_segment), Some(20));
    assert!(peak_admitted >= 60);
    assert!(peak_admitted <= 100);
    assert_eq!(pinned.current_generation(root()), Some(generation));
    assert!(pinned.p3_holds());
}

#[test]
fn failed_unlink_stays_in_the_pending_bucket() {
    let mut life = Lifecycle::new(Quota::scaled(10_000)).expect("quota");
    life.register_root(root()).expect("root");
    let fact = life.insert_fact(30, &[root()]).expect("fact");
    life.publish_view(root(), 5, &[fact]).expect("view");
    life.publish_view(root(), 5, &[]).expect("view");
    life.publish_view(root(), 5, &[]).expect("view");
    life.publish_view(root(), 5, &[]).expect("view");
    support::gc(&mut life, true);
    assert_eq!(life.object_state(fact), Some(Physical::PendingUnlink));
    life.fail_unlink(fact);
    assert!(life.buckets().pending >= 30);
    support::gc(&mut life, true);
    assert_eq!(life.object_bytes(fact), Some(30));
    assert!(life.buckets().pending >= 30);
    life.clear_unlink_fault(fact);
    support::drain(&mut life, true);
    assert!(life.object_bytes(fact).is_none());
    assert_eq!(life.buckets().pending, 0);
    assert!(life.p4_holds());
}

#[test]
fn disk_full_is_not_a_cache_limit() {
    let mut life = ledger(100);
    let before = life.admitted();
    life.inject_disk_full(true);
    let err = life.publish_view(root(), 5, &[]).expect_err("full");
    assert!(matches!(err, CacheError::DiskFull));
    assert_eq!(err.into_dctx(), DctxError::io("disk full"));
    assert_eq!(life.admitted(), before);
    life.inject_disk_full(false);
    life.publish_view(root(), 5, &[]).expect("space");
    assert!(life.admitted() > before);

    let mut writer = ledger(100);
    writer.writer_acquire(root(), dctx_adapter_cache::SessionId::new(3)).expect("acquire");
    writer.writer_reserve(root(), 10).expect("reserve");
    writer.inject_disk_full(true);
    assert!(matches!(writer.writer_stage_segment(root(), 10), Err(CacheError::DiskFull)));
    assert_eq!(writer.buckets().reserved, 10);
    assert_eq!(writer.buckets().staged, 0);
    assert_eq!(writer.admitted(), 10);
    writer.inject_disk_full(false);
    writer.writer_stage_segment(root(), 10).expect("stage");
    assert_eq!(writer.buckets().staged, 10);
    assert_eq!(writer.buckets().reserved, 0);
    assert_eq!(writer.admitted(), 10);
}

#[test]
fn segments_above_a_quarter_dead_are_compaction_candidates() {
    let mut life = ledger(1_000);
    let quiet = life.insert_segment(100, 250_000, &[root()]).expect("segment");
    let dirty = life.insert_segment(100, 250_001, &[root()]).expect("segment");
    assert!(!life.compaction_candidate(quiet));
    assert!(life.compaction_candidate(dirty));
    let _pressure = life.insert_segment(800, 0, &[root()]).expect("pressure");
    assert!(life.admitted() >= life.status().soft_watermark);
    assert!(life.compaction_candidate(quiet));
}
