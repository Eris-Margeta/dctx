//! Facade sessions share one cache root's admission, leases, and purge.
//! Implements CACHE-v1 through the dctx session (DX-RV5H) on the adapter coordinator.
//! Depends on [`dctx::Session`] and a temporary cache directory. The suite runs these tests.
//! A live handle is the pin that keeps bytes after purge and collection.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dctx::memory::{CancelFlag, FixedClock, MemorySource};
use dctx::{
    AutoCleanConfig, DctxError, GcRequest, HostCeiling, HostGrants, OperationLimits, PurgeRequest, PurgeScope,
    RuleTexts, SelectionPolicy, Session, SnapshotRequest, Source, StoragePolicy, Timestamp,
};

const CAP: u64 = 8 * 1024 * 1024;

#[test]
fn two_sessions_on_one_root_share_admission() {
    let dir = tempfile::tempdir().expect("temp");
    let cache = cache_dir(dir.path());
    let mut first = open(&cache, CAP, wide_source());
    let handle = first.snapshot(&SnapshotRequest::immutable()).expect("first");
    let admitted = first.cache_ledger().expect("ledger").admitted_bytes;
    assert!(admitted > 0);
    let kept = objects(&cache);
    assert_eq!(payload(&kept), admitted);
    assert_eq!(first.generation_count(), 1);

    let mut tight = open(&cache, admitted, wide_source());
    let error = tight.snapshot(&SnapshotRequest::immutable()).expect_err("over cap");
    match error {
        DctxError::CacheLimit { requested, available } => assert!(requested > available),
        other => panic!("expected cache limit, got {other:?}"),
    }
    assert_eq!(tight.generation_count(), 0);
    assert_eq!(objects(&cache), kept);
    assert_eq!(tight.cache_ledger().expect("tight").admitted_bytes, admitted);

    let mut third = open(&cache, CAP, narrow_source());
    let again = third.snapshot(&SnapshotRequest::immutable()).expect("fits");
    assert_eq!(again.generation(), 1);
    let grown = third.cache_ledger().expect("grown").admitted_bytes;
    assert!(grown > admitted);
    let after = objects(&cache);
    assert!(after.len() > kept.len());
    for (name, bytes) in &kept {
        assert_eq!(after.get(name), Some(bytes));
    }
    drop(handle);
}

#[test]
fn live_snapshot_lease_is_persisted_and_survives_reopen() {
    let dir = tempfile::tempdir().expect("temp");
    let cache = cache_dir(dir.path());
    let mut first = open(&cache, CAP, wide_source());
    let handle = first.snapshot(&SnapshotRequest::immutable()).expect("snapshot");
    let admitted = first.cache_ledger().expect("ledger").admitted_bytes;
    let kept = objects(&cache);
    assert_eq!(payload(&kept), admitted);
    drop(first);

    let mut reopened = open(&cache, CAP, wide_source());
    assert_eq!(reopened.cache_ledger().expect("reloaded").admitted_bytes, admitted);
    let blocked = reopened
        .cache_purge(PurgeRequest { confirm: true, apply: true, scope: PurgeScope::All })
        .expect("purge");
    let still = reopened.cache_gc(GcRequest { apply: true }).expect("gc");
    assert!(blocked.blocked_generations >= 1);
    assert_eq!(blocked.reclaimed_bytes, 0);
    assert_eq!(still.reclaimed_bytes, 0);
    assert_eq!(objects(&cache), kept);
    assert_eq!(reopened.cache_ledger().expect("held").admitted_bytes, admitted);

    drop(handle);
    let purged = reopened
        .cache_purge(PurgeRequest { confirm: true, apply: true, scope: PurgeScope::All })
        .expect("release purge");
    let _collected = reopened.cache_gc(GcRequest { apply: true }).expect("release gc");
    assert!(purged.reclaimed_bytes > 0);
    assert!(objects(&cache).is_empty());
    assert_eq!(reopened.cache_ledger().expect("empty").admitted_bytes, 0);
}

