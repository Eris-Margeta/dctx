// SPDX-License-Identifier: Apache-2.0
//! Deterministic hostile ledger, lifecycle, and filesystem adapter regressions.
//! Sizes are logical byte counts unless a codec or filesystem boundary requires bytes.
#![forbid(unsafe_code)]

use dctx_adapter_cache::adapters::{Cache, FsLedgerStore, FsObjectStore};
use dctx_adapter_cache::ports::{LedgerStore, ObjectStore};
use dctx_adapter_cache::{
    AutoCleanPolicy, CacheError, ExportEvidence, FakeClock, GcRequest, KnownExport,
    LEDGER_MAX_BYTES, Lifecycle, OutputPlace, Physical, PurgeRequest, PurgeScope, Quota, RootId,
    SessionId, SliceBudget,
};
use dctx_hash::Sha256;
use std::{collections::BTreeSet, io::Read, path::Path};

fn life() -> Lifecycle {
    Lifecycle::new(Quota::scaled(4096)).unwrap()
}
fn export(label: String) -> KnownExport {
    KnownExport {
        place: OutputPlace::Registry,
        label,
        bytes: 1,
        evidence: ExportEvidence::OwnedText { identity: 7 },
    }
}
fn resign(bytes: &mut [u8]) {
    let end = bytes.len() - 32;
    let sum = Sha256::digest(&bytes[..end]).unwrap();
    bytes[end..].copy_from_slice(sum.as_bytes());
}
fn frame(body: &[u8]) -> Vec<u8> {
    let mut bytes = b"DCTXLEDG\x01\0\0\0".to_vec();
    bytes.extend_from_slice(&u32::try_from(body.len()).unwrap().to_le_bytes());
    bytes.extend_from_slice(body);
    bytes.extend_from_slice(&[0; 32]);
    resign(&mut bytes);
    bytes
}
fn canonical(bytes: &[u8]) {
    if let Ok(restored) = Lifecycle::decode_ledger(bytes) {
        assert!(restored.p3_holds() && restored.p4_holds());
        let encoded = restored.encode_ledger().unwrap();
        assert_eq!(
            Lifecycle::decode_ledger(&encoded)
                .unwrap()
                .encode_ledger()
                .unwrap(),
            encoded
        );
    }
}

#[test]
fn ledger_empty_one_byte_every_prefix_and_single_byte_mutations_do_not_panic() {
    let mut life = life();
    let root = RootId::new(0);
    life.register_root(root).unwrap();
    let object = life.insert_fact(1, &[root, root]).unwrap();
    life.publish_view(root, 1, &[object, object]).unwrap();
    let lease = life.reader_begin(root, SessionId::new(u64::MAX)).unwrap();
    life.reader_confirm(lease).unwrap();
    life.adopt_legacy(&export("é".into())).unwrap();
    let valid = life.encode_ledger().unwrap();
    assert_eq!(life.encode_ledger().unwrap(), valid);
    assert_eq!(
        Lifecycle::decode_ledger(&valid)
            .unwrap()
            .encode_ledger()
            .unwrap(),
        valid
    );
    for cut in 0..valid.len() {
        assert!(
            Lifecycle::decode_ledger(&valid[..cut]).is_err(),
            "cut {cut}"
        );
        if cut >= 16 && cut < valid.len() - 32 {
            canonical(&frame(&valid[16..cut]));
        }
    }
    for at in 16..valid.len() - 32 {
        for value in [0, 1, 0xff] {
            let mut bytes = valid.clone();
            bytes[at] = value;
            resign(&mut bytes);
            canonical(&bytes);
        }
    }
    for value in 0..=255 {
        assert!(Lifecycle::decode_ledger(&[value]).is_err());
    }
}

