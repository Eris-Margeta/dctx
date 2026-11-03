// SPDX-License-Identifier: Apache-2.0
use std::sync::Arc;

use dctx_core::{SelectionPolicy, Timestamp};
use dctx_ports::memory::{CancelFlag, FixedClock, MemorySource};

use super::super::SnapshotHandle;
use super::*;
use crate::domain::{AutoCleanConfig, HostGrants, MapRequest, VecSink, Verb};
use crate::{HostCeiling, RuleTexts, StoragePolicy};

#[allow(clippy::field_reassign_with_default, reason = "Start from the public limit defaults and change only the limit exercised by this regression.")]
fn open(source: impl crate::ports::Source + Send + 'static, cache: &std::path::Path) -> Session {
    let mut auto = AutoCleanConfig::default();
    auto.enabled = false;
    Session::builder(source)
        .clock(FixedClock(Timestamp(0)))
        .cancellation(Arc::new(CancelFlag::new()))
        .selection(SelectionPolicy::from_texts(&RuleTexts::default()))
        .ceiling(HostCeiling::base())
        .grants(HostGrants::PermitAll)
        .storage(StoragePolicy::Ephemeral { max_bytes: 8 * 1024 * 1024 })
        .place("fixture")
        .view("default")
        .cache_root(cache, 1)
        .auto_clean(auto)
        .build()
        .expect("session")
}

fn source(body: &[u8]) -> MemorySource {
    let mut source = MemorySource::new();
    source.insert_file("item.rs", body, 9).expect("file");
    source
}

fn find_ids(session: &mut Session, handle: &SnapshotHandle, query: &str) -> Vec<String> {
    let mut request = MapRequest::new(Verb::Find);
    request.query = query.to_string();
    let mut sink = VecSink::new();
    let answer = session.query(handle, &request, &mut sink).expect("query");
    answer.records.into_iter().map(|record| record.id).collect()
}

#[test]
fn same_stamp_byte_change_rebuilds_and_a_match_reuses() {
    let dir = tempfile::tempdir().expect("temp");
    let cache = dir.path().join("cache");
    std::fs::create_dir(&cache).expect("dir");
    let mut session = open(source(b"fn kept() {}\n"), &cache);
    let handle = session.snapshot_reusable(&SnapshotRequest::strict(), "/src").expect("cold");
    assert!(!handle.reused());
    assert!(session.content_reads() >= 1);
    assert_eq!(session.cached_content_reads(), 0);
    let cold_reads = session.content_reads();
    let kept = find_ids(&mut session, &handle, "kept");
    assert!(kept.iter().any(|id| id.contains("kept")), "{kept:?}");
    drop(handle);
    drop(session);

    let mut warm = open(source(b"fn kept() {}\n"), &cache);
    let handle = warm.snapshot_reusable(&SnapshotRequest::strict(), "/src").expect("warm");
    assert!(handle.reused());
    assert_eq!(handle.generation(), 1);
    assert_eq!(warm.content_reads(), 0);
    assert!(warm.content_reads() < cold_reads);
    assert!(warm.cached_content_reads() >= 1);
    assert_eq!(find_ids(&mut warm, &handle, "kept"), kept);
    drop(handle);
    drop(warm);

    let mut third = open(source(b"fn lost() {}\n"), &cache);
    let handle = third.snapshot_reusable(&SnapshotRequest::strict(), "/src").expect("third");
    assert!(!handle.reused());
    assert!(third.content_reads() >= 1);
    let lost = find_ids(&mut third, &handle, "lost");
    assert!(lost.iter().any(|id| id.contains("lost")), "{lost:?}");
    assert_eq!(find_ids(&mut third, &handle, "kept"), [] as [std::string::String; 0]);
}

#[test]
fn a_different_source_key_does_not_reuse() {
    let dir = tempfile::tempdir().expect("temp");
    let cache = dir.path().join("cache");
    std::fs::create_dir(&cache).expect("dir");
    let mut session = open(source(b"fn kept() {}\n"), &cache);
    let handle = session.snapshot_reusable(&SnapshotRequest::strict(), "/src").expect("cold");
    assert!(!handle.reused());
    drop(handle);
    drop(session);

    let mut other = open(source(b"fn kept() {}\n"), &cache);
    let handle = other.snapshot_reusable(&SnapshotRequest::strict(), "/other").expect("other");
    assert!(!handle.reused());
    assert!(other.content_reads() >= 1);
    assert_eq!(other.cached_content_reads(), 0);
}