#[test]
fn facade_purge_and_gc_remove_unleased_bytes_and_keep_leased_ones() {
    let dir = tempfile::tempdir().expect("temp");
    let cache = cache_dir(dir.path());
    let mut session = open(&cache, CAP, wide_source());
    let unleased = session.snapshot(&SnapshotRequest::immutable()).expect("unleased");
    let unleased_files = objects(&cache);
    assert!(!unleased_files.is_empty());
    drop(unleased);
    assert_eq!(objects(&cache), unleased_files);

    let leased = session.snapshot(&SnapshotRequest::immutable()).expect("leased");
    let both = objects(&cache);
    assert!(both.len() > unleased_files.len());
    let leased_only: BTreeMap<_, _> = both
        .iter()
        .filter(|(name, _)| !unleased_files.contains_key(*name))
        .map(|(name, bytes)| (name.clone(), bytes.clone()))
        .collect();
    assert!(!leased_only.is_empty());

    let refused = session
        .cache_purge(PurgeRequest { confirm: false, apply: true, scope: PurgeScope::All })
        .expect_err("confirm");
    assert_eq!(refused, DctxError::invalid("purge requires confirm"));
    let dry = session
        .cache_purge(PurgeRequest { confirm: true, apply: false, scope: PurgeScope::All })
        .expect("dry");
    assert_eq!(dry.reclaimed_bytes, 0);
    assert_eq!(objects(&cache), both);
    assert_eq!(session.generation_count(), 2);

    let purged = session
        .cache_purge(PurgeRequest { confirm: true, apply: true, scope: PurgeScope::All })
        .expect("purge");
    let collected = session.cache_gc(GcRequest { apply: true }).expect("gc");
    assert!(purged.blocked_generations >= 1);
    assert!(purged.reclaimed_bytes > 0);
    assert_eq!(collected.reclaimed_bytes, 0);
    assert_eq!(objects(&cache), leased_only);
    assert_eq!(payload(&leased_only), session.cache_ledger().expect("kept").admitted_bytes);
    assert_eq!(session.generation_count(), 1);
    let _kept = session.cache_status();
    assert!(leased.generation() >= 1);

    drop(leased);
    let finished = session
        .cache_purge(PurgeRequest { confirm: true, apply: true, scope: PurgeScope::All })
        .expect("final purge");
    let _ = session.cache_gc(GcRequest { apply: true }).expect("final gc");
    assert!(finished.reclaimed_bytes > 0);
    assert!(objects(&cache).is_empty());
    assert_eq!(session.cache_ledger().expect("gone").admitted_bytes, 0);
    assert_eq!(session.generation_count(), 0);
}

fn open(cache: &Path, max_bytes: u64, source: impl Source + Send + 'static) -> Session {
    Session::builder(source)
        .clock(FixedClock(Timestamp(0)))
        .cancellation(Arc::new(CancelFlag::new()))
        .selection(SelectionPolicy::from_texts(&RuleTexts::default()))
        .ceiling(HostCeiling::base())
        .grants(HostGrants::PermitAll)
        .storage(StoragePolicy::Ephemeral { max_bytes })
        .limits(OperationLimits::default())
        .place("fixture")
        .view("map")
        .cache_root(cache, 11)
        .auto_clean(AutoCleanConfig::default())
        .build()
        .expect("session")
}

fn wide_source() -> MemorySource {
    let mut source = MemorySource::new();
    source.insert_file("src/wide.rs", b"fn wide_marker() {}\n", 0).expect("wide");
    source.insert_file("src/extra.rs", b"fn extra_marker() {}\n", 0).expect("extra");
    source
}

fn narrow_source() -> MemorySource {
    let mut source = MemorySource::new();
    source.insert_file("src/narrow.rs", b"fn narrow() {}\n", 0).expect("narrow");
    source
}

fn cache_dir(root: &Path) -> PathBuf {
    let cache = root.join("cache");
    std::fs::create_dir(&cache).expect("dir");
    cache
}

fn objects(cache: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut out = BTreeMap::new();
    let Ok(entries) = std::fs::read_dir(cache.join("objects")) else {
        return out;
    };
    for entry in entries {
        let entry = entry.expect("entry");
        if entry.file_type().expect("type").is_file() {
            let name = entry.file_name().to_string_lossy().into_owned();
            out.insert(name, std::fs::read(entry.path()).expect("bytes"));
        }
    }
    out
}

fn payload(files: &BTreeMap<String, Vec<u8>>) -> u64 {
    files.values().fold(0, |total, bytes| {
        total.saturating_add(u64::try_from(bytes.len()).unwrap_or(u64::MAX))
    })
}