#[test]
fn ledger_size_count_and_utf8_limits_are_enforced() {
    let valid = life().encode_ledger().unwrap();
    for size in [LEDGER_MAX_BYTES, LEDGER_MAX_BYTES + 1] {
        let mut body = valid[16..valid.len() - 32].to_vec();
        body.resize(size - 48, 0);
        assert!(Lifecycle::decode_ledger(&frame(&body)).is_err());
    }
    let mut life = life();
    life.adopt_legacy(&export("label".into())).unwrap();
    let mut bytes = life.encode_ledger().unwrap();
    let offset = bytes.windows(5).position(|part| part == b"label").unwrap();
    bytes[offset] = 0xff;
    resign(&mut bytes);
    assert!(matches!(
        Lifecycle::decode_ledger(&bytes),
        Err(CacheError::CorruptLedger { reason: "text" })
    ));
    // Header plus fixed scalars and three absent optional values precede the maps.
    let empty = self::life().encode_ledger().unwrap();
    for count in [100_000u32, 100_001, u32::MAX] {
        let mut bytes = empty.clone();
        bytes[175..179].copy_from_slice(&count.to_le_bytes());
        resign(&mut bytes);
        assert!(Lifecycle::decode_ledger(&bytes).is_err());
    }
}

#[test]
fn legacy_labels_must_fit_the_persisted_utf8_byte_limit() {
    for label in [
        String::new(),
        "x".into(),
        "x".repeat(4096),
        "é".repeat(2048),
    ] {
        let mut life = life();
        life.adopt_legacy(&export(label)).unwrap();
        let bytes = life.encode_ledger().unwrap();
        assert_eq!(
            Lifecycle::decode_ledger(&bytes)
                .unwrap()
                .encode_ledger()
                .unwrap(),
            bytes
        );
    }
    for label in ["x".repeat(4097), "é".repeat(2049)] {
        let mut life = life();
        let before = life.encode_ledger().unwrap();
        assert!(matches!(
            life.adopt_legacy(&export(label)),
            Err(CacheError::InvalidRequest { .. })
        ));
        assert_eq!(life.encode_ledger().unwrap(), before);
    }
}

#[test]
fn duplicate_and_reordered_root_maps_are_rejected_or_canonicalized() {
    let mut life = life();
    for id in [2, 1] {
        life.register_root(RootId::new(id)).unwrap();
    }
    let valid = life.encode_ledger().unwrap();
    // Empty root record: id, three u64 stamps/counters, optional unavailable,
    // flags, optional last generation. No optional values are present here.
    let start = 179;
    let width = 35;
    assert_eq!(
        u64::from_le_bytes(valid[start..start + 8].try_into().unwrap()),
        1
    );
    let mut reordered = valid.clone();
    reordered[start..start + width].copy_from_slice(&valid[start + width..start + 2 * width]);
    reordered[start + width..start + 2 * width].copy_from_slice(&valid[start..start + width]);
    resign(&mut reordered);
    assert_eq!(
        Lifecycle::decode_ledger(&reordered)
            .unwrap()
            .encode_ledger()
            .unwrap(),
        valid
    );
    let mut duplicate = valid.clone();
    duplicate[start + width..start + 2 * width].copy_from_slice(&valid[start..start + width]);
    resign(&mut duplicate);
    assert!(matches!(
        Lifecycle::decode_ledger(&duplicate),
        Err(CacheError::CorruptLedger {
            reason: "duplicate"
        })
    ));
}

#[test]
fn all_publication_classes_handle_empty_one_max_and_over_max_counts() {
    type Publish = fn(
        &mut Lifecycle,
        RootId,
        u64,
        &[dctx_adapter_cache::ObjectId],
    ) -> Result<dctx_adapter_cache::GenerationId, CacheError>;
    for publish in [
        Lifecycle::publish_view as Publish,
        Lifecycle::publish_overlay,
        Lifecycle::publish_pack,
        Lifecycle::publish_preview,
    ] {
        for size in [0, 1, 4096, 4097, u64::MAX] {
            let mut life = life();
            let root = RootId::new(u64::MAX);
            life.register_root(root).unwrap();
            let result = publish(&mut life, root, size, &[]);
            assert_eq!(result.is_ok(), size > 0 && size <= 4096, "size {size}");
            assert!(life.p3_holds() && life.p4_holds());
            canonical(&life.encode_ledger().unwrap());
        }
    }
    for size in [0, 1, 4096, 4097, u64::MAX] {
        let mut life = life();
        let root = RootId::new(1);
        life.register_root(root).unwrap();
        assert!(life.insert_fact(size, &[]).is_err());
        assert_eq!(
            life.insert_fact(size, &[root, root]).is_ok(),
            size > 0 && size <= 4096
        );
        let mut life = self::life();
        life.register_root(root).unwrap();
        assert_eq!(
            life.insert_segment(size, u32::MAX, &[root]).is_ok(),
            size > 0 && size <= 4096
        );
        let mut life = self::life();
        assert_eq!(life.load_staged(size, 0).is_ok(), size > 0 && size <= 4096);
    }
}

