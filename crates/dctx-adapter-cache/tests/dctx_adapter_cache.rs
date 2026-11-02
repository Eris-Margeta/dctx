//! Filesystem ledger, object unlink and auto-clean.
//! Restart, expiry, the system cap and the interval all go through a temp directory.
//! The fake clock supplies wall time. Nothing here starts a background cleaner.

use dctx_adapter_cache::adapters::{auto_clean, FsLedgerStore, FsObjectStore, LEDGER_FILE};
use dctx_adapter_cache::ports::LedgerStore;
use dctx_adapter_cache::{
    AutoCleanPolicy, ExportEvidence, FakeClock, KnownExport, Lifecycle, OutputPlace, Physical,
    Purpose, Quota, RestartEvidence, RootId, SessionId, WriterPhase, HARD_CAP_BYTES,
    UNLINK_GRACE_MS,
};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

fn cache_dir() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("temp");
    let root = dir.path().join("cache");
    fs::create_dir(&root).expect("dir");
    (dir, root)
}

fn rich() -> Lifecycle {
    let mut life = Lifecycle::new(Quota::scaled(500)).expect("quota");
    life.reconcile(&RestartEvidence { boot_id: 42, held_sessions: BTreeSet::new() });
    life.follow(&FakeClock::at(10));
    let root = RootId::new(3);
    life.register_root(root).expect("root");
    life.publish_view(root, 20, &[]).expect("view");
    let lease = life.reader_begin(root, SessionId::new(4)).expect("begin");
    life.reader_confirm(lease).expect("confirm");
    let other = RootId::new(8);
    life.register_root(other).expect("root");
    life.writer_acquire(other, SessionId::new(9)).expect("acquire");
    life.writer_reserve(other, 30).expect("reserve");
    life.crash_writer(other).expect("crash");
    life.adopt_legacy(&KnownExport {
        place: OutputPlace::Registry,
        label: "out".to_string(),
        bytes: 4,
        evidence: ExportEvidence::OwnedText { identity: 3 },
    })
    .expect("adopt");
    life
}

fn open(root: &Path) -> (FsLedgerStore, FsObjectStore) {
    let store = FsLedgerStore::open(root).expect("ledger");
    let objects = FsObjectStore::open(root).expect("objects");
    (store, objects)
}

#[test]
fn restart_round_trip_keeps_lock_evidence_on_disk() {
    let (_dir, root) = cache_dir();
    let (store, _) = open(&root);
    let life = rich();
    let bytes = life.encode_ledger().expect("encode");
    let admitted = life.admitted();
    store.store(&life).expect("store");
    let loaded = store.load(Quota::scaled(9)).expect("load");
    assert!(loaded.fault.is_none());
    assert_eq!(loaded.lifecycle.encode_ledger().expect("encode"), bytes);
    assert_eq!(loaded.lifecycle.boot_id(), 42);
    assert_eq!(loaded.lifecycle.status().hard_cap, 500);
    assert_eq!(loaded.lifecycle.admitted(), admitted);

    let mut kept = loaded.lifecycle;
    let mut held = BTreeSet::new();
    held.insert(SessionId::new(4));
    held.insert(SessionId::new(9));
    kept.reconcile(&RestartEvidence { boot_id: 42, held_sessions: held });
    assert_eq!(kept.writer_phase(RootId::new(8)), WriterPhase::Dead);
    assert_eq!(kept.status().active_leases, 1);

    let again = store.load(Quota::scaled(9)).expect("load");
    let mut dropped = again.lifecycle;
    dropped.reconcile(&RestartEvidence { boot_id: 42, held_sessions: BTreeSet::new() });
    assert_eq!(dropped.writer_phase(RootId::new(8)), WriterPhase::Free);
    assert_eq!(dropped.status().active_leases, 0);
    assert_eq!(dropped.admitted(), admitted - 30);
}

#[test]
fn overlay_ttl_unlinks_the_object_file_after_grace() {
    let (_dir, root) = cache_dir();
    let (store, objects) = open(&root);
    let mut life = Lifecycle::new(Quota::scaled(10_000)).expect("quota");
    let id = RootId::new(1);
    life.register_root(id).expect("root");
    let generation = life.publish_overlay(id, 8, &[]).expect("overlay");
    let manifest = life.manifest_of(id, generation).expect("manifest");
    let object = life.manifest_object(manifest).expect("object");
    objects.put_object(object.raw(), b"overlay").expect("put");
    let path = root.join("objects").join(object.raw().to_string());
    let mut policy = AutoCleanPolicy::product_default();
    policy.min_interval_ms = 0;
    policy.overlay_ttl_ms = 1_000;

    auto_clean(&store, &objects, &mut life, FakeClock::at(999).sample(), policy).expect("early");
    assert_eq!(life.manifest_state(manifest), Some(Physical::Published));
    assert_eq!(fs::read(&path).expect("file"), b"overlay");
    assert!(!life.root_expired(id));

    auto_clean(&store, &objects, &mut life, FakeClock::at(1_000).sample(), policy).expect("due");
    assert_eq!(life.manifest_state(manifest), Some(Physical::Retired));
    assert_eq!(fs::read(&path).expect("file"), b"overlay");
    assert!(!life.root_expired(id));

    let mut clock = FakeClock::at(1_000);
    clock.advance(UNLINK_GRACE_MS);
    for _ in 0..4 {
        auto_clean(&store, &objects, &mut life, clock.sample(), policy).expect("grace");
        if life.object_state(object).is_none() {
            break;
        }
    }
    assert!(life.object_state(object).is_none());
    assert!(!path.exists());
    assert!(!life.root_expired(id));
}

