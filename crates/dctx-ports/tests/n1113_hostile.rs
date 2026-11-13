// SPDX-License-Identifier: Apache-2.0
//! Deterministic hostile inputs for every port and in-memory implementation.
//! Bytes are opaque (including invalid UTF-8); typed paths reject invalid text before port calls.
//! Source has no file-size ceiling beyond the read argument; Cache uses its configured capacity.
//! Mutation methods deliberately change state: replay compares equivalent operation sequences.
//! MemorySource/CancelAfter are not Sync; concurrent access uses a Mutex or independent instances.

use std::path::PathBuf;
use std::sync::{Arc, Barrier, Mutex};

use dctx_core::budget::Reservation;
use dctx_core::datatypes::{DirContextTreeNode, NodeType};
use dctx_core::error::{DctxError, Resource};
use dctx_core::facts::{ContentDigests, PackSnapshot, ReadGrant, SourceEntry};
use dctx_core::ids::{RelativePath, Timestamp, ViewKey, MAX_PATH_BYTES};
use dctx_core::version::Version;
use dctx_core::FileHandlingDecision;
use dctx_ports::memory::{
    CancelAfter, CancelFlag, FixedClock, MemoryCache, MemoryFactStore, MemorySource, SourceCall,
};
use dctx_ports::source::{Inventory, InventoryEntry};
use dctx_ports::{Cache, Cancellation, Clock, FactStore, Source};

fn path(text: &str) -> RelativePath {
    RelativePath::parse(text).unwrap()
}
fn grant(text: &str) -> ReadGrant {
    ReadGrant::authorize(&path(text), FileHandlingDecision::Include, true).unwrap()
}
fn snapshot() -> PackSnapshot {
    let root = DirContextTreeNode::new(NodeType::Directory, "", PathBuf::new());
    PackSnapshot::new(
        root,
        Version::new_v1(),
        Timestamp(u64::MAX),
        ContentDigests::new(),
    )
    .unwrap()
}

#[test]
fn source_empty_defaults_and_reference_forwarding() {
    for source in [MemorySource::new(), MemorySource::default()] {
        let reference = &source;
        let forwarded = &reference;
        assert!(forwarded.inventory().is_none());
        forwarded.invalidate_inventory();
        assert_eq!(forwarded.root_kind().unwrap(), NodeType::Directory);
        assert_eq!(
            forwarded.kind(&RelativePath::root()).unwrap(),
            NodeType::Directory
        );
        assert!(forwarded.entries(&RelativePath::root()).unwrap().is_empty());
        let meta = forwarded.metadata(&RelativePath::root()).unwrap();
        assert_eq!((meta.size, meta.modified), (0, Timestamp(0)));
        assert_eq!(forwarded.stat(&RelativePath::root()).unwrap(), (meta, None));
        assert_eq!(
            forwarded.read(&grant("x"), 0),
            Err(DctxError::SourceUnavailable)
        );
        assert_eq!(
            forwarded.metadata(&path("x")),
            Err(DctxError::SourceUnavailable)
        );
        assert_eq!(
            forwarded.kind(&path("x")),
            Err(DctxError::SourceUnavailable)
        );
        assert_eq!(
            forwarded.entries(&path("x")),
            Err(DctxError::SourceUnavailable)
        );
    }
}

#[test]
fn source_every_byte_prefix_and_read_size_boundary() {
    let sample = b"\0a\xff\xc0\x80\xf0\x9f\x92\xa9\n";
    for end in 0..=sample.len() {
        let bytes = &sample[..end];
        let mut source = MemorySource::new();
        source.insert_file("x", bytes, u64::MAX).unwrap();
        for cap in [end as u64, u64::MAX] {
            assert_eq!(source.read(&grant("x"), cap).unwrap(), bytes);
            assert_eq!(source.read(&grant("x"), cap).unwrap(), bytes);
        }
        if end > 0 {
            assert_eq!(
                source.read(&grant("x"), end as u64 - 1),
                Err(DctxError::ResourceLimit {
                    resource: Resource::FileBytes,
                    limit: end as u64 - 1
                })
            );
        }
        let meta = source.metadata(&path("x")).unwrap();
        assert_eq!(
            (meta.size, meta.modified),
            (end as u64, Timestamp(u64::MAX))
        );
        assert_eq!(source.stat(&path("x")).unwrap(), (meta, None));
    }
}

