// SPDX-License-Identifier: Apache-2.0
//! Pack engine through its public API with spy ports: publish/diff/version flow, privacy (P1),
//! cancellation and quota refusals with positive controls, and replay determinism (P6).
//! Depends on dctx-pack, dctx-ports' in-memory doubles and dctx-core.
//! Run by `cargo test -p dctx-pack`.

use dctx_core::budget::PackLimits;
use dctx_core::diff::ChangeType;
use dctx_core::error::DctxError;
use dctx_core::ids::{Timestamp, ViewKey};
use dctx_core::selection::{RuleTexts, SelectionPolicy};
use dctx_core::version::Version;
use dctx_pack::{pack, PackPorts, PackReport, PackRequest};
use dctx_ports::memory::{CancelAfter, CancelFlag, FixedClock, MemoryCache, MemoryFactStore, MemorySource};
use dctx_ports::Cancellation;

struct Rig {
    store: MemoryFactStore,
    cache: MemoryCache,
    policy: SelectionPolicy,
}

impl Rig {
    fn new() -> Self {
        let texts = RuleTexts { local_ignore: Some(".env\nprivate/\n"), ..RuleTexts::default() };
        Self { store: MemoryFactStore::new(), cache: MemoryCache::new(1 << 20), policy: SelectionPolicy::from_texts(&texts) }
    }

    fn run_with(&mut self, source: &MemorySource, cancel: &dyn Cancellation) -> Result<PackReport, DctxError> {
        let request = PackRequest {
            view: ViewKey::parse("project").unwrap(),
            policy: &self.policy,
            debug_ignored: false,
            max_diff_files: 999,
            limits: PackLimits::unbounded(),
        };
        let ports = PackPorts {
            source,
            store: &mut self.store,
            cache: &mut self.cache,
            clock: &FixedClock(Timestamp(1_000)),
            cancel,
        };
        pack(ports, &request)
    }

    fn run(&mut self, source: &MemorySource) -> Result<PackReport, DctxError> {
        self.run_with(source, &CancelFlag::new())
    }
}

fn project() -> MemorySource {
    let mut source = MemorySource::new();
    source.insert_file("src/lib.rs", b"pub fn a() {}\n", 100).unwrap();
    source.insert_file("README.md", b"# demo\n", 100).unwrap();
    source.insert_file("Cargo.lock", b"[[package]]\n", 100).unwrap();
    source.insert_file(".env", b"SECRET=1\n", 100).unwrap();
    source.insert_file("private/notes.txt", b"do not read\n", 100).unwrap();
    source
}

#[test]
fn first_run_publishes_v1_and_reserves_read_bytes() {
    let mut rig = Rig::new();
    let report = rig.run(&project()).unwrap();
    assert!(report.published);
    assert!(report.major_reset);
    assert_eq!(report.previous_version, None);
    assert_eq!(report.snapshot.version(), &Version::new_v1());
    assert_eq!(report.snapshot.taken_at(), Timestamp(1_000));
    assert_eq!(rig.store.publishes(), 1);
    assert_eq!(rig.cache.committed(), 21);
    assert_eq!(rig.cache.reserved(), 0);
}

#[test]
fn unchanged_rerun_does_not_publish_and_change_bumps_minor() {
    let mut rig = Rig::new();
    let mut source = project();
    rig.run(&source).unwrap();
    let again = rig.run(&source).unwrap();
    assert!(!again.published);
    assert!(!again.diff.has_changes);
    assert_eq!(rig.store.publishes(), 1);

    source.insert_file("src/lib.rs", b"pub fn a() { 1 }\n", 200).unwrap();
    let changed = rig.run(&source).unwrap();
    assert!(changed.published);
    assert_eq!(changed.previous_version, Some(Version::new_v1()));
    assert_eq!(changed.snapshot.version(), &Version { major: 1, minor: 1 });
    assert_eq!(changed.diff.entries.len(), 1);
    assert_eq!(changed.diff.entries[0].change_type, ChangeType::Modified);
    assert_eq!(changed.diff.entries[0].relative_path, "src/lib.rs");
}

#[test]
fn p1_excluded_and_omitted_content_is_never_read_with_positive_twin() {
    let mut rig = Rig::new();
    let source = project();
    let report = rig.run(&source).unwrap();
    let mut reads = source.reads();
    reads.sort();
    assert_eq!(reads, ["README.md", "src/lib.rs"]);
    assert!(!source.touched(".env"));
    assert!(!source.touched("private"));
    assert!(source.metadata_requests().contains(&"Cargo.lock".to_string()));
    let names: Vec<&str> = report.snapshot.tree().children.iter().map(|c| c.relative_path.as_str()).collect();
    assert_eq!(names, ["Cargo.lock", "README.md", "src"]);
    assert!(report.snapshot.digests().contains_key("README.md"));
    assert!(!report.snapshot.digests().contains_key("Cargo.lock"));
}

#[test]
fn cancellation_before_commit_keeps_store_and_cache_unchanged() {
    let source = project();
    for allowed in 0..12 {
        let mut rig = Rig::new();
        let cancel = CancelAfter::new(allowed);
        match rig.run_with(&source, &cancel) {
            Err(error) => {
                assert_eq!(error, DctxError::Cancelled, "after {allowed} checks");
                assert_eq!(rig.store.publishes(), 0);
                assert_eq!((rig.cache.committed(), rig.cache.reserved()), (0, 0));
            }
            Ok(report) => {
                assert!(report.published, "positive control after {allowed} checks");
                assert_eq!(rig.store.publishes(), 1);
            }
        }
    }
    let mut rig = Rig::new();
    assert!(rig.run_with(&source, &CancelAfter::new(1_000)).is_ok());
}

#[test]
fn quota_and_store_refusals_abort_the_reservation() {
    let source = project();
    let mut rig = Rig { cache: MemoryCache::new(20), ..Rig::new() };
    assert_eq!(rig.run(&source).unwrap_err(), DctxError::CacheLimit { requested: 21, available: 20 });
    assert_eq!(rig.store.publishes(), 0);

    let mut rig = Rig::new();
    rig.store.fail_publishes();
    assert!(matches!(rig.run(&source), Err(DctxError::Io { .. })));
    assert_eq!((rig.cache.committed(), rig.cache.reserved()), (0, 0));

    let mut rig = Rig { cache: MemoryCache::new(21), ..Rig::new() };
    assert!(rig.run(&source).is_ok());
}

#[test]
fn p6_replay_gives_identical_snapshot_identity() {
    let first = Rig::new().run(&project()).unwrap();
    let second = Rig::new().run(&project()).unwrap();
    assert_eq!(first.snapshot.digest(), second.snapshot.digest());

    let mut edited = project();
    edited.insert_file("README.md", b"# demx\n", 100).unwrap();
    let third = Rig::new().run(&edited).unwrap();
    assert_ne!(first.snapshot.digest(), third.snapshot.digest());
}

#[test]
fn unreadable_items_become_diagnostics_not_failures() {
    let mut source = project();
    source.fail_reads_of("README.md");
    source.fail_listing_of("src");
    let report = Rig::new().run(&source).unwrap();
    assert_eq!(report.diagnostics.len(), 2);
    let readme = report.snapshot.tree().children.iter().find(|c| c.relative_path == "README.md").unwrap();
    assert!(readme.file_content.is_none());
    assert_eq!(readme.content_size, 7);
}