#[test]
fn system_cap_reclaims_views_across_roots() {
    let (_dir, root) = cache_dir();
    let (store, objects) = open(&root);
    let mut life = Lifecycle::new(Quota::scaled(6_000)).expect("quota");
    let mut files = Vec::new();
    for raw in 1..=3 {
        let id = RootId::new(raw);
        life.register_root(id).expect("root");
        let generation = life.publish_view(id, 800, &[]).expect("view");
        let manifest = life.manifest_of(id, generation).expect("manifest");
        let object = life.manifest_object(manifest).expect("object");
        objects.put_object(object.raw(), b"view").expect("put");
        files.push(root.join("objects").join(object.raw().to_string()));
    }
    assert_eq!(life.admitted(), 2_400);
    let mut policy = AutoCleanPolicy::product_default();
    policy.min_interval_ms = 0;
    policy.root_ttl_ms = 1;
    policy.system_cap_bytes = 1_000;
    let mut clock = FakeClock::at(1);
    auto_clean(&store, &objects, &mut life, clock.sample(), policy).expect("first");
    assert_eq!(life.status().hard_cap, 1_000);
    assert!(life.admitted() > 1_000);
    assert!(life.legacy_frozen());
    // Admission evicts the expired views instead of refusing (P-C11); the files wait for the next commit.
    let mut probe = life.clone();
    probe.reserve_for(None, Purpose::Payload, 1).expect("eviction makes room");
    assert_eq!(probe.take_evicted().len(), 2, "oldest first, and only as many as make room");
    for path in &files {
        assert!(path.is_file());
    }

    clock.advance(UNLINK_GRACE_MS);
    let mut reclaimed = 0u64;
    for _ in 0..4 {
        let report = auto_clean(&store, &objects, &mut life, clock.sample(), policy).expect("later");
        reclaimed = reclaimed.saturating_add(report.reclaimed_bytes);
        if life.admitted() == 0 {
            break;
        }
    }
    assert!(reclaimed > 0);
    assert_eq!(life.admitted(), 0);
    for path in &files {
        assert!(!path.exists());
    }
    life.reserve_for(None, Purpose::Payload, 100).expect("fits");
    assert!(life.reserve_for(None, Purpose::Payload, 901).is_err());
    let loaded = store.load(Quota::scaled(50)).expect("load");
    assert!(loaded.fault.is_none());
    assert_eq!(loaded.lifecycle.status().hard_cap, 1_000);
    assert_eq!(loaded.lifecycle.admitted(), 0);
}

#[test]
fn auto_clean_interval_survives_reload_and_disabled_is_a_no_op() {
    let (_dir, root) = cache_dir();
    let (store, objects) = open(&root);
    let mut life = Lifecycle::new(Quota::scaled(100)).expect("quota");
    let mut policy = AutoCleanPolicy::product_default();
    policy.min_interval_ms = 5_000;
    policy.system_cap_bytes = 100;
    let mut clock = FakeClock::at(1_000);
    let first = auto_clean(&store, &objects, &mut life, clock.sample(), policy).expect("first");
    assert!(first.ran);
    let stamp = life.last_auto_clean().expect("stamp");
    let loaded = store.load(Quota::scaled(7)).expect("load");
    assert_eq!(loaded.lifecycle.last_auto_clean(), Some(stamp));
    assert_eq!(loaded.lifecycle.status().hard_cap, 100);
    let mut life = loaded.lifecycle;
    assert!(!auto_clean(&store, &objects, &mut life, clock.sample(), policy).expect("same").ran);
    clock.advance(4_999);
    assert!(!auto_clean(&store, &objects, &mut life, clock.sample(), policy).expect("gap").ran);
    clock.advance(1);
    assert!(auto_clean(&store, &objects, &mut life, clock.sample(), policy).expect("due").ran);

    let mut idle = Lifecycle::new(Quota::scaled(80)).expect("quota");
    let staged = idle.load_staged(8, 0).expect("staged");
    store.store(&idle).expect("store");
    let image = fs::read(root.join(LEDGER_FILE)).expect("image");
    let revision = idle.logical_revision();
    let mut disabled = AutoCleanPolicy::product_default();
    disabled.enabled = false;
    disabled.staging_ttl_ms = 1;
    let report = auto_clean(&store, &objects, &mut idle, FakeClock::at(10_000).sample(), disabled).expect("off");
    assert!(!report.ran);
    assert_eq!(idle.logical_revision(), revision);
    assert_eq!(idle.object_state(staged), Some(Physical::Staged));
    assert!(idle.last_auto_clean().is_none());
    assert_eq!(fs::read(root.join(LEDGER_FILE)).expect("image"), image);
    disabled.system_cap_bytes = HARD_CAP_BYTES + 1;
    assert!(auto_clean(&store, &objects, &mut idle, FakeClock::at(10_000).sample(), disabled).is_err());
    assert_eq!(idle.logical_revision(), revision);
    assert_eq!(fs::read(root.join(LEDGER_FILE)).expect("image"), image);
}