#[test]
fn source_path_boundaries_and_failed_insertion_are_atomic() {
    let mut source = MemorySource::new();
    let long = "x".repeat(MAX_PATH_BYTES);
    source.insert_file(&long, b"x", 1).unwrap();
    assert_eq!(source.read(&grant(&long), 1).unwrap(), b"x");
    let long_dir = "d".repeat(MAX_PATH_BYTES);
    source.insert_dir(&long_dir, 1).unwrap();
    let before = source.entries(&RelativePath::root()).unwrap();
    for invalid in [
        "../x", "a/../x", "/x", "x/", "a//b", "a\\b", "a\0b", "a\nb", "a:b", ".", "..",
    ] {
        assert!(
            source.insert_file(invalid, b"secret", 0).is_err(),
            "{invalid:?}"
        );
        assert!(source.insert_dir(invalid, 0).is_err(), "{invalid:?}");
    }
    for too_long in [format!("{long}x"), format!("{long_dir}d")] {
        assert!(matches!(
            source.insert_file(&too_long, b"", 0),
            Err(DctxError::ResourceLimit { .. })
        ));
        assert!(matches!(
            source.insert_dir(&too_long, 0),
            Err(DctxError::ResourceLimit { .. })
        ));
    }
    assert!(source.insert_file("", b"", 0).is_err());
    assert_eq!(source.entries(&RelativePath::root()).unwrap(), before);
    source.insert_dir("", u64::MAX).unwrap();
    assert_eq!(
        source.metadata(&RelativePath::root()).unwrap().modified,
        Timestamp(u64::MAX)
    );
}

#[test]
fn source_truncated_unicode_paths_and_duplicate_reordering() {
    let sample = "dir/é.txt";
    for end in 0..=sample.len() {
        // String APIs cannot receive invalid UTF-8; reject those byte prefixes at the type boundary.
        if let Ok(prefix) = std::str::from_utf8(&sample.as_bytes()[..end]) {
            let mut source = MemorySource::new();
            if source.insert_file(prefix, b"\xff", 1).is_ok() {
                assert_eq!(source.read(&grant(prefix), 1).unwrap(), b"\xff");
            }
        }
    }
    let mut left = MemorySource::new();
    let mut right = MemorySource::new();
    for name in ["b", "dir/é", "a", "a"] {
        left.insert_file(name, b"x", 1).unwrap();
    }
    for name in ["a", "dir/é", "b"] {
        right.insert_file(name, b"x", 1).unwrap();
    }
    for dir in ["", "dir"] {
        assert_eq!(
            left.entries(&path(dir)).unwrap(),
            right.entries(&path(dir)).unwrap()
        );
        assert_eq!(
            left.entries(&path(dir)).unwrap(),
            left.entries(&path(dir)).unwrap()
        );
    }
    left.insert_file("a", b"replacement", 2).unwrap();
    assert_eq!(left.read(&grant("a"), u64::MAX).unwrap(), b"replacement");
}

#[test]
fn source_rejects_file_directory_collisions_without_partial_parents() {
    let mut source = MemorySource::new();
    source.insert_file("file", b"kept", 7).unwrap();
    source.insert_dir("dir", 8).unwrap();
    let before = source.entries(&RelativePath::root()).unwrap();
    for name in ["file", "file/new", "file/new/deep"] {
        assert!(source.insert_dir(name, 9).is_err(), "directory {name}");
    }
    for name in ["dir", "file/new", "file/new/deep"] {
        assert!(
            source.insert_file(name, b"changed", 9).is_err(),
            "file {name}"
        );
    }
    assert_eq!(source.entries(&RelativePath::root()).unwrap(), before);
    assert_eq!(source.kind(&path("file")).unwrap(), NodeType::File);
    assert_eq!(source.kind(&path("dir")).unwrap(), NodeType::Directory);
    assert_eq!(source.read(&grant("file"), 4).unwrap(), b"kept");
    assert_eq!(
        source.metadata(&path("dir")).unwrap().modified,
        Timestamp(8)
    );
}

