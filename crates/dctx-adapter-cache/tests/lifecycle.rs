//! Reachability, pins, purge, revocation and quarantine.
//! Implements CACHE-v1 `Live = closure(retained manifests ∪ active leases)` and purge rules.
//! Depends on the public ledger API. `p3_pin_gc_interleavings` is the P3 schedule.

#[path = "support/mod.rs"]
mod support;

use dctx_adapter_cache::{
    CacheError, ExportEvidence, GcRequest, KnownExport, Lifecycle, OutputPlace, Physical,
    PurgeRequest, PurgeScope, Quota, RootId, SessionId, SliceBudget, RECOGNIZED_LEGACY_VERSION,
};

fn ledger() -> Lifecycle {
    Lifecycle::new(Quota::scaled(10_000)).expect("quota")
}

#[test]
fn p3_pin_gc_interleavings() {
    let mut life = ledger();
    let root = RootId::new(1);
    life.register_root(root).expect("root");
    let fact = life.insert_fact(10, &[root]).expect("fact");
    let pinned = life.publish_view(root, 5, &[fact]).expect("view");
    let lease = life.reader_begin(root, SessionId::new(4)).expect("begin");
    assert_eq!(life.reader_confirm(lease).expect("confirm"), pinned);
    life.publish_view(root, 5, &[]).expect("view");
    life.publish_view(root, 5, &[]).expect("view");
    life.publish_view(root, 5, &[]).expect("view");
    support::drain(&mut life, true);
    assert_eq!(life.object_bytes(fact), Some(10));
    assert!(life.p3_holds());
    life.reader_release(lease).expect("release");
    support::drain(&mut life, true);
    assert!(life.object_bytes(fact).is_none());
    assert!(life.p3_holds());
    assert!(life.p4_holds());

    let mut shared = ledger();
    let left = RootId::new(1);
    let right = RootId::new(2);
    shared.register_root(left).expect("root");
    shared.register_root(right).expect("root");
    let fact = shared.insert_fact(10, &[left, right]).expect("shared");
    let only_left = shared.insert_fact(7, &[left]).expect("exclusive");
    shared.publish_view(left, 4, &[fact, only_left]).expect("view");
    shared.publish_view(right, 4, &[fact]).expect("view");
    shared.publish_view(left, 4, &[]).expect("view");
    shared.publish_view(left, 4, &[]).expect("view");
    shared.publish_view(left, 4, &[]).expect("view");
    support::drain(&mut shared, true);
    assert_eq!(shared.object_bytes(fact), Some(10));
    assert!(shared.object_bytes(only_left).is_none());
    assert!(shared.p3_holds());
    let status = shared.status();
    let row = status.roots.iter().find(|row| row.id == right).expect("right");
    assert_eq!(row.shared_bytes, 10);
}

#[test]
fn acquiring_lease_does_not_keep_the_generation() {
    let mut life = ledger();
    let root = RootId::new(1);
    life.register_root(root).expect("root");
    let retired_fact = life.insert_fact(10, &[root]).expect("fact");
    let kept = life.insert_fact(3, &[root]).expect("fact");
    life.publish_view(root, 5, &[retired_fact]).expect("view");
    let lease = life.reader_begin(root, SessionId::new(1)).expect("begin");
    life.publish_view(root, 5, &[kept]).expect("view");
    life.publish_view(root, 5, &[kept]).expect("view");
    life.publish_view(root, 5, &[kept]).expect("view");
    assert!(life.reader_confirm(lease).is_err());
    support::drain(&mut life, true);
    assert!(life.object_bytes(retired_fact).is_none());
    assert_eq!(life.object_bytes(kept), Some(3));
    assert!(life.p3_holds());
}