#[test]
fn reservations_and_port_calls_refuse_overflow_and_settle_once() {
    for size in [0, 1, 4096, 4097, u64::MAX] {
        let mut life = life();
        let result = life.reserve(size);
        if size == 0 || size > 4096 {
            assert!(result.is_err());
            continue;
        }
        let reservation = result.unwrap();
        assert!(
            life.commit_reservation(
                dctx_core::budget::Reservation::new(reservation.id(), size),
                size + 1
            )
            .is_err()
        );
        life.commit(reservation, size).unwrap();
        assert_eq!(life.admitted(), size);
        assert!(life.p4_holds());
    }
    let mut life = life();
    let hold = life.reserve(1).unwrap();
    let again = dctx_core::budget::Reservation::new(hold.id(), 1);
    life.abort(hold);
    let bytes = life.encode_ledger().unwrap();
    life.abort(again);
    assert_eq!(life.encode_ledger().unwrap(), bytes);
}

#[test]
fn malformed_writer_order_is_refused_and_a_valid_writer_round_trips_each_step() {
    let root = RootId::new(1);
    let mut life = life();
    life.register_root(root).unwrap();
    assert!(life.writer_reserve(root, 1).is_err());
    assert!(life.writer_stage_segment(root, 1).is_err());
    assert!(life.writer_validate_segment(root).is_err());
    assert!(life.writer_stage_manifest(root, 1, &[]).is_err());
    assert!(life.writer_validate_manifest(root).is_err());
    assert!(life.writer_swap(root).is_err());
    life.writer_acquire(root, SessionId::new(0)).unwrap();
    assert_eq!(
        life.writer_acquire(root, SessionId::new(1)).unwrap_err(),
        CacheError::Busy
    );
    life.writer_reserve(root, 2).unwrap();
    life.writer_stage_segment(root, 1).unwrap();
    canonical(&life.encode_ledger().unwrap());
    life.writer_validate_segment(root).unwrap();
    life.writer_stage_manifest(root, 1, &[]).unwrap();
    canonical(&life.encode_ledger().unwrap());
    life.writer_validate_manifest(root).unwrap();
    life.writer_swap(root).unwrap();
    canonical(&life.encode_ledger().unwrap());
    life.writer_release(root).unwrap();
    canonical(&life.encode_ledger().unwrap());
}

#[test]
fn readonly_plans_and_empty_maintenance_are_byte_identical() {
    let mut life = life();
    let root = RootId::new(1);
    life.register_root(root).unwrap();
    let fact = life.insert_fact(1, &[root]).unwrap();
    life.publish_view(root, 1, &[fact]).unwrap();
    let bytes = life.encode_ledger().unwrap();
    for _ in 0..2 {
        let request = GcRequest {
            root: None,
            budget: SliceBudget::explicit(0),
            bypass_grace: false,
            max_bytes: Some(0),
            cancel: None,
        };
        assert_eq!(
            life.plan_gc(request).unwrap(),
            life.plan_gc(request).unwrap()
        );
        assert_eq!(life.status(), life.status());
        assert_eq!(life.buckets().admitted(), life.admitted());
        assert_eq!(life.object_ids().len(), 2);
        assert_eq!(life.object_state(fact), Some(Physical::Published));
        assert_eq!(life.object_bytes(fact), Some(1));
        let dry = PurgeRequest {
            scope: PurgeScope::All,
            confirm: false,
            dry_run: true,
        };
        assert_eq!(life.purge(dry).unwrap(), life.purge(dry).unwrap());
        assert_eq!(life.legacy_dry_run(&[]), life.legacy_dry_run(&[]));
        assert_eq!(life.encode_ledger().unwrap(), bytes);
    }
    let mut disabled = AutoCleanPolicy::product_default();
    disabled.enabled = false;
    assert!(
        !life
            .auto_clean(FakeClock::at(0).sample(), disabled)
            .unwrap()
            .ran
    );
    assert_eq!(life.encode_ledger().unwrap(), bytes);
    assert!(
        life.purge(PurgeRequest {
            scope: PurgeScope::All,
            confirm: false,
            dry_run: false
        })
        .is_err()
    );
    assert_eq!(life.encode_ledger().unwrap(), bytes);
}