#[test]
fn source_spies_and_failure_injection_repeat_exactly() {
    let mut source = MemorySource::new();
    assert!(!source.touched(""));
    source.root_kind().unwrap();
    assert!(source.touched(""));
    source.insert_file("dir/x", b"x", 0).unwrap();
    source.entries(&path("dir")).unwrap();
    source.metadata(&path("dir/x")).unwrap();
    source.read(&grant("dir/x"), 1).unwrap();
    assert_eq!(
        source.calls(),
        vec![
            SourceCall::RootKind,
            SourceCall::Entries("dir".into()),
            SourceCall::Metadata("dir/x".into()),
            SourceCall::Read("dir/x".into())
        ]
    );
    assert_eq!(source.reads(), ["dir/x"]);
    assert_eq!(source.listed(), ["dir"]);
    assert_eq!(source.metadata_requests(), ["dir/x"]);
    assert!(source.touched("dir"));
    assert!(!source.touched("di"));
    assert_eq!(source.calls(), source.calls());
    source.fail_reads_of("dir/x");
    source.fail_reads_of("dir/x");
    source.fail_listing_of("dir");
    source.fail_listing_of("dir");
    assert_eq!(
        source.read(&grant("dir/x"), 1),
        source.read(&grant("dir/x"), 1)
    );
    assert!(matches!(
        source.read(&grant("dir/x"), 1),
        Err(DctxError::Io { .. })
    ));
    assert_eq!(source.entries(&path("dir")), source.entries(&path("dir")));
    assert!(source.entries(&path("dir")).is_err());
    for text in ["", "x", "\0", &"x".repeat(MAX_PATH_BYTES + 1)] {
        source.fail_reads_of(text);
        source.fail_listing_of(text);
        source.touched(text);
    }
}

// Exercise Source defaults independently from MemorySource's kind override.
struct DefaultSource(MemorySource);
impl Source for DefaultSource {
    fn root_kind(&self) -> Result<NodeType, DctxError> {
        self.0.root_kind()
    }
    fn entries(&self, dir: &RelativePath) -> Result<Vec<SourceEntry>, DctxError> {
        self.0.entries(dir)
    }
    fn metadata(&self, p: &RelativePath) -> Result<dctx_core::facts::EntryMeta, DctxError> {
        self.0.metadata(p)
    }
    fn read(&self, g: &ReadGrant, max: u64) -> Result<Vec<u8>, DctxError> {
        self.0.read(g, max)
    }
}
struct Traversal;
impl Inventory for Traversal {
    fn entries(&mut self, _: &RelativePath) -> Result<Vec<InventoryEntry>, DctxError> {
        Ok(vec![InventoryEntry {
            entry: SourceEntry::file("x"),
            stat: None,
        }])
    }
}
struct InventorySource(DefaultSource);
impl Source for InventorySource {
    fn inventory(&self) -> Option<Box<dyn Inventory + '_>> {
        Some(Box::new(Traversal))
    }
    fn invalidate_inventory(&self) {
        self.0.invalidate_inventory();
    }
    fn root_kind(&self) -> Result<NodeType, DctxError> {
        self.0.root_kind()
    }
    fn entries(&self, dir: &RelativePath) -> Result<Vec<SourceEntry>, DctxError> {
        self.0.entries(dir)
    }
    fn metadata(&self, p: &RelativePath) -> Result<dctx_core::facts::EntryMeta, DctxError> {
        self.0.metadata(p)
    }
    fn read(&self, g: &ReadGrant, max: u64) -> Result<Vec<u8>, DctxError> {
        self.0.read(g, max)
    }
}

#[test]
fn source_default_kind_stat_inventory_and_reference_dispatch() {
    let mut memory = MemorySource::new();
    memory.insert_file("dir/x", b"x", 0).unwrap();
    let source = DefaultSource(memory);
    assert_eq!(source.kind(&path("")).unwrap(), NodeType::Directory);
    assert_eq!(source.kind(&path("dir/x")).unwrap(), NodeType::File);
    assert_eq!(
        source.kind(&path("missing")),
        Err(DctxError::SourceUnavailable)
    );
    assert_eq!(source.stat(&path("dir/x")).unwrap().1, None);
    let inventory_source = InventorySource(source);
    let borrowed = &inventory_source;
    let forwarded = &borrowed;
    forwarded.invalidate_inventory();
    let mut inventory = forwarded.inventory().unwrap();
    for dir in ["", "x", &"x".repeat(MAX_PATH_BYTES)] {
        let entries = inventory.entries(&path(dir)).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].clone().entry, SourceEntry::file("x"));
        assert!(entries[0].stat.is_none());
    }
}