#[test]
fn purge_requires_confirmation_and_keeps_leased_bytes() {
    let mut life = ledger();
    let left = RootId::new(1);
    let right = RootId::new(2);
    life.register_root(left).expect("root");
    life.register_root(right).expect("root");
    let shared = life.insert_fact(12, &[left, right]).expect("shared");
    let only_left = life.insert_fact(9, &[left]).expect("exclusive");
    let generation = life.publish_view(left, 4, &[shared, only_left]).expect("view");
    life.publish_view(right, 4, &[shared]).expect("view");
    let export = KnownExport {
        place: OutputPlace::Registry,
        label: "registry/owned.bin".to_string(),
        bytes: 6,
        evidence: ExportEvidence::OwnedBinary { version: RECOGNIZED_LEGACY_VERSION, identity: 3 },
    };
    life.adopt_legacy(&export).expect("adopt");
    let revision = life.logical_revision();
    assert!(matches!(
        life.purge(PurgeRequest { scope: PurgeScope::Root(left), confirm: false, dry_run: false }),
        Err(CacheError::InvalidRequest { .. })
    ));
    assert_eq!(life.logical_revision(), revision);
    let planned = life
        .purge(PurgeRequest { scope: PurgeScope::Root(left), confirm: false, dry_run: true })
        .expect("plan");
    assert!(planned.dry_run);
    assert_eq!(planned.dropped, 1);
    assert_eq!(life.logical_revision(), revision);
    assert_eq!(life.adopted_labels(), vec!["registry/owned.bin".to_string()]);

    let lease = life.reader_begin(left, SessionId::new(1)).expect("begin");
    life.reader_confirm(lease).expect("confirm");
    let held = life
        .purge(PurgeRequest { scope: PurgeScope::Root(left), confirm: true, dry_run: false })
        .expect("purge");
    assert_eq!(held.held_by_lease, 1);
    assert_eq!(held.dropped, 0);
    assert!(life.status().deletion_requested_bytes >= 4);
    assert_eq!(life.open_current(left).expect("open"), generation);
    life.reader_release(lease).expect("release");
    let dropped = life
        .purge(PurgeRequest { scope: PurgeScope::Root(left), confirm: true, dry_run: false })
        .expect("purge");
    assert_eq!(dropped.dropped, 1);
    support::drain(&mut life, true);
    assert_eq!(life.object_bytes(shared), Some(12));
    assert!(life.object_bytes(only_left).is_none());
    assert_eq!(life.adopted_labels(), vec!["registry/owned.bin".to_string()]);
    assert!(life.p3_holds());
}

#[test]
fn revocation_hides_a_generation_before_gc() {
    let mut life = ledger();
    let root = RootId::new(1);
    life.register_root(root).expect("root");
    let generation = life.publish_view(root, 8, &[]).expect("view");
    let manifest = life.manifest_of(root, generation).expect("manifest");
    let object = life.manifest_object(manifest).expect("object");
    life.revoke_manifest(manifest).expect("revoke");
    assert!(matches!(life.open_current(root), Err(CacheError::PolicyDenied)));
    support::drain(&mut life, true);
    assert_eq!(life.object_bytes(object), Some(8));
    assert!(life.status().authorization_revoked_bytes >= 8);
    assert!(life.p3_holds());
}

#[test]
fn corrupt_objects_stay_quarantined() {
    let mut life = ledger();
    let root = RootId::new(1);
    life.register_root(root).expect("root");
    let generation = life.publish_view(root, 8, &[]).expect("view");
    let manifest = life.manifest_of(root, generation).expect("manifest");
    let object = life.manifest_object(manifest).expect("object");
    life.note_incompatible(object).expect("quarantine");
    assert!(matches!(life.open_current(root), Err(CacheError::CorruptIndex { .. })));
    support::drain(&mut life, true);
    assert_eq!(life.object_bytes(object), Some(8));
    assert_eq!(life.object_state(object), Some(Physical::Published));
    assert!(life.status().incompatible);
    assert!(life.p3_holds());
}

#[test]
fn evicted_pack_history_sets_history_reset() {
    let mut life = ledger();
    let root = RootId::new(1);
    life.register_root(root).expect("root");
    let oldest = life.publish_pack(root, 6, &[]).expect("pack");
    let middle = life.publish_pack(root, 6, &[]).expect("pack");
    assert!(!life.history_reset(root));
    let newest = life.publish_pack(root, 6, &[]).expect("pack");
    assert!(life.history_reset(root));
    assert_eq!(life.manifest_state(life.manifest_of(root, oldest).expect("id")), Some(Physical::Retired));
    assert_eq!(life.manifest_state(life.manifest_of(root, middle).expect("id")), Some(Physical::Published));
    assert_eq!(life.manifest_state(life.manifest_of(root, newest).expect("id")), Some(Physical::Published));
    assert!(life.status().roots.iter().any(|row| row.id == root && row.history_reset));
    life.acknowledge_history(root);
    assert!(!life.history_reset(root));
}

#[test]
fn plan_gc_does_not_move_the_callers_ledger() {
    let mut life = ledger();
    let root = RootId::new(1);
    life.register_root(root).expect("root");
    let fact = life.insert_fact(15, &[root]).expect("fact");
    life.hold_maintenance(SessionId::new(9)).expect("lock");
    let revision = life.logical_revision();
    let admitted = life.admitted();
    let cursor = life.cursor();
    let report = life
        .plan_gc(GcRequest {
            root: None,
            budget: SliceBudget::explicit(32),
            bypass_grace: true,
            max_bytes: None,
            cancel: None,
        })
        .expect("plan");
    assert!(report.ran);
    assert_eq!(life.logical_revision(), revision);
    assert_eq!(life.admitted(), admitted);
    assert_eq!(life.cursor(), cursor);
    assert_eq!(life.object_bytes(fact), Some(15));
    assert!(life.maintenance_held());
    assert!(matches!(
        life.gc(GcRequest {
            root: None,
            budget: SliceBudget::explicit(8),
            bypass_grace: true,
            max_bytes: None,
            cancel: None,
        }),
        Err(CacheError::Busy)
    ));
}