#[test]
fn legacy_duplicate_order_and_repeat_apply_do_not_remove_other_records() {
    let mut life = life();
    let mut a = export("a".into());
    let mut b = export("b".into());
    b.evidence = ExportEvidence::OwnedText { identity: 8 };
    for item in [&b, &a, &a] {
        life.adopt_legacy(item).unwrap();
    }
    assert_eq!(life.adopted_labels(), ["a", "b"]);
    let dry = life.legacy_dry_run(&[b.clone(), a.clone(), a.clone()]);
    assert_eq!(dry.candidates.len(), 3);
    a.evidence = ExportEvidence::Modified { identity: 7 };
    let result = life.legacy_apply(&[a, b], &[8, 8, 7]).unwrap();
    assert_eq!(result.reclaimed.len(), 1);
    assert_eq!(result.untouched.len(), 2);
    assert_eq!(life.adopted_labels(), ["a"]);
    assert!(life.legacy_apply(&[], &[]).unwrap().reclaimed.is_empty());
}

#[test]
fn object_and_ledger_adapters_preserve_bytes_under_repeat_and_concurrent_reads() {
    let temp = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(temp.path()).unwrap();
    let objects = FsObjectStore::open(&root).unwrap();
    for (id, bytes) in [
        (0, &b""[..]),
        (1, &b"\xff"[..]),
        (u64::MAX, &b"\0\xffabc"[..]),
    ] {
        for _ in 0..2 {
            objects.put_object(id, bytes).unwrap();
        }
        let mut read = Vec::new();
        objects
            .open_object(id)
            .unwrap()
            .unwrap()
            .read_to_end(&mut read)
            .unwrap();
        assert_eq!(read, bytes);
        assert!(objects.put_object(id, b"different").is_err());
    }
    assert_eq!(
        objects.orphans(&BTreeSet::new()).unwrap(),
        objects.orphans(&BTreeSet::new()).unwrap()
    );
    for path in [
        "",
        "..",
        "../escape",
        "objects/../escape",
        "a:b",
        "a\\b",
        "a\0b",
    ] {
        assert!(
            objects.unlink_relative(Path::new(path)).is_err(),
            "{path:?}"
        );
    }
    let ledger = FsLedgerStore::open(&root).unwrap();
    let life = life();
    ledger.store(&life).unwrap();
    for _ in 0..2 {
        ledger.store(&life).unwrap();
    }
    std::thread::scope(|scope| {
        let readers: Vec<_> = (0..4)
            .map(|_| {
                scope.spawn(|| {
                    ledger
                        .read(Quota::scaled(4096), |life| life.encode_ledger())
                        .unwrap()
                })
            })
            .collect();
        for reader in readers {
            assert_eq!(reader.join().unwrap(), life.encode_ledger().unwrap());
        }
    });
    assert!(ledger.load(Quota::scaled(4096)).unwrap().fault.is_none());
    objects.unlink_relative(Path::new("objects/1")).unwrap();
    objects.unlink_relative(Path::new("objects/1")).unwrap();
}

#[test]
fn simultaneous_first_object_writes_are_idempotent() {
    let temp = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(temp.path()).unwrap();
    let objects = FsObjectStore::open(&root).unwrap();
    let barrier = std::sync::Barrier::new(8);
    std::thread::scope(|scope| {
        let writers: Vec<_> = (0..8)
            .map(|_| {
                scope.spawn(|| {
                    barrier.wait();
                    objects.put_object(1, b"identical")
                })
            })
            .collect();
        for writer in writers {
            writer.join().unwrap().unwrap();
        }
    });
    assert_eq!(std::fs::read(root.join("objects/1")).unwrap(), b"identical");
}