#[test]
fn cache_zero_one_max_and_over_capacity() {
    for capacity in [0, 1, 7, u64::MAX] {
        let mut cache = MemoryCache::new(capacity);
        let empty = cache.reserve(0).unwrap();
        cache.commit(empty, 0).unwrap();
        let reservation = cache.reserve(capacity).unwrap();
        assert_eq!(cache.reserved(), capacity);
        assert_eq!(
            cache.reserve(1),
            Err(DctxError::CacheLimit {
                requested: 1,
                available: 0
            })
        );
        cache.commit(reservation, capacity).unwrap();
        assert_eq!((cache.committed(), cache.reserved()), (capacity, 0));
    }
    let mut cache = MemoryCache::new(u64::MAX - 1);
    assert!(cache.reserve(u64::MAX).is_err());
    assert_eq!((cache.committed(), cache.reserved()), (0, 0));
}

#[test]
fn cache_failed_commit_preserves_reservation_and_settles_once() {
    let mut cache = MemoryCache::new(5);
    let first = cache.reserve(3).unwrap();
    let second = cache.reserve(2).unwrap();
    let first_id = first.id();
    let second_id = second.id();
    assert!(cache.commit(first, 4).is_err());
    assert_eq!(cache.reserved(), 5);
    assert!(cache
        .commit(Reservation::new(u64::MAX, u64::MAX), 1)
        .is_err());
    assert_eq!(cache.reserved(), 5);
    cache.abort(second);
    cache.abort(Reservation::new(second_id, 2));
    cache.commit(Reservation::new(first_id, 3), 2).unwrap();
    assert!(cache.commit(Reservation::new(first_id, 3), 0).is_err());
    cache.abort(Reservation::new(first_id, 3));
    assert_eq!((cache.committed(), cache.reserved()), (2, 0));
}

#[test]
fn cache_reordered_settlement_replays_byte_identically() {
    fn replay(reverse: bool) -> Vec<u8> {
        let mut cache = MemoryCache::new(10);
        let a = cache.reserve(4).unwrap();
        let b = cache.reserve(6).unwrap();
        if reverse {
            cache.abort(b);
            cache.commit(a, 3).unwrap();
        } else {
            cache.commit(a, 3).unwrap();
            cache.abort(b);
        }
        [
            cache.committed().to_le_bytes(),
            cache.reserved().to_le_bytes(),
        ]
        .concat()
    }
    assert_eq!(replay(false), replay(true));
    assert_eq!(replay(false), replay(false));
}

#[test]
fn fact_store_key_boundaries_duplicate_publish_and_failure_atomicity() {
    let mut store = MemoryFactStore::new();
    let snapshot = snapshot();
    for key in ["v".to_string(), "é".to_string(), "k".repeat(MAX_PATH_BYTES)] {
        let view = ViewKey::parse(&key).unwrap();
        assert!(store.load(&view).unwrap().is_none());
        store.publish(&view, &snapshot).unwrap();
        store.publish(&view, &snapshot).unwrap();
        let loaded = store.load(&view).unwrap().unwrap();
        assert_eq!(loaded.digest(), snapshot.digest());
        assert_eq!(
            loaded.digest(),
            store.load(&view).unwrap().unwrap().digest()
        );
        assert_eq!(loaded.taken_at(), Timestamp(u64::MAX));
    }
    assert_eq!(store.publishes(), 6);
    for end in 0..="é-view".len() {
        if let Ok(prefix) = std::str::from_utf8(&"é-view".as_bytes()[..end]) {
            if let Ok(view) = ViewKey::parse(prefix) {
                store.load(&view).unwrap();
            }
        }
    }
    for invalid in [
        "".to_string(),
        "\0".to_string(),
        "x".repeat(MAX_PATH_BYTES + 1),
    ] {
        assert!(ViewKey::parse(&invalid).is_err());
    }
    store.fail_publishes();
    store.fail_publishes();
    assert!(store
        .publish(&ViewKey::parse("new").unwrap(), &snapshot)
        .is_err());
    assert!(store
        .load(&ViewKey::parse("new").unwrap())
        .unwrap()
        .is_none());
    assert_eq!(store.publishes(), 6);
    assert_eq!(
        store
            .load(&ViewKey::parse("v").unwrap())
            .unwrap()
            .unwrap()
            .digest(),
        snapshot.digest()
    );
    assert_eq!(MemoryFactStore::default().publishes(), 0);
}

