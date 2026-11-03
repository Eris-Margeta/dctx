// SPDX-License-Identifier: Apache-2.0
//! Explicit facade import tests for D3-LEGACY-REMOVAL §5 D3-02.
//! Uses shared in-memory store/cache spies and immutable legacy bytes.
//! Checks publication, body policy, refusal atomicity and absence of source reads.

#![forbid(unsafe_code)]

mod common;

use dctx::memory::{CancelFlag, FixedClock, MemoryCache, MemoryFactStore, MemorySource};
use dctx::{
    Cache, Cancellation, DctxError, FactStore, HostCeiling, HostGrants, OperationLimits, PackRequest, Session,
    StoragePolicy, Timestamp, VecSink,
};
use dctx_core::budget::Reservation;
use dctx_core::error::DctxError as CoreError;
use dctx_core::facts::PackSnapshot;
use dctx_core::ids::ViewKey;
use dctx_pack::{LegacyBodies, LegacyLimits, decode_legacy, encode_legacy};
use std::sync::{Arc, Mutex};

const BYTES: &[u8] = b"DIRCTXTV\x03\0\0\0\x07\0\0\0\
    \x01\0\0\x01\0\0\0\0\0\0\0\x01\0\0\0\
    \0\x08\0logo.png\x02\0\0\0\0\0\0\0\
    \0\0\0\0\0\0\0\0\x07\0\0\0\0\0\0\0\x89PNG\0\0\x01";

#[derive(Clone, Default)]
struct Store(Arc<Mutex<MemoryFactStore>>);

impl FactStore for Store {
    fn load(&self, view: &ViewKey) -> Result<Option<PackSnapshot>, CoreError> {
        self.0.lock().unwrap().load(view)
    }
    fn publish(&mut self, view: &ViewKey, snapshot: &PackSnapshot) -> Result<(), CoreError> {
        self.0.lock().unwrap().publish(view, snapshot)
    }
}

#[derive(Clone)]
struct Quota(Arc<Mutex<MemoryCache>>);

impl Cache for Quota {
    fn reserve(&mut self, bytes: u64) -> Result<Reservation, CoreError> {
        self.0.lock().unwrap().reserve(bytes)
    }
    fn commit(&mut self, reservation: Reservation, used: u64) -> Result<(), CoreError> {
        self.0.lock().unwrap().commit(reservation, used)
    }
    fn abort(&mut self, reservation: Reservation) {
        self.0.lock().unwrap().abort(reservation);
    }
}

fn session(
    store: Store,
    quota: Quota,
    cancel: Arc<dyn Cancellation + Send + Sync>,
    limits: OperationLimits,
) -> (Session, Arc<Mutex<dctx::memory::MemorySource>>) {
    let source = common::SharedSource::new(common::repository());
    let source_handle = source.handle();
    let session = Session::builder(source)
        .clock(FixedClock(Timestamp(123)))
        .cancellation(cancel)
        .selection(dctx::SelectionPolicy::from_texts(&dctx::RuleTexts::default()))
        .ceiling(HostCeiling::base())
        .grants(HostGrants::PermitAll)
        .storage(StoragePolicy::CallerOwned { max_bytes: 4096 })
        .store(store)
        .cache(quota)
        .limits(limits)
        .place("fixture")
        .view("map")
        .build()
        .unwrap();
    (session, source_handle)
}

fn quota(capacity: u64) -> Quota {
    Quota(Arc::new(Mutex::new(MemoryCache::new(capacity))))
}

fn key(name: &str) -> ViewKey {
    ViewKey::parse(name).unwrap()
}