#[test]
fn unknown_root_operations_refuse_zero_and_max_ids_without_panicking() {
    for raw in [0, 1, u64::MAX] {
        let mut life = life();
        let root = RootId::new(raw);
        assert_eq!(root.raw(), raw);
        assert_eq!(SessionId::new(raw).raw(), raw);
        assert!(life.touch(root).is_err());
        assert!(life.pin_root(root, true).is_err());
        assert!(life.note_missing(root).is_err());
        assert!(life.note_offline(root).is_err());
        assert!(life.note_online(root).is_err());
        assert!(life.release_recovery(root).is_err());
        assert!(life.open_current(root).is_err());
        assert!(life.reader_begin(root, SessionId::new(raw)).is_err());
        assert!(life.writer_acquire(root, SessionId::new(raw)).is_err());
        assert!(life.current_generation(root).is_none());
        assert!(life.persist_writes(root).is_none());
        assert!(life.last_use(root).is_none());
        assert!(life.unavailable_since(root).is_none());
        assert!(!life.root_expired(root));
        assert!(!life.history_reset(root));
        life.acknowledge_history(root);
        assert!(life.p3_holds() && life.p4_holds());
    }
}

#[test]
fn root_availability_and_reader_lifecycle_keep_persisted_invariants() {
    let mut life = life();
    let root = RootId::new(1);
    life.register_root(root).unwrap();
    assert!(life.register_root(root).is_err());
    life.touch(root).unwrap();
    life.pin_root(root, true).unwrap();
    life.note_missing(root).unwrap();
    let unavailable = life.unavailable_since(root);
    life.note_missing(root).unwrap();
    life.note_offline(root).unwrap();
    assert_eq!(life.unavailable_since(root), unavailable);
    life.note_online(root).unwrap();
    assert!(life.unavailable_since(root).is_none());
    let fact = life.insert_fact(1, &[root]).unwrap();
    let generation = life.publish_view(root, 1, &[fact]).unwrap();
    assert_eq!(life.open_current(root).unwrap(), generation);
    let manifest = life.manifest_of(root, generation).unwrap();
    let object = life.manifest_object(manifest).unwrap();
    assert_eq!(life.manifest_state(manifest), Some(Physical::Published));
    life.pin_manifest(manifest, true).unwrap();
    life.pin_manifest(manifest, false).unwrap();
    let lease = life.reader_begin(root, SessionId::new(1)).unwrap();
    assert!(lease.raw() > 0);
    assert_eq!(life.open_leases(), [lease]);
    assert_eq!(life.reader_confirm(lease).unwrap(), generation);
    assert!(life.reader_confirm(lease).is_err());
    life.reader_release(lease).unwrap();
    life.reader_release(lease).unwrap();
    assert!(life.open_leases().is_empty());
    life.reader_owner_dead(lease).unwrap();
    life.release_recovery(root).unwrap();
    life.revoke_manifest(manifest).unwrap();
    assert_eq!(
        life.open_current(root).unwrap_err(),
        CacheError::PolicyDenied
    );
    life.note_incompatible(object).unwrap();
    assert!(matches!(
        life.open_current(root),
        Err(CacheError::CorruptIndex { .. })
    ));
    canonical(&life.encode_ledger().unwrap());
}

#[test]
fn injected_time_and_maintenance_bounds_do_not_use_a_system_clock() {
    let mut clock = FakeClock::at(u64::MAX - 1);
    clock.advance(1);
    clock.advance_days(u64::MAX);
    assert_eq!(clock.sample().wall.0, u64::MAX);
    clock.rewind_wall(u64::MAX);
    assert_eq!(clock.sample().wall.0, 0);
    clock.set_boot(u64::MAX);
    assert_eq!(clock.boot_id(), u64::MAX);
    let mut life = life();
    life.follow(&FakeClock::at(1));
    life.observe(clock.sample());
    assert!(life.clock_fault());
    assert_eq!(life.wall().0, 1);
    let evidence = dctx_adapter_cache::RestartEvidence {
        boot_id: clock.boot_id(),
        held_sessions: BTreeSet::new(),
    };
    life.reconcile(&evidence);
    assert_eq!(life.boot_id(), u64::MAX);
    let session = SessionId::new(1);
    life.hold_maintenance(session).unwrap();
    assert!(life.maintenance_held());
    assert!(life.hold_maintenance(session).is_err());
    assert!(life.release_maintenance(SessionId::new(2)).is_err());
    assert!(
        life.startup(FakeClock::at(1).sample(), None)
            .unwrap()
            .deferred
    );
    life.release_maintenance(session).unwrap();
    for max in [0, 1, u32::MAX] {
        let report = life
            .startup_with(
                FakeClock::at(1).sample(),
                Some(&evidence),
                SliceBudget::explicit(max),
            )
            .unwrap();
        assert!(report.examined <= max);
    }
    assert_eq!(life.cursor(), 0);
    assert!(life.last_auto_clean().is_none());
    assert_eq!(life.touches(), 0);
    assert!(life.logical_revision() > 0);
    let request = GcRequest {
        root: None,
        budget: SliceBudget::explicit(0),
        bypass_grace: true,
        max_bytes: Some(0),
        cancel: None,
    };
    assert_eq!(life.gc(request).unwrap().examined, 0);
    canonical(&life.encode_ledger().unwrap());
}