#[allow(clippy::field_reassign_with_default, reason = "Start from the public limit defaults and change only the limit exercised by this regression.")]
fn open_with_rules(source: MemorySource, cache: &std::path::Path, rules: crate::domain::RuleSources) -> Session {
    let mut auto = AutoCleanConfig::default();
    auto.enabled = false;
    let selection = rules.policy();
    Session::builder(source)
        .clock(FixedClock(Timestamp(0)))
        .cancellation(Arc::new(CancelFlag::new()))
        .selection(selection)
        .rule_sources(rules)
        .ceiling(HostCeiling::base())
        .grants(HostGrants::PermitAll)
        .storage(StoragePolicy::Ephemeral { max_bytes: 8 * 1024 * 1024 })
        .place("fixture")
        .view("default")
        .cache_root(cache, 1)
        .auto_clean(auto)
        .build()
        .expect("session")
}

#[test]
fn a_comment_only_rule_edit_rebuilds_and_the_same_text_reuses() {
    let dir = tempfile::tempdir().expect("temp");
    let cache = dir.path().join("cache");
    std::fs::create_dir(&cache).expect("dir");
    let first = crate::domain::RuleSources { local_ignore: Some("# one\n".to_string()), ..crate::domain::RuleSources::default() };
    let mut session = open_with_rules(source(b"fn kept() {}\n"), &cache, first);
    let handle = session.snapshot_reusable(&SnapshotRequest::strict(), "/src").expect("cold");
    assert!(!handle.reused());
    drop(handle);
    drop(session);

    let same = crate::domain::RuleSources { local_ignore: Some("# one\n".to_string()), ..crate::domain::RuleSources::default() };
    let mut warm = open_with_rules(source(b"fn kept() {}\n"), &cache, same);
    let handle = warm.snapshot_reusable(&SnapshotRequest::strict(), "/src").expect("warm");
    assert!(handle.reused());
    assert_eq!(warm.content_reads(), 0);
    drop(handle);
    drop(warm);

    let edited = crate::domain::RuleSources { local_ignore: Some("# two\n".to_string()), ..crate::domain::RuleSources::default() };
    let mut third = open_with_rules(source(b"fn kept() {}\n"), &cache, edited);
    let handle = third.snapshot_reusable(&SnapshotRequest::strict(), "/src").expect("edited");
    assert!(!handle.reused());
    assert!(third.content_reads() >= 1);
}

#[cfg(unix)]
mod stamp {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use dctx_core::error::DctxError as CoreError;
    use dctx_core::facts::{EntryMeta, ReadGrant, SourceEntry};
    use dctx_core::ids::RelativePath;
    use dctx_core::facts::EntryStamp;
    use dctx_ports::Source;

    use super::*;
    use crate::adapters::FsSource;

    /// The real filesystem source, counting every content read and byte it serves.
    struct Counting {
        inner: FsSource,
        reads: Arc<AtomicU64>,
        bytes: Arc<AtomicU64>,
    }

    impl Source for Counting {
        fn root_kind(&self) -> Result<dctx_core::datatypes::NodeType, CoreError> {
            self.inner.root_kind()
        }

        fn entries(&self, dir: &RelativePath) -> Result<Vec<SourceEntry>, CoreError> {
            self.inner.entries(dir)
        }

        fn metadata(&self, path: &RelativePath) -> Result<EntryMeta, CoreError> {
            self.inner.metadata(path)
        }

        fn stat(&self, path: &RelativePath) -> Result<(EntryMeta, Option<EntryStamp>), CoreError> {
            self.inner.stat(path)
        }

        fn read(&self, grant: &ReadGrant, max_bytes: u64) -> Result<Vec<u8>, CoreError> {
            let bytes = self.inner.read(grant, max_bytes)?;
            self.reads.fetch_add(1, Ordering::Relaxed);
            self.bytes.fetch_add(bytes.len() as u64, Ordering::Relaxed);
            Ok(bytes)
        }
    }