#[test]
fn import_publishes_once_preserves_input_and_other_views_and_never_reads_source() {
    let mut store = Store::default();
    let old = decode_legacy(BYTES, LegacyBodies::MetadataOnly, LegacyLimits::default()).unwrap();
    let previous = PackSnapshot::new(old.root, old.header.version, Timestamp(0), std::collections::BTreeMap::default()).unwrap();
    store.publish(&key("previous"), &previous).unwrap();
    let quota = quota(4096);
    let (mut session, source) = session(store.clone(), quota.clone(), Arc::new(CancelFlag::new()), OperationLimits::default());
    let input = BYTES.to_vec();
    let snapshot = session.import_legacy(&input, "imported", LegacyBodies::Include, LegacyLimits::default()).unwrap();
    assert_eq!(input, BYTES);
    assert_eq!(snapshot.version().to_string(), "V3.7");
    assert_eq!(snapshot.taken_at(), Timestamp(123));
    assert_eq!(snapshot.tree().children[0].file_content.as_deref(), Some(&b"\x89PNG\0\0\x01"[..]));
    assert_eq!(snapshot.digests().len(), 1);
    assert_eq!(store.load(&key("imported")).unwrap().unwrap().digest(), snapshot.digest());
    assert_eq!(store.load(&key("previous")).unwrap().unwrap().digest(), previous.digest());
    assert_eq!(store.0.lock().unwrap().publishes(), 2);
    assert_eq!(encode_legacy(snapshot.tree(), snapshot.version(), LegacyLimits::default()).unwrap(), input);
    assert_eq!(source.lock().unwrap().reads(), [] as [std::string::String; 0]);
    assert!(!source.lock().unwrap().touched(".gitignore"));
    assert_eq!(quota.0.lock().unwrap().committed(), BYTES.len() as u64);
    assert!(session.import_legacy(BYTES, "imported", LegacyBodies::Include, LegacyLimits::default()).is_err());
    assert_eq!(store.0.lock().unwrap().publishes(), 2);
    assert_eq!(store.load(&key("imported")).unwrap().unwrap().digest(), snapshot.digest());
    assert_eq!(quota.0.lock().unwrap().reserved(), 0);
}

#[test]
fn metadata_import_does_not_invent_content_or_content_digests() {
    let store = Store::default();
    let (mut session, _) = session(store.clone(), quota(4096), Arc::new(CancelFlag::new()), OperationLimits::default());
    let snapshot =
        session.import_legacy(BYTES, "metadata", LegacyBodies::MetadataOnly, LegacyLimits::default()).unwrap();
    assert!(snapshot.tree().children[0].file_content.is_none());
    assert_eq!(snapshot.tree().children[0].content_size, 7);
    assert!(snapshot.digests().is_empty());
    assert!(store.load(&key("metadata")).unwrap().unwrap().digests().is_empty());
}

#[test]
fn invalid_bytes_and_paths_publish_nothing() {
    let store = Store::default();
    let quota = quota(4096);
    let (mut session, source) = session(store.clone(), quota.clone(), Arc::new(CancelFlag::new()), OperationLimits::default());
    for end in 0..BYTES.len() {
        assert!(session.import_legacy(&BYTES[..end], "new", LegacyBodies::Include, LegacyLimits::default()).is_err());
    }
    let mut bad_path = BYTES.to_vec();
    bad_path[34..42].copy_from_slice(b"../a.png");
    assert!(session.import_legacy(&bad_path, "new", LegacyBodies::Include, LegacyLimits::default()).is_err());
    assert!(session.import_legacy(BYTES, "", LegacyBodies::Include, LegacyLimits::default()).is_err());
    assert_eq!(store.0.lock().unwrap().publishes(), 0);
    assert!(store.load(&key("new")).unwrap().is_none());
    assert_eq!(quota.0.lock().unwrap().reserved(), 0);
    assert_eq!(quota.0.lock().unwrap().committed(), 0);
    assert_eq!(source.lock().unwrap().reads(), [] as [std::string::String; 0]);
    assert!(session.import_legacy(BYTES, "new", LegacyBodies::Include, LegacyLimits::default()).is_ok());
}

#[test]
fn cache_and_publish_refusals_do_not_leave_reservations() {
    for publish_failure in [false, true] {
        let store = Store::default();
        let quota = quota(if publish_failure { 4096 } else { 1 });
        if publish_failure {
            store.0.lock().unwrap().fail_publishes();
        }
        let (mut session, _) = session(store.clone(), quota.clone(), Arc::new(CancelFlag::new()), OperationLimits::default());
        assert!(session.import_legacy(BYTES, "new", LegacyBodies::Include, LegacyLimits::default()).is_err());
        assert_eq!(store.0.lock().unwrap().publishes(), 0);
        assert!(store.load(&key("new")).unwrap().is_none());
        assert_eq!(quota.0.lock().unwrap().reserved(), 0);
        assert_eq!(quota.0.lock().unwrap().committed(), 0);
    }
}