#[test]
fn quota_diskfull_legacy_and_compaction_boundaries_keep_accounting() {
    for cap in [0, 1, 4096, u64::MAX] {
        let mut life = Lifecycle::new(Quota::scaled(cap).with_ceilings(cap, cap, cap)).unwrap();
        let root = RootId::new(1);
        life.register_root(root).unwrap();
        life.inject_disk_full(true);
        assert_eq!(
            life.insert_fact(1, &[root]).unwrap_err(),
            CacheError::DiskFull
        );
        life.inject_disk_full(false);
        assert!(life.set_hard_cap(cap).is_ok());
        assert!(life.p4_holds());
    }
    let mut life = life();
    let wal = life.observe_legacy_wal(u64::MAX).unwrap();
    assert!(life.legacy_frozen());
    assert!(
        life.reserve_for(None, dctx_adapter_cache::Purpose::Payload, 1)
            .is_err()
    );
    assert_eq!(life.reclaim_legacy(wal).unwrap(), u64::MAX);
    assert!(!life.legacy_frozen());
    assert!(life.reclaim_legacy(wal).is_err());
    assert!(life.observe_legacy_wal(0).is_err());
    let root = RootId::new(1);
    life.register_root(root).unwrap();
    let segment = life.insert_segment(64, 500_000, &[root]).unwrap();
    assert!(life.compaction_candidate(segment));
    let compacted = life.compact_segment(segment).unwrap();
    assert_eq!(life.object_bytes(compacted.new_segment), Some(32));
    life.fail_unlink(compacted.new_segment);
    life.clear_unlink_fault(compacted.new_segment);
    assert!(life.p4_holds());
    assert!(life.reclaimable_ids().is_empty());
    assert!(life.evict_to_fit(u64::MAX));
    assert!(life.evicted_ids().is_empty());
    assert!(life.take_evicted().is_empty());
    assert_eq!(life.begin_reclaim(), 0);
    assert!(life.finish_reclaim().is_empty());
    canonical(&life.encode_ledger().unwrap());
}

#[test]
fn crash_reconciliation_and_exhausted_id_cursors_are_typed_refusals() {
    let mut life = life();
    let root = RootId::new(1);
    life.register_root(root).unwrap();
    life.writer_acquire(root, SessionId::new(1)).unwrap();
    life.writer_reserve(root, 1).unwrap();
    life.writer_stage_segment(root, 1).unwrap();
    life.crash_writer(root).unwrap();
    assert_eq!(
        life.writer_phase(root),
        dctx_adapter_cache::WriterPhase::Dead
    );
    life.reconcile(&dctx_adapter_cache::RestartEvidence {
        boot_id: 1,
        held_sessions: BTreeSet::new(),
    });
    assert_eq!(
        life.writer_phase(root),
        dctx_adapter_cache::WriterPhase::Free
    );
    canonical(&life.encode_ledger().unwrap());
    let valid = self::life().encode_ledger().unwrap();
    // Five allocator cursors follow quota, clocks, flags and the legacy ceiling.
    for cursor in 0..5 {
        let mut bytes = valid.clone();
        let offset = 16 + 92 + cursor * 8;
        bytes[offset..offset + 8].copy_from_slice(&u64::MAX.to_le_bytes());
        resign(&mut bytes);
        let mut restored = Lifecycle::decode_ledger(&bytes).unwrap();
        restored.register_root(root).unwrap();
        if cursor == 4 {
            assert!(
                restored
                    .reserve_for(None, dctx_adapter_cache::Purpose::Payload, 1)
                    .is_err()
            );
        } else {
            let _ = restored.publish_view(root, 1, &[]);
        }
        assert!(restored.p3_holds() && restored.p4_holds());
    }
}