    struct Fixture {
        tree: tempfile::TempDir,
        cache: PathBuf,
        _cache_dir: tempfile::TempDir,
        reads: Arc<AtomicU64>,
        bytes: Arc<AtomicU64>,
    }

    impl Fixture {
        fn new() -> Self {
            let tree = tempfile::tempdir().expect("tree");
            let cache_dir = tempfile::tempdir().expect("cache");
            let cache = cache_dir.path().join("cache");
            std::fs::create_dir(&cache).expect("cache dir");
            Self { tree, cache, _cache_dir: cache_dir, reads: Arc::new(AtomicU64::new(0)), bytes: Arc::new(AtomicU64::new(0)) }
        }

        fn path(&self, name: &str) -> PathBuf {
            self.tree.path().join(name)
        }

        /// Writes `body` and sets an mtime `age` seconds in the past, so the file is outside the racy window.
        fn write(&self, name: &str, body: &str, age: u64) {
            std::fs::write(self.path(name), body).expect("write");
            set_age(&self.path(name), age);
        }

        fn session(&self) -> Session {
            self.reads.store(0, Ordering::Relaxed);
            self.bytes.store(0, Ordering::Relaxed);
            let inner = FsSource::open(self.tree.path()).expect("open");
            open(Counting { inner, reads: Arc::clone(&self.reads), bytes: Arc::clone(&self.bytes) }, &self.cache)
        }

        fn served(&self) -> (u64, u64) {
            (self.reads.load(Ordering::Relaxed), self.bytes.load(Ordering::Relaxed))
        }
    }

    fn set_age(path: &Path, seconds: u64) {
        let when = std::time::SystemTime::now() - std::time::Duration::from_secs(seconds);
        std::fs::File::options().write(true).open(path).expect("open").set_modified(when).expect("set mtime");
    }

    fn snapshot(session: &mut Session) -> SnapshotHandle {
        session.snapshot_reusable(&SnapshotRequest::strict(), "/src").expect("snapshot")
    }

    #[test]
    fn manifest_stamps_round_trip_and_only_settled_files_carry_one() {
        let fixture = Fixture::new();
        fixture.write("a.rs", "fn alpha() {}\n", 3600);
        fixture.write("b.rs", "fn bravo() {}\n", 7200);
        std::fs::write(fixture.path("c.rs"), "fn charlie() {}\n").expect("fresh");
        let mut cold = fixture.session();
        drop(snapshot(&mut cold));
        drop(cold);

        let mut reader = fixture.session();
        let bytes = reader.load_retained().expect("load").expect("retained segment");
        let decoded = decode_generation(&bytes).expect("decode");
        let stamps: Vec<_> = decoded.generation.files.iter().map(|file| (file.path.as_str(), file.stamp)).collect();
        assert_eq!(stamps.len(), 3);
        for (path, stamp) in &stamps {
            match *path {
                "c.rs" => assert_eq!(*stamp, None, "a file written just now is racy and carries no stamp"),
                other => {
                    let stamp = stamp.unwrap_or_else(|| panic!("{other} must carry a stamp"));
                    let meta = std::fs::metadata(fixture.path(other)).expect("meta");
                    assert_eq!(stamp.size, meta.len());
                    assert_eq!(stamp.modified_ns, meta.modified().expect("mtime").duration_since(std::time::UNIX_EPOCH).expect("epoch").as_nanos());
                    assert_eq!(stamp.inode, std::os::unix::fs::MetadataExt::ino(&meta));
                }
            }
        }
        // The stamp never reaches the canonical payload: the segment digest is the same whatever the inode is.
        let again = encode_generation("/src", "default", &crate::domain::resolve_scan(&crate::domain::OperationLimits::default()).expect("limits"), &decoded.generation)
            .expect("encode");
        assert_eq!(again, bytes, "decode then encode reproduces the manifest bytes");
    }

