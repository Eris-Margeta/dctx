// SPDX-License-Identifier: Apache-2.0
use std::sync::Arc;

use dctx_core::{SelectionPolicy, Timestamp};
use dctx_ports::memory::{CancelFlag, FixedClock, MemorySource};

use super::super::SnapshotHandle;
use super::*;
use crate::domain::{AutoCleanConfig, HostGrants, MapRequest, VecSink, Verb};
use crate::{HostCeiling, RuleTexts, StoragePolicy};

#[allow(
    clippy::field_reassign_with_default,
    reason = "Start from the public limit defaults and change only the limit exercised by this regression."
)]
fn open(source: impl crate::ports::Source + Send + 'static, cache: &std::path::Path) -> Session {
    let mut auto = AutoCleanConfig::default();
    auto.enabled = false;
    Session::builder(source)
        .clock(FixedClock(Timestamp(0)))
        .cancellation(Arc::new(CancelFlag::new()))
        .selection(SelectionPolicy::from_texts(&RuleTexts::default()))
        .ceiling(HostCeiling::base())
        .grants(HostGrants::PermitAll)
        .storage(StoragePolicy::Ephemeral {
            max_bytes: 8 * 1024 * 1024,
        })
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
    let handle = session
        .snapshot_reusable(&SnapshotRequest::strict(), "/src")
        .expect("cold");
    assert!(!handle.reused());
    assert!(session.content_reads() >= 1);
    assert_eq!(session.cached_content_reads(), 0);
    let cold_reads = session.content_reads();
    let kept = find_ids(&mut session, &handle, "kept");
    assert!(kept.iter().any(|id| id.contains("kept")), "{kept:?}");
    drop(handle);
    drop(session);

    let mut warm = open(source(b"fn kept() {}\n"), &cache);
    let handle = warm
        .snapshot_reusable(&SnapshotRequest::strict(), "/src")
        .expect("warm");
    assert!(handle.reused());
    assert_eq!(handle.generation(), 1);
    assert_eq!(warm.content_reads(), 0);
    assert!(warm.content_reads() < cold_reads);
    assert!(warm.cached_content_reads() >= 1);
    assert_eq!(find_ids(&mut warm, &handle, "kept"), kept);
    drop(handle);
    drop(warm);

    let mut third = open(source(b"fn lost() {}\n"), &cache);
    let handle = third
        .snapshot_reusable(&SnapshotRequest::strict(), "/src")
        .expect("third");
    assert!(!handle.reused());
    assert!(third.content_reads() >= 1);
    let lost = find_ids(&mut third, &handle, "lost");
    assert!(lost.iter().any(|id| id.contains("lost")), "{lost:?}");
    assert_eq!(
        find_ids(&mut third, &handle, "kept"),
        [] as [std::string::String; 0]
    );
}

#[test]
fn a_different_source_key_does_not_reuse() {
    let dir = tempfile::tempdir().expect("temp");
    let cache = dir.path().join("cache");
    std::fs::create_dir(&cache).expect("dir");
    let mut session = open(source(b"fn kept() {}\n"), &cache);
    let handle = session
        .snapshot_reusable(&SnapshotRequest::strict(), "/src")
        .expect("cold");
    assert!(!handle.reused());
    drop(handle);
    drop(session);

    let mut other = open(source(b"fn kept() {}\n"), &cache);
    let handle = other
        .snapshot_reusable(&SnapshotRequest::strict(), "/other")
        .expect("other");
    assert!(!handle.reused());
    assert!(other.content_reads() >= 1);
    assert_eq!(other.cached_content_reads(), 0);
}