#[test]
fn ledger_stage_updates_reclaiming_and_lease_holds_preserve_transactions() {
    let temp = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(temp.path()).unwrap();
    let ledger = FsLedgerStore::open(&root).unwrap();
    let objects = FsObjectStore::open(&root).unwrap();
    let quota = Quota::scaled(4096);
    let life = life();
    ledger.stage(&life).unwrap();
    ledger.commit().unwrap();
    let before = std::fs::read(root.join("ledger.v1")).unwrap();
    let result: Result<(), _> = ledger.update(quota, |_| Err("rejected"));
    assert!(matches!(
        result,
        Err(dctx_adapter_cache::adapters::LedgerUpdateError::Body(
            "rejected"
        ))
    ));
    ledger
        .update(quota, |life| {
            life.register_root(RootId::new(1))?;
            Ok::<_, CacheError>(((), false))
        })
        .unwrap();
    assert_eq!(std::fs::read(root.join("ledger.v1")).unwrap(), before);
    let (_, life) = ledger
        .update_reclaiming(quota, &objects, |life| {
            let id = RootId::new(1);
            life.register_root(id)?;
            life.publish_view(id, 1, &[])?;
            Ok::<_, CacheError>(((), true))
        })
        .unwrap();
    let mut life = life;
    let lease = life
        .reader_begin(RootId::new(1), SessionId::new(1))
        .unwrap();
    let hold = ledger.hold_lease(lease).unwrap();
    assert!(hold.path().is_file());
    assert!(ledger.hold_lease(lease).is_err());
    drop(hold);
    let loaded = ledger.load(quota).unwrap();
    assert!(loaded.fault.is_none());
    assert_eq!(
        loaded.lifecycle.open_current(RootId::new(1)).unwrap().raw(),
        1
    );
}

#[cfg(unix)]
#[test]
fn lease_liveness_lock_refuses_symlinks() {
    let temp = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(temp.path()).unwrap();
    let ledger = FsLedgerStore::open(&root).unwrap();
    let mut life = life();
    let id = RootId::new(1);
    life.register_root(id).unwrap();
    life.publish_view(id, 1, &[]).unwrap();
    let lease = life.reader_begin(id, SessionId::new(1)).unwrap();
    let victim = root.join("victim");
    std::fs::write(&victim, b"keep").unwrap();
    std::os::unix::fs::symlink(&victim, root.join(format!("lease.{}.lock", lease.raw()))).unwrap();
    assert!(ledger.hold_lease(lease).is_err());
    assert_eq!(std::fs::read(victim).unwrap(), b"keep");
}

#[test]
fn every_cache_error_names_a_next_step_and_maps_to_the_port() {
    let errors = [
        CacheError::CacheLimit {
            requested: 1,
            available: 0,
            pinned: 0,
            pending: 0,
        },
        CacheError::Busy,
        CacheError::Cancelled,
        CacheError::InvalidRequest { reason: "input" },
        CacheError::CorruptIndex { reason: "checksum" },
        CacheError::PolicyDenied,
        CacheError::DiskFull,
        CacheError::UnknownLedgerVersion { found: u32::MAX },
        CacheError::CorruptLedger { reason: "checksum" },
        CacheError::OutsideCacheRoot,
        CacheError::LedgerIo { reason: "read" },
    ];
    for error in errors {
        let message = error.to_string();
        let (_, step) = message.split_once(';').expect("an actionable error");
        assert!(!step.trim().is_empty());
        let mapped = error.into_dctx();
        assert!(!mapped.to_string().is_empty());
    }
    let mut policy = AutoCleanPolicy::product_default();
    assert!(policy.check().is_ok());
    policy.system_cap_bytes = u64::MAX;
    assert!(policy.check().is_err());
    let mut life = life();
    let reservation = life
        .reserve_for(None, dctx_adapter_cache::Purpose::Payload, 1)
        .unwrap();
    life.abort_reservation(reservation);
    assert_eq!(life.admitted(), 0);
}