#[test]
fn cancellation_at_either_checkpoint_leaves_the_store_untouched() {
    struct CancelAfter(std::sync::atomic::AtomicUsize, usize);
    impl Cancellation for CancelAfter {
        fn is_cancelled(&self) -> bool {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst) >= self.1
        }
    }
    for allowed in 0..2 {
        let store = Store::default();
        let quota = quota(4096);
        let flag = Arc::new(CancelAfter(std::sync::atomic::AtomicUsize::new(0), allowed));
        let (mut session, _) = session(store.clone(), quota.clone(), flag, OperationLimits::default());
        assert!(matches!(
            session.import_legacy(BYTES, "new", LegacyBodies::Include, LegacyLimits::default()),
            Err(dctx::DctxError::Cancelled)
        ));
        assert_eq!(store.0.lock().unwrap().publishes(), 0);
        assert_eq!(quota.0.lock().unwrap().reserved(), 0);
    }
}

#[test]
fn session_limits_cannot_be_raised_by_import_limits() {
    for limits in [
        OperationLimits { source_bytes: Some(1), ..OperationLimits::default() },
        OperationLimits { work_units: Some(1), ..OperationLimits::default() },
        OperationLimits { parse_bytes: Some(1), ..OperationLimits::default() },
        OperationLimits { memory_bytes: Some(1), ..OperationLimits::default() },
    ] {
        let store = Store::default();
        let (mut session, _) = session(store.clone(), quota(4096), Arc::new(CancelFlag::new()), limits);
        assert!(session.import_legacy(BYTES, "new", LegacyBodies::Include, LegacyLimits::default()).is_err());
        assert_eq!(store.0.lock().unwrap().publishes(), 0);
    }
}

#[test]
fn maximum_revision_import_refuses_and_the_prior_revision_packs() {
    let store = Store::default();
    let quota = quota(4096);
    let mut source = MemorySource::new();
    source.insert_file("logo.png", b"\x89PNG\0\0\x01", 3).expect("png");
    let source = common::SharedSource::new(source);
    let handle = source.handle();
    let session = Session::builder(source)
        .clock(FixedClock(Timestamp(123)))
        .cancellation(Arc::new(CancelFlag::new()))
        .selection(dctx::SelectionPolicy::from_texts(&dctx::RuleTexts::default()))
        .ceiling(HostCeiling::base())
        .grants(HostGrants::PermitAll)
        .storage(StoragePolicy::CallerOwned { max_bytes: 4096 })
        .store(store.clone())
        .cache(quota.clone())
        .limits(OperationLimits::default())
        .place("fixture")
        .view("map")
        .build()
        .expect("session");
    let mut session = session;

    let mut exhausted = BYTES.to_vec();
    exhausted[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
    let decoded = decode_legacy(&exhausted, LegacyBodies::MetadataOnly, LegacyLimits::default()).expect("decode");
    assert_eq!(decoded.header.version.major, u32::MAX);
    assert_eq!(
        session
            .import_legacy(&exhausted, "map", LegacyBodies::Include, LegacyLimits::default())
            .err(),
        Some(DctxError::invalid("version exhausted"))
    );
    assert_eq!(store.0.lock().expect("store").publishes(), 0);
    assert!(store.load(&key("map")).expect("load").is_none());
    assert_eq!(quota.0.lock().expect("quota").reserved(), 0);
    assert_eq!(quota.0.lock().expect("quota").committed(), 0);

    let mut prior = BYTES.to_vec();
    prior[8..12].copy_from_slice(&(u32::MAX - 1).to_le_bytes());
    let imported = session.import_legacy(&prior, "map", LegacyBodies::Include, LegacyLimits::default()).expect("import");
    assert_eq!(imported.version().major, u32::MAX - 1);
    let mut sink = VecSink::new();
    let report = session.pack(&PackRequest::new(), &mut sink).expect("pack to maximum");
    assert_eq!(report.version, format!("V{}", u32::MAX));
    assert!(report.major_reset);
    assert!(report.published);
    let digest = store.load(&key("map")).expect("load").expect("snapshot").digest();

    handle.lock().expect("source").insert_file("logo.png", b"\x89PNG\0\0\x01", 4).expect("mtime");
    let mut refused = VecSink::new();
    assert_eq!(
        session.pack(&PackRequest::new(), &mut refused),
        Err(DctxError::invalid("version exhausted"))
    );
    assert!(refused.is_empty());
    let stored = store.load(&key("map")).expect("load").expect("snapshot");
    assert_eq!(stored.version().major, u32::MAX);
    assert_eq!(stored.version().minor, 0);
    assert_eq!(stored.digest(), digest);
}