#[allow(
    clippy::field_reassign_with_default,
    reason = "Start from the public limit defaults and change only the limit exercised by this regression."
)]
fn open_with_rules(
    source: MemorySource,
    cache: &std::path::Path,
    rules: crate::domain::RuleSources,
) -> Session {
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
        .storage(StoragePolicy::Ephemeral {
            max_bytes: 8 * 1024 * 1024,
        })
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
    let first = crate::domain::RuleSources {
        local_ignore: Some("# one\n".to_string()),
        ..crate::domain::RuleSources::default()
    };
    let mut session = open_with_rules(source(b"fn kept() {}\n"), &cache, first);
    let handle = session
        .snapshot_reusable(&SnapshotRequest::strict(), "/src")
        .expect("cold");
    assert!(!handle.reused());
    drop(handle);
    drop(session);

    let same = crate::domain::RuleSources {
        local_ignore: Some("# one\n".to_string()),
        ..crate::domain::RuleSources::default()
    };
    let mut warm = open_with_rules(source(b"fn kept() {}\n"), &cache, same);
    let handle = warm
        .snapshot_reusable(&SnapshotRequest::strict(), "/src")
        .expect("warm");
    assert!(handle.reused());
    assert_eq!(warm.content_reads(), 0);
    drop(handle);
    drop(warm);

    let edited = crate::domain::RuleSources {
        local_ignore: Some("# two\n".to_string()),
        ..crate::domain::RuleSources::default()
    };
    let mut third = open_with_rules(source(b"fn kept() {}\n"), &cache, edited);
    let handle = third
        .snapshot_reusable(&SnapshotRequest::strict(), "/src")
        .expect("edited");
    assert!(!handle.reused());
    assert!(third.content_reads() >= 1);
}