    #[test]
    fn reuse_stamp_untouched_tree_hashes_zero_bytes_and_an_edit_is_seen() {
        let fixture = Fixture::new();
        fixture.write("a.rs", "fn alpha() {}\n", 3600);
        fixture.write("b.rs", "fn bravo() {}\n", 3600);
        let mut cold = fixture.session();
        drop(snapshot(&mut cold));
        let (cold_reads, cold_bytes) = fixture.served();
        assert_eq!(cold_reads, 2);
        assert!(cold_bytes > 0);
        drop(cold);

        let mut warm = fixture.session();
        let handle = snapshot(&mut warm);
        assert!(handle.reused());
        assert_eq!(fixture.served(), (0, 0), "an untouched settled tree is not read at all");
        assert_eq!(warm.stamp_skipped_reads(), 2);
        assert_eq!(warm.cached_content_reads(), 0);
        assert_eq!(find_ids(&mut warm, &handle, "alpha").len(), 1);
        drop(handle);
        drop(warm);

        // Same size, new bytes, new mtime: the stamp differs, so the generation is rebuilt and answers the new symbol.
        let edited = "fn omega() {}\n";
        assert_eq!(edited.len(), "fn alpha() {}\n".len());
        std::fs::write(fixture.path("a.rs"), edited).expect("edit");
        let mut third = fixture.session();
        let handle = snapshot(&mut third);
        assert!(!handle.reused());
        assert_eq!(find_ids(&mut third, &handle, "omega").len(), 1);
        assert!(find_ids(&mut third, &handle, "alpha").is_empty());
    }

    #[test]
    fn reuse_stamp_inode_change_is_digested_not_trusted() {
        let fixture = Fixture::new();
        fixture.write("a.rs", "fn alpha() {}\n", 3600);
        fixture.write("b.rs", "fn bravo() {}\n", 3600);
        let mut cold = fixture.session();
        drop(snapshot(&mut cold));
        drop(cold);

        // A new file with the same bytes, size and nanosecond mtime, moved over the old one: only the inode differs.
        let modified = std::fs::metadata(fixture.path("a.rs")).expect("meta").modified().expect("mtime");
        std::fs::write(fixture.path("a.tmp"), "fn alpha() {}\n").expect("copy");
        std::fs::File::options().write(true).open(fixture.path("a.tmp")).expect("open").set_modified(modified).expect("mtime");
        std::fs::rename(fixture.path("a.tmp"), fixture.path("a.rs")).expect("replace");

        let mut warm = fixture.session();
        let handle = snapshot(&mut warm);
        assert!(handle.reused(), "same bytes: the generation survives");
        assert_eq!(warm.stamp_skipped_reads(), 1, "b.rs is trusted by its stamp");
        assert_eq!(warm.cached_content_reads(), 1, "a.rs has a new inode and is hashed");
        assert_eq!(fixture.served().0, 1);
    }

    #[test]
    fn reuse_racy_recent_files_are_always_digested() {
        let fixture = Fixture::new();
        std::fs::write(fixture.path("a.rs"), "fn alpha() {}\n").expect("a");
        std::fs::write(fixture.path("b.rs"), "fn bravo() {}\n").expect("b");
        fixture.write("old.rs", "fn old() {}\n", 3600);
        let mut cold = fixture.session();
        drop(snapshot(&mut cold));
        drop(cold);

        let mut warm = fixture.session();
        let handle = snapshot(&mut warm);
        assert!(handle.reused());
        assert_eq!(warm.stamp_skipped_reads(), 1, "only the settled file is trusted");
        assert_eq!(warm.cached_content_reads(), 2, "both files inside the racy window are hashed");
        assert_eq!(fixture.served().0, 2);
    }

    #[test]
    fn reuse_racy_freshness_switch_parses_and_refuses_unknown_values() {
        assert!(stamp_pass_mode(None).expect("default"));
        assert!(stamp_pass_mode(Some("")).expect("empty"));
        assert!(stamp_pass_mode(Some("stamp")).expect("stamp"));
        assert!(!stamp_pass_mode(Some("strict_live")).expect("strict"));
        let error = stamp_pass_mode(Some("strict-live")).expect_err("typo");
        assert_eq!(error.code().as_str(), "InvalidRequest");
        assert!(error.to_string().contains("DCTX_FRESHNESS"), "{error}");
    }
}