#[test]
fn fixed_clock_and_cancellation_extremes() {
    for value in [0, 1, u64::MAX] {
        let clock = FixedClock(Timestamp(value));
        assert_eq!(clock.now(), Timestamp(value));
        assert_eq!(clock.now(), clock.now());
    }
    for allowed in [0, 1, 3] {
        let cancel = CancelAfter::new(allowed);
        assert_eq!(cancel.checks(), 0);
        for _ in 0..allowed {
            assert!(cancel.checkpoint().is_ok());
        }
        assert_eq!(cancel.checkpoint(), Err(DctxError::Cancelled));
        assert!(cancel.is_cancelled());
        assert_eq!(cancel.checks(), allowed + 2);
    }
    let cancel = CancelAfter::new(u64::MAX);
    assert!(cancel.checkpoint().is_ok());
    assert_eq!(cancel.checks(), 1);
    for flag in [CancelFlag::new(), CancelFlag::default()] {
        assert!(!flag.is_cancelled());
        flag.cancel();
        flag.cancel();
        assert!(flag.is_cancelled());
        assert_eq!(flag.checkpoint(), Err(DctxError::Cancelled));
    }
}

#[test]
fn concurrent_cancellation_and_port_access_use_explicit_synchronization() {
    let flag = Arc::new(CancelFlag::new());
    let barrier = Arc::new(Barrier::new(5));
    let mut memory = MemorySource::new();
    memory.insert_file("x", b"\xff", 0).unwrap();
    let source = Mutex::new(memory);
    let cache = Mutex::new(MemoryCache::new(4));
    let store = MemoryFactStore::new();
    let view = ViewKey::parse("v").unwrap();
    let clock = FixedClock(Timestamp(u64::MAX));
    std::thread::scope(|scope| {
        for _ in 0..4 {
            let flag = Arc::clone(&flag);
            let barrier = Arc::clone(&barrier);
            let (source, cache, store, view, clock) = (&source, &cache, &store, &view, &clock);
            scope.spawn(move || {
                barrier.wait();
                assert!(flag.is_cancelled());
                assert_eq!(flag.checkpoint(), Err(DctxError::Cancelled));
                assert_eq!(clock.now(), Timestamp(u64::MAX));
                assert!(store.load(view).unwrap().is_none());
                assert_eq!(
                    source.lock().unwrap().read(&grant("x"), 1).unwrap(),
                    b"\xff"
                );
                let mut cache = cache.lock().unwrap();
                let reservation = cache.reserve(1).unwrap();
                cache.commit(reservation, 1).unwrap();
            });
        }
        flag.cancel();
        barrier.wait();
    });
    assert_eq!(cache.into_inner().unwrap().committed(), 4);
    assert_eq!(source.into_inner().unwrap().reads(), ["x", "x", "x", "x"]);
}

#[test]
fn port_refusals_include_corrective_steps() {
    let mut cache = MemoryCache::new(0);
    let mut source = MemorySource::new();
    let missing = source.read(&grant("missing"), 1).unwrap_err();
    source.insert_file("x", b"xx", 0).unwrap();
    let limited = source.read(&grant("x"), 1).unwrap_err();
    let invalid = source.insert_file("../x", b"", 0).unwrap_err();
    source.fail_reads_of("x");
    let injected = source.read(&grant("x"), 2).unwrap_err();
    let errors = [
        cache.reserve(1).unwrap_err(),
        missing,
        limited,
        invalid,
        injected,
        CancelAfter::new(0).checkpoint().unwrap_err(),
    ];
    for error in errors {
        let message = error.to_string();
        let (_, step) = message.split_once("; ").expect("corrective step");
        assert!(step.split_whitespace().count() >= 4, "{message}");
        assert_eq!(message, error.to_string());
    }
}