#[cfg(unix)]
mod stamp {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use dctx_core::error::DctxError as CoreError;
    use dctx_core::facts::EntryStamp;
    use dctx_core::facts::{EntryMeta, ReadGrant, SourceEntry};
    use dctx_core::ids::RelativePath;
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
            Self {
                tree,
                cache,
                _cache_dir: cache_dir,
                reads: Arc::new(AtomicU64::new(0)),
                bytes: Arc::new(AtomicU64::new(0)),
            }
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
            open(
                Counting {
                    inner,
                    reads: Arc::clone(&self.reads),
                    bytes: Arc::clone(&self.bytes),
                },
                &self.cache,
            )
        }

        fn served(&self) -> (u64, u64) {
            (
                self.reads.load(Ordering::Relaxed),
                self.bytes.load(Ordering::Relaxed),
            )
        }
    }

    fn set_age(path: &Path, seconds: u64) {
        let when = std::time::SystemTime::now() - std::time::Duration::from_secs(seconds);
        std::fs::File::options()
            .write(true)
            .open(path)
            .expect("open")
            .set_modified(when)
            .expect("set mtime");
    }

    fn snapshot(session: &mut Session) -> SnapshotHandle {
        session
            .snapshot_reusable(&SnapshotRequest::strict(), "/src")
            .expect("snapshot")
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
        let bytes = reader
            .load_retained()
            .expect("load")
            .expect("retained segment");
        let decoded = decode_generation(&bytes).expect("decode");
        let stamps: Vec<_> = decoded
            .generation
            .files
            .iter()
            .map(|file| (file.path.as_str(), file.stamp))
            .collect();
        assert_eq!(stamps.len(), 3);
        for (path, stamp) in &stamps {
            match *path {
                "c.rs" => assert_eq!(
                    *stamp, None,
                    "a file written just now is racy and carries no stamp"
                ),
                other => {
                    let stamp = stamp.unwrap_or_else(|| panic!("{other} must carry a stamp"));
                    let meta = std::fs::metadata(fixture.path(other)).expect("meta");
                    assert_eq!(stamp.size, meta.len());
                    assert_eq!(
                        stamp.modified_ns,
                        meta.modified()
                            .expect("mtime")
                            .duration_since(std::time::UNIX_EPOCH)
                            .expect("epoch")
                            .as_nanos()
                    );
                    assert_eq!(stamp.inode, std::os::unix::fs::MetadataExt::ino(&meta));
                }
            }
        }
        // The stamp never reaches the canonical payload: the segment digest is the same whatever the inode is.
        let again = encode_generation(
            "/src",
            "default",
            &crate::domain::resolve_scan(&crate::domain::OperationLimits::default())
                .expect("limits"),
            &decoded.generation,
        )
        .expect("encode");
        assert_eq!(
            again, bytes,
            "decode then encode reproduces the manifest bytes"
        );
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
        assert_eq!(
            fixture.served(),
            (0, 0),
            "an untouched settled tree is not read at all"
        );
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
        let modified = std::fs::metadata(fixture.path("a.rs"))
            .expect("meta")
            .modified()
            .expect("mtime");
        std::fs::write(fixture.path("a.tmp"), "fn alpha() {}\n").expect("copy");
        std::fs::File::options()
            .write(true)
            .open(fixture.path("a.tmp"))
            .expect("open")
            .set_modified(modified)
            .expect("mtime");
        std::fs::rename(fixture.path("a.tmp"), fixture.path("a.rs")).expect("replace");

        let mut warm = fixture.session();
        let handle = snapshot(&mut warm);
        assert!(handle.reused(), "same bytes: the generation survives");
        assert_eq!(
            warm.stamp_skipped_reads(),
            1,
            "b.rs is trusted by its stamp"
        );
        assert_eq!(
            warm.cached_content_reads(),
            1,
            "a.rs has a new inode and is hashed"
        );
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
        assert_eq!(
            warm.stamp_skipped_reads(),
            1,
            "only the settled file is trusted"
        );
        assert_eq!(
            warm.cached_content_reads(),
            2,
            "both files inside the racy window are hashed"
        );
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

mod robustness {
    use super::*;
    use proptest::prelude::*;
    use std::sync::OnceLock;

    pub(super) fn seed_for_regression() -> &'static [u8] {
        static SEED: OnceLock<Vec<u8>> = OnceLock::new();
        SEED.get_or_init(|| {
            let dir = tempfile::tempdir().unwrap();
            let cache = dir.path().join("cache");
            std::fs::create_dir(&cache).unwrap();
            let mut session = open(source(b"fn kept() {}\nfn caller() { kept(); }\n"), &cache);
            let handle = session
                .snapshot_reusable(&SnapshotRequest::strict(), "fixture")
                .unwrap();
            let bytes = session.load_retained().unwrap().unwrap();
            drop(handle);
            bytes
        })
    }

    fn check(bytes: &[u8]) {
        match decode_generation(bytes) {
            Ok(decoded) => {
                let generation = &decoded.generation;
                for node in &generation.nodes {
                    let mut id = String::new();
                    crate::index::write_node_id(&mut id, &generation.text, node);
                    let _ = crate::index::id_matches(&generation.text, node, &id);
                    let _ = generation.node_index(&id);
                }
                let limits =
                    crate::domain::resolve_scan(&crate::domain::OperationLimits::default())
                        .unwrap();
                let encoded =
                    encode_generation(&decoded.source_key, &decoded.view, &limits, generation)
                        .unwrap();
                assert!(decode_generation(&encoded).is_ok());
            }
            Err(error) => assert!(!error.to_string().is_empty()),
        }
    }

    proptest! {
        #[test]
        fn robustness_generation_decoder(bytes in prop::collection::vec(any::<u8>(), 0..4096)) {
            check(&bytes);
            let mut framed = MAGIC.to_vec();
            framed.extend_from_slice(&VERSION.to_le_bytes());
            framed.extend_from_slice(&bytes);
            check(&framed);
        }

        #[test]
        fn robustness_generation_mutations(
            changes in prop::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 1..8),
        ) {
            let mut bytes = seed_for_regression().to_vec();
            for (index, value) in changes {
                let at = index.index(bytes.len());
                bytes[at] = value;
            }
            check(&bytes);
        }
    }

    #[test]
    fn robustness_generation_prefixes_and_poisoned_counts() {
        let bytes = seed_for_regression();
        check(bytes);
        for cut in 0..bytes.len() {
            assert!(decode_generation(&bytes[..cut]).is_err(), "cut {cut}");
        }
        for at in 12..bytes.len().saturating_sub(8) {
            let mut poisoned = bytes.to_vec();
            poisoned[at..at + 8].fill(0xff);
            check(&poisoned);
        }
    }

    #[test]
    fn robustness_corrupt_generation_rebuilds_then_reuses() {
        for corrupt in [
            Vec::new(),
            b"dctxgen1\x04\0\0\0\xff\xff\xff\xff".to_vec(),
            vec![0xff; 512],
        ] {
            let dir = tempfile::tempdir().unwrap();
            let cache = dir.path().join("cache");
            std::fs::create_dir(&cache).unwrap();
            let mut session = open(source(b"fn kept() {}\n"), &cache);
            let handle = session
                .snapshot_reusable(&SnapshotRequest::strict(), "fixture")
                .unwrap();
            let retained = session.load_retained().unwrap().unwrap();
            drop(handle);
            drop(session);
            let mut replaced = 0;
            for file in std::fs::read_dir(cache.join("objects")).unwrap() {
                let path = file.unwrap().path();
                if std::fs::read(&path).unwrap() == retained {
                    std::fs::write(path, &corrupt).unwrap();
                    replaced += 1;
                }
            }
            assert_eq!(replaced, 1);
            assert!(decode_generation(&corrupt).is_err());
            let mut reopened = open(source(b"fn kept() {}\n"), &cache);
            let handle = reopened
                .snapshot_reusable(&SnapshotRequest::strict(), "fixture")
                .unwrap();
            assert!(!handle.reused());
            assert!(
                find_ids(&mut reopened, &handle, "kept")
                    .iter()
                    .any(|id| id.contains("kept"))
            );
            drop(handle);
            drop(reopened);
            let mut warm = open(source(b"fn kept() {}\n"), &cache);
            assert!(
                warm.snapshot_reusable(&SnapshotRequest::strict(), "fixture")
                    .unwrap()
                    .reused()
            );
        }
    }
}

#[test]
fn compact_strings_preserve_heading_ids_edges_and_exports() {
    let dir = tempfile::tempdir().expect("temp");
    let cache = dir.path().join("cache");
    std::fs::create_dir(&cache).expect("cache");
    let mut source = MemorySource::new();
    let heading = "Repeated heading Ω 1";
    for path in ["a.md", "b.md"] {
        source
            .insert_file(
                path,
                format!("# {heading}\n\nA short summary.\n\n```text\nbody-canary-not-cached\n```\n# {heading}\n\nA short summary.\n")
                    .as_bytes(),
                1,
            )
            .expect("doc");
    }
    let mut cold = open(source, &cache);
    let handle = cold
        .snapshot_reusable(&SnapshotRequest::strict(), "fixture")
        .expect("cold");
    let bytes = cold.load_retained().expect("load").expect("segment");
    let decoded = decode_generation(&bytes).expect("decode");
    assert_eq!(
        bytes
            .windows(heading.len())
            .filter(|value| *value == heading.as_bytes())
            .count(),
        1
    );
    assert!(
        !bytes
            .windows(b"body-canary-not-cached".len())
            .any(|value| value == b"body-canary-not-cached")
    );
    let original = cold.generation(&handle).expect("generation");
    assert_eq!(decoded.generation.files, original.files);
    for (saved, live) in decoded.generation.nodes.iter().zip(&original.nodes) {
        assert_eq!(
            (&saved.summary, &saved.signature, saved.ordinal),
            (&live.summary, &live.signature, live.ordinal)
        );
    }
    // Text spans may be relocated; compare the externally encoded graph instead.
    let format = crate::index::export::ExportFormat::parse("json").expect("format");
    assert_eq!(
        crate::index::export::export_graph(&decoded.generation, format),
        crate::index::export::export_graph(&original, format)
    );
    for end in 0..bytes.len() {
        assert!(decode_generation(&bytes[..end]).is_err(), "{end}");
    }
    let mut broken = bytes.clone();
    // First reference follows the dictionary. An invalid id is refused even with valid framing.
    let mut reader = Reader {
        rest: &bytes[MAGIC.len() + 4..],
        strings: Vec::new(),
    };
    reader.load_strings().expect("pool");
    let reference = bytes.len() - reader.rest.len();
    broken[reference..reference + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(decode_generation(&broken).is_err());
}

#[test]
fn an_old_generation_rebuilds_once_and_then_reuses() {
    let dir = tempfile::tempdir().expect("temp");
    let cache = dir.path().join("cache");
    std::fs::create_dir(&cache).expect("cache");
    let mut cold = open(source(b"fn kept() {}\n"), &cache);
    let handle = cold
        .snapshot_reusable(&SnapshotRequest::strict(), "fixture")
        .expect("cold");
    let ids = find_ids(&mut cold, &handle, "kept");
    drop(handle);
    let mut old = cold.load_retained().expect("load").expect("segment");
    old[MAGIC.len()..MAGIC.len() + 4].copy_from_slice(&3u32.to_le_bytes());
    drop(cold.store_cache_segment(&old).expect("publish old"));
    drop(cold);
    let mut rebuilt = open(source(b"fn kept() {}\n"), &cache);
    let handle = rebuilt
        .snapshot_reusable(&SnapshotRequest::strict(), "fixture")
        .expect("rebuild");
    assert!(!handle.reused());
    assert_eq!(find_ids(&mut rebuilt, &handle, "kept"), ids);
    drop(handle);
    drop(rebuilt);
    let mut warm = open(source(b"fn kept() {}\n"), &cache);
    let handle = warm
        .snapshot_reusable(&SnapshotRequest::strict(), "fixture")
        .expect("warm");
    assert!(handle.reused());
    assert_eq!(find_ids(&mut warm, &handle, "kept"), ids);
}

#[test]
fn a_previously_failed_read_is_rebuilt_instead_of_refreshing_unverified_metadata() {
    use dctx_core::error::DctxError as CoreError;
    use dctx_core::facts::{EntryMeta, ReadGrant, SourceEntry};
    use dctx_core::ids::RelativePath;
    use dctx_ports::Source;

    struct Unreadable(MemorySource);
    impl Source for Unreadable {
        fn root_kind(&self) -> Result<dctx_core::datatypes::NodeType, CoreError> {
            self.0.root_kind()
        }
        fn entries(&self, path: &RelativePath) -> Result<Vec<SourceEntry>, CoreError> {
            self.0.entries(path)
        }
        fn metadata(&self, path: &RelativePath) -> Result<EntryMeta, CoreError> {
            self.0.metadata(path)
        }
        fn read(&self, _: &ReadGrant, _: u64) -> Result<Vec<u8>, CoreError> {
            Err(CoreError::SourceUnavailable)
        }
    }
    let cache = tempfile::tempdir().expect("cache");
    let mut cold = open(Unreadable(source(b"fn kept() {}\n")), cache.path());
    let original = cold
        .snapshot_reusable(&SnapshotRequest::strict(), "fixture")
        .expect("partial");
    assert!(find_ids(&mut cold, &original, "kept").is_empty());
    drop(original);
    drop(cold);
    let mut warm = open(source(b"fn kept() {}\n"), cache.path());
    let rebuilt = warm
        .snapshot_reusable(&SnapshotRequest::strict(), "fixture")
        .expect("recovered");
    assert!(!rebuilt.reused());
    assert_eq!(find_ids(&mut warm, &rebuilt, "kept"), ["s:item.rs#kept"]);
}
