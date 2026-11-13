// SPDX-License-Identifier: Apache-2.0
//! Deterministic hostile inputs through every public dctx-pack operation.
//! Byte APIs receive malformed wire data; typed APIs receive equivalent source/tree facts.
//! All sources, quotas and timestamps are in memory; no filesystem or wall clock is used.

#![forbid(unsafe_code)]

use dctx_core::budget::PackLimits;
use dctx_core::datatypes::NodeType;
use dctx_core::error::{DctxError, Resource};
use dctx_core::facts::{
    ContentDigests, Diagnostic, EntryMeta, PackSnapshot, ReadGrant, SourceEntry,
};
use dctx_core::ids::{RelativePath, Timestamp, ViewKey, MAX_PATH_BYTES};
use dctx_core::selection::{RuleTexts, SelectionPolicy};
use dctx_core::version::Version;
use dctx_pack::legacy_codec::{revision_can_advance, LEGACY_MAX_DEPTH};
use dctx_pack::{
    decode_legacy, diff_against, encode_legacy, encode_legacy_to, pack, read_snapshot_file, scan,
    scan_spooled, LegacyBodies, LegacyError, LegacyLimits, LegacyNode, PackPorts, PackReport,
    PackRequest, Scan, ScanOptions,
};
use dctx_ports::memory::{CancelFlag, FixedClock, MemoryCache, MemoryFactStore, MemorySource};
use dctx_ports::{FactStore, Source};
use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::PathBuf;

const MODES: [LegacyBodies; 2] = [LegacyBodies::MetadataOnly, LegacyBodies::Include];

fn node(kind: NodeType, name: &str) -> LegacyNode {
    LegacyNode::new(kind, name, PathBuf::new())
}

fn file(name: &str, bytes: &[u8]) -> LegacyNode {
    let mut out = node(NodeType::File, name);
    out.content_size = bytes.len() as u64;
    out.file_content = Some(bytes.to_vec());
    out
}

fn tree(bytes: &[u8]) -> LegacyNode {
    let mut out = node(NodeType::Directory, "");
    out.children.push(file("a.txt", bytes));
    out
}

fn encoded(tree: &LegacyNode) -> Vec<u8> {
    encode_legacy(tree, &Version::new_v1(), LegacyLimits::default()).unwrap()
}

fn snapshot(tree: LegacyNode, version: Version) -> PackSnapshot {
    PackSnapshot::new(tree, version, Timestamp(0), ContentDigests::new()).unwrap()
}

fn body_cases() -> Vec<Vec<u8>> {
    // Every prefix includes cuts inside the BOM, CRLF and a four-byte UTF-8 character.
    let sample = b"\xef\xbb\xbfA\r\n\0\xf0\x9f\x92\xa9\xff";
    let mut cases: Vec<_> = (0..=sample.len())
        .map(|end| sample[..end].to_vec())
        .collect();
    cases.extend([b"line\r\nnext\r\n".to_vec(), vec![b'x'; 64], vec![b'x'; 65]]);
    cases
}

fn source(bytes: &[u8]) -> MemorySource {
    let mut out = MemorySource::new();
    out.insert_file("a.txt", bytes, 7).unwrap();
    out
}

fn run_scan(source: &dyn Source, limits: PackLimits) -> Result<Scan, DctxError> {
    let policy = SelectionPolicy::from_texts(&RuleTexts::default());
    scan(
        source,
        &CancelFlag::new(),
        ScanOptions {
            policy: &policy,
            debug_ignored: false,
            limits,
        },
    )
}

fn run_spooled(
    source: &dyn Source,
    limits: PackLimits,
) -> Result<(Scan, BTreeMap<String, Vec<u8>>), DctxError> {
    let policy = SelectionPolicy::from_texts(&RuleTexts::default());
    let mut bodies = BTreeMap::new();
    let result = scan_spooled(
        source,
        &CancelFlag::new(),
        ScanOptions {
            policy: &policy,
            debug_ignored: false,
            limits,
        },
        &mut |path, body| {
            bodies.insert(path.to_string(), body.to_vec());
            Ok(())
        },
    )?;
    Ok((result, bodies))
}

fn run_pack(
    source: &dyn Source,
    limits: PackLimits,
    store: &mut MemoryFactStore,
    cache: &mut MemoryCache,
    max_diff_files: u32,
) -> Result<PackReport, DctxError> {
    let policy = SelectionPolicy::from_texts(&RuleTexts::default());
    pack(
        PackPorts {
            source,
            store,
            cache,
            clock: &FixedClock(Timestamp(11)),
            cancel: &CancelFlag::new(),
        },
        &PackRequest {
            view: ViewKey::parse("hostile").unwrap(),
            policy: &policy,
            debug_ignored: false,
            max_diff_files,
            limits,
        },
    )
}

fn pack_twice(source: &dyn Source, limits: PackLimits, quota: u64, diff_limit: u32) {
    let mut store = MemoryFactStore::new();
    let mut cache = MemoryCache::new(quota);
    let first = run_pack(source, limits, &mut store, &mut cache, diff_limit).unwrap();
    let second = run_pack(source, limits, &mut store, &mut cache, diff_limit).unwrap();
    assert_eq!(
        encoded(first.snapshot.tree()),
        encoded(second.snapshot.tree())
    );
    assert_eq!(first.snapshot.digest(), second.snapshot.digest());
    assert_eq!(first.snapshot.version(), second.snapshot.version());
    assert_eq!(first.snapshot.taken_at(), second.snapshot.taken_at());
    assert_eq!(first.diagnostics, second.diagnostics);
    assert!(first.published);
    assert!(!second.published);
    assert!(!second.diff.has_changes);
    assert_eq!(store.publishes(), 1);
    assert_eq!(cache.reserved(), 0);
}

fn manifest(source: &dyn Source) -> PackSnapshot {
    let scanned = run_scan(source, PackLimits::unbounded()).unwrap();
    PackSnapshot::new(
        scanned.tree,
        Version::new_v1(),
        Timestamp(0),
        scanned.digests,
    )
    .unwrap()
    .into_manifest()
}

// Unlike MemorySource, this can return duplicate, invalid or reordered names and
// metadata at integer boundaries without allocating a correspondingly large body.
struct FlatSource {
    names: Vec<SourceEntry>,
    body: Vec<u8>,
    size: u64,
    kind: NodeType,
}

impl FlatSource {
    fn new(names: &[&str], body: &[u8]) -> Self {
        Self {
            names: names.iter().map(|name| SourceEntry::file(name)).collect(),
            body: body.to_vec(),
            size: body.len() as u64,
            kind: NodeType::Directory,
        }
    }
}

impl Source for FlatSource {
    fn root_kind(&self) -> Result<NodeType, DctxError> {
        Ok(self.kind)
    }

    fn entries(&self, _dir: &RelativePath) -> Result<Vec<SourceEntry>, DctxError> {
        Ok(self.names.clone())
    }

    fn metadata(&self, path: &RelativePath) -> Result<EntryMeta, DctxError> {
        Ok(EntryMeta {
            size: if path.is_root() && self.kind == NodeType::Directory {
                0
            } else {
                self.size
            },
            modified: Timestamp(7),
        })
    }

    fn read(&self, _grant: &ReadGrant, max_bytes: u64) -> Result<Vec<u8>, DctxError> {
        if self.body.len() as u64 > max_bytes {
            return Err(DctxError::ResourceLimit {
                resource: Resource::FileBytes,
                limit: max_bytes,
            });
        }
        Ok(self.body.clone())
    }
}

#[test]
fn decode_empty_and_every_one_byte_input_are_typed_refusals() {
    for mode in MODES {
        for bytes in std::iter::once(Vec::new()).chain((0..=255).map(|byte| vec![byte])) {
            for _ in 0..2 {
                assert!(matches!(
                    decode_legacy(&bytes, mode, LegacyLimits::default()),
                    Err(LegacyError::Truncated)
                ));
            }
        }
    }
}

#[test]
fn decode_every_truncation_offset_without_panicking_or_mutating_input() {
    let bytes = encoded(&tree(b"\xef\xbb\xbfhello\r\n\0\xff"));
    let original = bytes.clone();
    for mode in MODES {
        for end in 0..bytes.len() {
            let first = decode_legacy(&bytes[..end], mode, LegacyLimits::default()).unwrap_err();
            let second = decode_legacy(&bytes[..end], mode, LegacyLimits::default()).unwrap_err();
            assert!(
                matches!(first, LegacyError::Truncated),
                "offset {end}: {first}"
            );
            assert_eq!(first.to_string(), second.to_string());
        }
        assert!(decode_legacy(&bytes, mode, LegacyLimits::default()).is_ok());
    }
    assert_eq!(bytes, original);
}

#[test]
fn decode_malformed_fields_are_repeatable_without_panics() {
    let original = encoded(&tree(b"abc"));
    // Exhaust each small document field with fixed hostile bytes, including lengths,
    // child counts, discriminants, revisions, offsets and body sizes.
    for offset in 0..original.len() {
        for byte in [0, 1, 127, 128, 255] {
            let mut bytes = original.clone();
            bytes[offset] = byte;
            for mode in MODES {
                let first = decode_legacy(&bytes, mode, LegacyLimits::default());
                let second = decode_legacy(&bytes, mode, LegacyLimits::default());
                match (first, second) {
                    (Ok(first), Ok(second)) => {
                        assert_eq!(first.header, second.header);
                        assert_eq!(encoded(&first.root), encoded(&second.root));
                    }
                    (Err(first), Err(second)) => assert_eq!(first.to_string(), second.to_string()),
                    _ => panic!("inconsistent decode at {offset}, byte {byte}"),
                }
            }
        }
    }
}

#[test]
fn decode_invalid_utf8_names_but_preserve_arbitrary_body_bytes() {
    let mut bytes = encoded(&file("a", b"\xff\xfe\0"));
    bytes[19] = 255; // Header (16), kind (1), name length (2), then name.
    for mode in MODES {
        assert!(matches!(
            decode_legacy(&bytes, mode, LegacyLimits::default()),
            Err(LegacyError::InvalidUtf8)
        ));
    }
    let bytes = encoded(&file("é", b"\xff\xfe\0"));
    let decoded = decode_legacy(&bytes, LegacyBodies::Include, LegacyLimits::default()).unwrap();
    assert_eq!(
        decoded.root.file_content.as_deref(),
        Some(&b"\xff\xfe\0"[..])
    );
    assert_eq!(encoded(&decoded.root), bytes);
}

#[test]
fn codec_exact_and_over_limit_dimensions_leave_failed_sinks_untouched() {
    let root = tree(b"abc");
    let bytes = encoded(&root);
    let exact = LegacyLimits {
        max_total_bytes: bytes.len(),
        max_nodes: 2,
        max_depth: 1,
        max_name_bytes: 5,
    };
    assert_eq!(
        encode_legacy(&root, &Version::new_v1(), exact).unwrap(),
        bytes
    );
    for mode in MODES {
        assert!(decode_legacy(&bytes, mode, exact).is_ok());
    }
    for (limits, bound) in [
        (
            LegacyLimits {
                max_total_bytes: bytes.len() - 1,
                ..exact
            },
            "total bytes",
        ),
        (
            LegacyLimits {
                max_nodes: 1,
                ..exact
            },
            "nodes",
        ),
        (
            LegacyLimits {
                max_depth: 0,
                ..exact
            },
            "depth",
        ),
        (
            LegacyLimits {
                max_name_bytes: 4,
                ..exact
            },
            "name bytes",
        ),
    ] {
        for _ in 0..2 {
            assert!(matches!(
                encode_legacy(&root, &Version::new_v1(), limits),
                Err(LegacyError::LimitExceeded(name)) if name == bound
            ));
            let mut sink = b"sentinel".to_vec();
            assert!(matches!(
                encode_legacy_to(&root, &Version::new_v1(), limits, &mut sink),
                Err(LegacyError::LimitExceeded(name)) if name == bound
            ));
            assert_eq!(sink, b"sentinel");
            for mode in MODES {
                assert!(matches!(
                    decode_legacy(&bytes, mode, limits),
                    Err(LegacyError::LimitExceeded(name)) if name == bound
                ));
            }
        }
    }
}

#[test]
fn codec_zero_and_oversized_budgets_are_bounded() {
    let root = tree(b"x");
    let bytes = encoded(&root);
    let huge = LegacyLimits {
        max_total_bytes: usize::MAX,
        max_depth: usize::MAX,
        max_name_bytes: usize::MAX,
        max_nodes: usize::MAX,
    };
    let zero = LegacyLimits {
        max_total_bytes: 0,
        max_depth: 0,
        max_name_bytes: 0,
        max_nodes: 0,
    };
    for _ in 0..2 {
        assert_eq!(
            encode_legacy(&root, &Version::new_v1(), huge).unwrap(),
            bytes
        );
        let mut sink = Vec::new();
        encode_legacy_to(&root, &Version::new_v1(), huge, &mut sink).unwrap();
        assert_eq!(sink, bytes);
        assert!(encode_legacy(&root, &Version::new_v1(), zero).is_err());
        assert!(encode_legacy_to(&root, &Version::new_v1(), zero, &mut sink).is_err());
        assert_eq!(sink, bytes);
        for mode in MODES {
            assert!(decode_legacy(&bytes, mode, huge).is_ok());
            assert!(matches!(
                decode_legacy(&bytes, mode, zero),
                Err(LegacyError::LimitExceeded("total bytes"))
            ));
        }
    }
}

#[test]
fn codec_wire_name_and_hard_depth_maxima_and_one_past() {
    let limits = LegacyLimits {
        max_name_bytes: usize::MAX,
        max_depth: usize::MAX,
        ..LegacyLimits::default()
    };
    let mut root = file(&"x".repeat(usize::from(u16::MAX)), b"");
    let bytes = encode_legacy(&root, &Version::new_v1(), limits).unwrap();
    for mode in MODES {
        assert_eq!(
            decode_legacy(&bytes, mode, limits)
                .unwrap()
                .root
                .relative_path,
            root.relative_path
        );
    }
    let mut sink = Vec::new();
    encode_legacy_to(&root, &Version::new_v1(), limits, &mut sink).unwrap();
    assert_eq!(sink, bytes);
    root.relative_path.push('x');
    assert!(matches!(
        encode_legacy(&root, &Version::new_v1(), limits),
        Err(LegacyError::LimitExceeded("name bytes"))
    ));
    assert!(encode_legacy_to(&root, &Version::new_v1(), limits, &mut sink).is_err());
    assert_eq!(sink, bytes);

    root = node(NodeType::Directory, "");
    for _ in 0..LEGACY_MAX_DEPTH {
        let mut parent = node(NodeType::Directory, "");
        parent.children.push(root);
        root = parent;
    }
    let bytes = encode_legacy(&root, &Version::new_v1(), limits).unwrap();
    for mode in MODES {
        assert!(decode_legacy(&bytes, mode, limits).is_ok());
    }
    let mut parent = node(NodeType::Directory, "");
    parent.children.push(root);
    assert!(matches!(
        encode_legacy(&parent, &Version::new_v1(), limits),
        Err(LegacyError::LimitExceeded("depth"))
    ));
    assert!(encode_legacy_to(&parent, &Version::new_v1(), limits, &mut Vec::new()).is_err());
    // Add one directory record independently, so decoder coverage does not depend on encoder acceptance.
    let mut over = bytes[..16].to_vec();
    over.extend_from_slice(&[1, 0, 0]);
    over.extend_from_slice(&0u64.to_le_bytes());
    over.extend_from_slice(&1u32.to_le_bytes());
    over.extend_from_slice(&bytes[16..]);
    for mode in MODES {
        assert!(matches!(
            decode_legacy(&over, mode, limits),
            Err(LegacyError::LimitExceeded("depth"))
        ));
    }
}

#[test]
fn encode_empty_one_byte_and_all_body_prefixes_are_byte_identical() {
    for body in body_cases() {
        let root = tree(&body);
        let before = format!("{root:?}");
        let bytes = encoded(&root);
        assert_eq!(bytes, encoded(&root));
        for _ in 0..2 {
            let mut sink = Vec::new();
            encode_legacy_to(
                &root,
                &Version::new_v1(),
                LegacyLimits::default(),
                &mut sink,
            )
            .unwrap();
            assert_eq!(sink, bytes);
        }
        let decoded =
            decode_legacy(&bytes, LegacyBodies::Include, LegacyLimits::default()).unwrap();
        assert_eq!(
            decoded.root.children[0].file_content.as_deref(),
            Some(body.as_slice())
        );
        assert_eq!(encoded(&decoded.root), bytes);
        assert_eq!(format!("{root:?}"), before);
    }
    let empty = node(NodeType::Directory, "");
    assert_eq!(encoded(&empty).len(), 31);
}

#[test]
fn codec_duplicates_and_reordering_preserve_wire_order_on_replay() {
    for names in [["a", "a", "b"], ["b", "a", "a"]] {
        let mut root = node(NodeType::Directory, "");
        root.children = names
            .iter()
            .enumerate()
            .map(|(index, name)| file(name, &[u8::try_from(index).unwrap()]))
            .collect();
        let bytes = encoded(&root);
        for mode in MODES {
            let first = decode_legacy(&bytes, mode, LegacyLimits::default()).unwrap();
            let second = decode_legacy(&bytes, mode, LegacyLimits::default()).unwrap();
            assert_eq!(
                first
                    .root
                    .children
                    .iter()
                    .map(|child| child.relative_path.as_str())
                    .collect::<Vec<_>>(),
                names
            );
            assert_eq!(encoded(&first.root), encoded(&second.root));
            if mode == LegacyBodies::Include {
                assert_eq!(encoded(&first.root), bytes);
            }
        }
        let mut sink = Vec::new();
        encode_legacy_to(
            &root,
            &Version::new_v1(),
            LegacyLimits::default(),
            &mut sink,
        )
        .unwrap();
        assert_eq!(sink, bytes);
    }
}

#[test]
fn encoder_invalid_trees_and_unrepresentable_sizes_fail_before_writing() {
    let mut file_with_children = file("a", b"x");
    file_with_children.children.push(file("a/b", b"y"));
    let mut directory_with_body = tree(b"");
    directory_with_body.file_content = Some(Vec::new());
    let mut enormous = node(NodeType::File, "a");
    enormous.content_size = u64::MAX;
    for root in [file_with_children, directory_with_body, enormous] {
        for _ in 0..2 {
            assert!(encode_legacy(&root, &Version::new_v1(), LegacyLimits::default()).is_err());
            let mut sink = b"sentinel".to_vec();
            assert!(encode_legacy_to(
                &root,
                &Version::new_v1(),
                LegacyLimits::default(),
                &mut sink
            )
            .is_err());
            assert_eq!(sink, b"sentinel");
        }
    }
}

struct CutSink {
    remaining: usize,
    bytes: Vec<u8>,
}

impl Write for CutSink {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.remaining == 0 {
            return Err(io::Error::other("fixed sink boundary"));
        }
        let count = bytes.len().min(self.remaining).min(1);
        self.bytes.extend_from_slice(&bytes[..count]);
        self.remaining -= count;
        Ok(count)
    }

    fn flush(&mut self) -> io::Result<()> {
        panic!("encoder must leave flushing to its caller")
    }
}

#[test]
fn encode_to_sink_failure_at_every_offset_is_repeatable() {
    let root = tree(b"a\r\n\0\xff");
    let expected = encoded(&root);
    for end in 0..=expected.len() {
        for _ in 0..2 {
            let mut sink = CutSink {
                remaining: end,
                bytes: Vec::new(),
            };
            let result = encode_legacy_to(
                &root,
                &Version::new_v1(),
                LegacyLimits::default(),
                &mut sink,
            );
            if end == expected.len() {
                result.unwrap();
            } else {
                assert!(matches!(result, Err(LegacyError::Sink(_))));
            }
            assert_eq!(sink.bytes, expected[..end]);
        }
    }
}

#[test]
fn revision_boundaries_are_repeatable_and_codec_accepts_historical_maximum() {
    let root = tree(b"");
    for version in [
        Version { major: 0, minor: 0 },
        Version {
            major: 0,
            minor: u32::MAX,
        },
    ] {
        for _ in 0..2 {
            assert!(matches!(
                revision_can_advance(&version),
                Err(LegacyError::UnknownVersion { .. })
            ));
            assert!(encode_legacy(&root, &version, LegacyLimits::default()).is_err());
            let mut sink = Vec::new();
            assert!(encode_legacy_to(&root, &version, LegacyLimits::default(), &mut sink).is_err());
            assert!(sink.is_empty());
        }
    }
    for major in [1, u32::MAX - 1, u32::MAX] {
        let version = Version {
            major,
            minor: u32::MAX,
        };
        for _ in 0..2 {
            if major == u32::MAX {
                assert!(matches!(
                    revision_can_advance(&version),
                    Err(LegacyError::Overflow)
                ));
            } else {
                revision_can_advance(&version).unwrap();
            }
            let bytes = encode_legacy(&root, &version, LegacyLimits::default()).unwrap();
            let mut sink = Vec::new();
            encode_legacy_to(&root, &version, LegacyLimits::default(), &mut sink).unwrap();
            assert_eq!(bytes, sink);
            for mode in MODES {
                assert_eq!(
                    decode_legacy(&bytes, mode, LegacyLimits::default())
                        .unwrap()
                        .header
                        .version,
                    version
                );
            }
        }
    }
}

#[test]
fn scan_empty_root_and_all_body_prefixes_are_repeatable() {
    let empty = MemorySource::new();
    let scanned = run_scan(&empty, PackLimits::unbounded()).unwrap();
    assert_eq!((scanned.entries, scanned.content_bytes), (1, 0));
    assert!(scanned.tree.children.is_empty());
    assert_eq!(
        encoded(&scanned.tree),
        encoded(&run_scan(&empty, PackLimits::unbounded()).unwrap().tree)
    );
    for body in body_cases() {
        let source = source(&body);
        let first = run_scan(&source, PackLimits::unbounded()).unwrap();
        let second = run_scan(&source, PackLimits::unbounded()).unwrap();
        assert_eq!(encoded(&first.tree), encoded(&second.tree));
        assert_eq!(format!("{first:?}"), format!("{second:?}"));
        assert_eq!(first.entries, 2);
        assert_eq!(first.content_bytes, body.len() as u64);
        assert!(first.digests.contains_key("a.txt"));
    }
}

#[test]
fn scan_spooled_empty_and_hostile_bodies_preserve_facts_and_replay() {
    let (empty, bodies) = run_spooled(&MemorySource::new(), PackLimits::unbounded()).unwrap();
    assert_eq!(empty.entries, 1);
    assert!(bodies.is_empty());
    for body in body_cases() {
        let source = source(&body);
        let resident = run_scan(&source, PackLimits::unbounded()).unwrap();
        let (first, bodies) = run_spooled(&source, PackLimits::unbounded()).unwrap();
        let (second, again) = run_spooled(&source, PackLimits::unbounded()).unwrap();
        assert_eq!(format!("{first:?}"), format!("{second:?}"));
        assert_eq!(bodies, again);
        assert_eq!(first.digests, resident.digests);
        assert_eq!(first.content_bytes, resident.content_bytes);
        assert!(first.tree.children[0].file_content.is_none());
        if let Some(bytes) = &resident.tree.children[0].file_content {
            assert_eq!(bodies.get("a.txt"), Some(bytes));
        } else {
            assert!(bodies.is_empty());
        }
        assert_eq!(encoded(&first.tree), encoded(&second.tree));
    }
}

#[test]
fn scan_and_spooled_exact_and_over_limits_are_repeatable() {
    let source = source(b"abcd");
    let exact = PackLimits {
        max_entries: 2,
        max_depth: 1,
        max_file_bytes: 4,
        max_source_bytes: 4,
    };
    for _ in 0..2 {
        assert_eq!(run_scan(&source, exact).unwrap().content_bytes, 4);
        assert_eq!(run_spooled(&source, exact).unwrap().0.content_bytes, 4);
        for (limits, resource, limit) in [
            (
                PackLimits {
                    max_entries: 1,
                    ..exact
                },
                Resource::Entries,
                1,
            ),
            (
                PackLimits {
                    max_depth: 0,
                    ..exact
                },
                Resource::Depth,
                0,
            ),
            (
                PackLimits {
                    max_source_bytes: 3,
                    ..exact
                },
                Resource::SourceBytes,
                3,
            ),
        ] {
            let expected = DctxError::ResourceLimit { resource, limit };
            assert_eq!(run_scan(&source, limits).unwrap_err(), expected);
            assert_eq!(run_spooled(&source, limits).unwrap_err(), expected);
        }
        let small = PackLimits {
            max_file_bytes: 3,
            ..exact
        };
        let withheld = run_scan(&source, small).unwrap();
        assert_eq!(withheld.content_bytes, 0);
        assert_eq!(
            withheld.diagnostics,
            [Diagnostic::ContentWithheld {
                path: "a.txt".to_string(),
                size: 4
            }]
        );
        let (spooled, bodies) = run_spooled(&source, small).unwrap();
        assert_eq!(spooled.diagnostics, withheld.diagnostics);
        assert!(bodies.is_empty());
    }
}

#[test]
fn scan_zero_budgets_and_maximum_reported_size_do_not_allocate_huge_bodies() {
    let zero = PackLimits {
        max_entries: 0,
        max_depth: 0,
        max_file_bytes: 0,
        max_source_bytes: 0,
    };
    for _ in 0..2 {
        assert!(matches!(
            run_scan(&MemorySource::new(), zero),
            Err(DctxError::ResourceLimit {
                resource: Resource::Entries,
                limit: 0
            })
        ));
        assert!(run_spooled(&MemorySource::new(), zero).is_err());
    }
    let mut huge = FlatSource::new(&["a.txt"], b"x");
    huge.size = u64::MAX;
    let limits = PackLimits {
        max_file_bytes: u64::MAX - 1,
        ..PackLimits::unbounded()
    };
    let first = run_scan(&huge, limits).unwrap();
    assert_eq!(first.tree.children[0].content_size, u64::MAX);
    assert_eq!(first.content_bytes, 0);
    assert!(first.digests.is_empty());
    assert_eq!(
        format!("{first:?}"),
        format!("{:?}", run_scan(&huge, limits).unwrap())
    );
    assert!(run_spooled(&huge, limits).unwrap().1.is_empty());
    // The inclusive maximum must allow a read, whose actual length replaces the claim.
    assert_eq!(
        run_scan(&huge, PackLimits::unbounded())
            .unwrap()
            .content_bytes,
        1
    );
}

#[test]
fn scan_duplicate_reordered_and_invalid_names_are_repeatable() {
    let first = FlatSource::new(&["b.txt", "a.txt", "a.txt"], b"x");
    let reordered = FlatSource::new(&["a.txt", "b.txt", "a.txt"], b"x");
    assert_eq!(
        encoded(&run_scan(&first, PackLimits::unbounded()).unwrap().tree),
        encoded(&run_scan(&reordered, PackLimits::unbounded()).unwrap().tree)
    );
    assert_eq!(
        encoded(&run_spooled(&first, PackLimits::unbounded()).unwrap().0.tree),
        encoded(
            &run_spooled(&reordered, PackLimits::unbounded())
                .unwrap()
                .0
                .tree
        )
    );
    for source in [&first, &reordered] {
        let first_scan = run_scan(source, PackLimits::unbounded()).unwrap();
        let second_scan = run_scan(source, PackLimits::unbounded()).unwrap();
        assert_eq!(format!("{first_scan:?}"), format!("{second_scan:?}"));
        let (first_spool, bodies) = run_spooled(source, PackLimits::unbounded()).unwrap();
        let (second_spool, again) = run_spooled(source, PackLimits::unbounded()).unwrap();
        assert_eq!(format!("{first_spool:?}"), format!("{second_spool:?}"));
        assert_eq!(bodies, again);
        let manifest = manifest(source);
        for name in ["a.txt", "b.txt"] {
            let path = RelativePath::parse(name).unwrap();
            for _ in 0..2 {
                assert_eq!(
                    read_snapshot_file(&manifest, &path, source, &CancelFlag::new(), u64::MAX)
                        .unwrap(),
                    b"x"
                );
            }
        }
    }
    let long = "x".repeat(MAX_PATH_BYTES + 1);
    let invalid = FlatSource::new(
        &[
            "",
            ".",
            "..",
            "a/b",
            "a\\b",
            "a\0b",
            "\u{feff}\r\n",
            &long,
            "ok.txt",
        ],
        b"x",
    );
    let first = run_scan(&invalid, PackLimits::unbounded()).unwrap();
    assert_eq!(first.entries, 2);
    assert_eq!(first.diagnostics.len(), 8);
    assert!(first
        .diagnostics
        .iter()
        .all(|item| matches!(item, Diagnostic::UnrepresentableName { .. })));
    assert_eq!(
        format!("{first:?}"),
        format!("{:?}", run_scan(&invalid, PackLimits::unbounded()).unwrap())
    );
    let (spooled, bodies) = run_spooled(&invalid, PackLimits::unbounded()).unwrap();
    assert_eq!(spooled.diagnostics, first.diagnostics);
    assert_eq!(bodies.len(), 1);
    pack_twice(&invalid, PackLimits::unbounded(), u64::MAX, u32::MAX);
}

#[test]
fn pack_reordered_unique_items_produce_identical_bytes() {
    let forward = FlatSource::new(&["a.txt", "b.txt"], b"\xef\xbb\xbfhi\r\n\0");
    let reverse = FlatSource::new(&["b.txt", "a.txt"], b"\xef\xbb\xbfhi\r\n\0");
    let mut outputs = Vec::new();
    for source in [&forward, &reverse] {
        pack_twice(source, PackLimits::unbounded(), u64::MAX, u32::MAX);
        let report = run_pack(
            source,
            PackLimits::unbounded(),
            &mut MemoryFactStore::new(),
            &mut MemoryCache::new(u64::MAX),
            u32::MAX,
        )
        .unwrap();
        outputs.push(encoded(report.snapshot.tree()));
    }
    assert_eq!(outputs[0], outputs[1]);
}

#[test]
fn source_path_at_maximum_length_is_scanned_spooled_packed_and_read_twice() {
    let name = "x".repeat(MAX_PATH_BYTES);
    let source = FlatSource::new(&[&name], b"x");
    let scanned = run_scan(&source, PackLimits::unbounded()).unwrap();
    assert_eq!(scanned.tree.children[0].relative_path, name);
    assert_eq!(
        run_spooled(&source, PackLimits::unbounded())
            .unwrap()
            .1
            .get(&name)
            .unwrap(),
        b"x"
    );
    pack_twice(&source, PackLimits::unbounded(), u64::MAX, u32::MAX);
    let manifest = manifest(&source);
    let path = RelativePath::parse(&name).unwrap();
    for _ in 0..2 {
        assert_eq!(
            read_snapshot_file(&manifest, &path, &source, &CancelFlag::new(), 1).unwrap(),
            b"x"
        );
    }
}

#[test]
fn file_root_empty_and_one_byte_inputs_use_all_source_operations() {
    for body in [b"".as_slice(), b"x".as_slice(), b"\xff\0".as_slice()] {
        let mut source = FlatSource::new(&[], body);
        source.kind = NodeType::File;
        let scanned = run_scan(&source, PackLimits::unbounded()).unwrap();
        assert_eq!(scanned.entries, 1);
        assert_eq!(scanned.content_bytes, body.len() as u64);
        assert_eq!(scanned.digests.len(), 1);
        assert_eq!(
            run_spooled(&source, PackLimits::unbounded())
                .unwrap()
                .0
                .digests,
            scanned.digests
        );
        pack_twice(&source, PackLimits::unbounded(), u64::MAX, 0);
        let manifest = manifest(&source);
        for _ in 0..2 {
            assert_eq!(
                read_snapshot_file(
                    &manifest,
                    &RelativePath::root(),
                    &source,
                    &CancelFlag::new(),
                    u64::MAX
                )
                .unwrap(),
                body
            );
        }
    }
}

#[test]
fn pack_all_body_prefixes_crlf_bom_nul_and_invalid_utf8_identically_twice() {
    pack_twice(&MemorySource::new(), PackLimits::unbounded(), 0, 0);
    for body in body_cases() {
        for diff_limit in [0, u32::MAX] {
            pack_twice(
                &source(&body),
                PackLimits::unbounded(),
                u64::MAX,
                diff_limit,
            );
        }
    }
}

#[test]
fn pack_exact_over_and_zero_budgets_preserve_store_and_cache_on_refusal() {
    let source = source(b"abcd");
    let exact = PackLimits {
        max_entries: 2,
        max_depth: 1,
        max_file_bytes: 4,
        max_source_bytes: 4,
    };
    pack_twice(&source, exact, 4, u32::MAX);
    for (limits, quota) in [
        (exact, 3),
        (exact, 0),
        (
            PackLimits {
                max_entries: 1,
                ..exact
            },
            4,
        ),
        (
            PackLimits {
                max_depth: 0,
                ..exact
            },
            4,
        ),
        (
            PackLimits {
                max_source_bytes: 3,
                ..exact
            },
            4,
        ),
        (
            PackLimits {
                max_entries: 0,
                max_depth: 0,
                max_file_bytes: 0,
                max_source_bytes: 0,
            },
            0,
        ),
    ] {
        let mut store = MemoryFactStore::new();
        let mut cache = MemoryCache::new(quota);
        let first = run_pack(&source, limits, &mut store, &mut cache, 0).unwrap_err();
        let second = run_pack(&source, limits, &mut store, &mut cache, 0).unwrap_err();
        assert_eq!(first, second);
        assert_eq!(store.publishes(), 0);
        assert_eq!((cache.reserved(), cache.committed()), (0, 0));
    }
    // Zero body budgets admit empty content and withhold nonempty content.
    let no_bytes = PackLimits {
        max_file_bytes: 0,
        max_source_bytes: 0,
        ..PackLimits::unbounded()
    };
    pack_twice(&source, no_bytes, 0, 0);
    pack_twice(&self::source(b""), no_bytes, 0, 0);
}

#[test]
fn verified_read_all_prefixes_and_byte_budgets_are_repeatable() {
    let path = RelativePath::parse("a.txt").unwrap();
    for body in body_cases() {
        let source = source(&body);
        let manifest = manifest(&source);
        for budget in [0, body.len() as u64, u64::MAX] {
            let first = read_snapshot_file(&manifest, &path, &source, &CancelFlag::new(), budget);
            let second = read_snapshot_file(&manifest, &path, &source, &CancelFlag::new(), budget);
            assert_eq!(first, second);
            if budget < body.len() as u64 {
                assert_eq!(
                    first,
                    Err(DctxError::ResourceLimit {
                        resource: Resource::FileBytes,
                        limit: budget
                    })
                );
            } else {
                assert_eq!(first.unwrap(), body);
            }
        }
        if !body.is_empty() {
            let budget = body.len() as u64 - 1;
            assert!(matches!(
                read_snapshot_file(&manifest, &path, &source, &CancelFlag::new(), budget),
                Err(DctxError::ResourceLimit {
                    resource: Resource::FileBytes,
                    ..
                })
            ));
        }
    }
}

#[test]
fn verified_read_refuses_every_truncated_or_changed_source() {
    let original = b"\xef\xbb\xbfhello\r\n\0\xff";
    let mut source = source(original);
    let manifest = manifest(&source);
    let path = RelativePath::parse("a.txt").unwrap();
    for end in 0..original.len() {
        source.insert_file("a.txt", &original[..end], 7).unwrap();
        for _ in 0..2 {
            assert_eq!(
                read_snapshot_file(&manifest, &path, &source, &CancelFlag::new(), u64::MAX),
                Err(DctxError::SourceChanged)
            );
        }
    }
    let mut changed = original.to_vec();
    changed[0] ^= 1;
    source.insert_file("a.txt", &changed, 7).unwrap();
    assert_eq!(
        read_snapshot_file(&manifest, &path, &source, &CancelFlag::new(), u64::MAX),
        Err(DctxError::SourceChanged)
    );
    source.insert_file("a.txt", original, 7).unwrap();
    assert_eq!(
        read_snapshot_file(&manifest, &path, &source, &CancelFlag::new(), u64::MAX).unwrap(),
        original
    );
}

#[test]
fn verified_read_empty_manifest_root_missing_and_sensitive_paths_are_denied() {
    let source = MemorySource::new();
    let manifest = manifest(&source);
    for name in ["", "a", "missing.txt", ".env", ".git/config"] {
        for _ in 0..2 {
            assert_eq!(
                read_snapshot_file(
                    &manifest,
                    &RelativePath::parse(name).unwrap(),
                    &source,
                    &CancelFlag::new(),
                    u64::MAX
                ),
                Err(DctxError::PolicyDenied)
            );
        }
    }
    assert!(source.reads().is_empty());
}

#[test]
fn diff_empty_one_byte_and_every_body_prefix_are_repeatable() {
    for body in body_cases() {
        let root = tree(&body);
        let previous = snapshot(root.clone(), Version::new_v1());
        for limit in [0, 1, u32::MAX] {
            let first = diff_against(Some(&previous), &root, limit);
            let second = diff_against(Some(&previous), &root, limit);
            assert_eq!(
                format!("{first:?}").into_bytes(),
                format!("{second:?}").into_bytes()
            );
            assert!(!first.report.has_changes);
            assert_eq!(first.version, Version::new_v1());
            let initial = diff_against(None, &root, limit);
            assert!(initial.report.entries.is_empty());
            assert!(initial.major_reset);
            assert_eq!(initial.version, Version::new_v1());
        }
    }
    let root = node(NodeType::Directory, "");
    let previous = snapshot(root.clone(), Version::new_v1());
    assert!(!diff_against(Some(&previous), &root, 0).report.has_changes);
}

#[test]
fn diff_reordered_and_maximum_size_facts_are_repeatable() {
    let mut root = tree(b"x");
    root.children.push(file("b.txt", b"y"));
    let previous = snapshot(
        root.clone(),
        Version {
            major: 1,
            minor: u32::MAX - 1,
        },
    );
    root.children.reverse();
    for limit in [0, u32::MAX] {
        let report = diff_against(Some(&previous), &root, limit);
        assert!(!report.report.has_changes);
        assert_eq!(
            format!("{report:?}"),
            format!("{:?}", diff_against(Some(&previous), &root, limit))
        );
    }
    root.children[0].content_size = u64::MAX;
    root.children[0].last_modified_timestamp = u64::MAX;
    for limit in [0, u32::MAX] {
        let first = diff_against(Some(&previous), &root, limit);
        assert!(first.report.has_changes);
        assert_eq!(
            format!("{first:?}"),
            format!("{:?}", diff_against(Some(&previous), &root, limit))
        );
    }
}

#[test]
#[ignore = "BUG: diff reports an added item when identical trees contain duplicate paths"]
fn diff_duplicate_items_do_not_invent_changes() {
    let mut root = tree(b"x");
    root.children.push(file("a.txt", b"x"));
    let previous = snapshot(root.clone(), Version::new_v1());
    for limit in [0, u32::MAX] {
        for _ in 0..2 {
            let report = diff_against(Some(&previous), &root, limit);
            assert!(!report.report.has_changes, "{report:?}");
            assert_eq!(report.version, Version::new_v1());
        }
    }
}

#[test]
#[ignore = "BUG: repeated packing of duplicate source entries republishes and increments the version"]
fn pack_duplicate_items_are_idempotent() {
    let source = FlatSource::new(&["a.txt", "a.txt"], b"x");
    pack_twice(&source, PackLimits::unbounded(), u64::MAX, u32::MAX);
}

#[test]
fn decoder_rejects_unclaimed_leading_data() {
    let mut bytes = encoded(&file("a", b"x"));
    // File offset follows header, kind, name length, one-byte name and timestamp.
    bytes[28..36].copy_from_slice(&1u64.to_le_bytes());
    bytes.insert(44, b'?');
    for mode in MODES {
        let error = decode_legacy(&bytes, mode, LegacyLimits::default()).unwrap_err();
        assert!(matches!(error, LegacyError::CorruptData { offset: 0 }));
        assert_eq!(
            error.to_string(),
            "corrupt legacy data at data section offset 0"
        );
    }
}

#[test]
fn decoder_requires_exact_data_coverage_in_both_modes() {
    let mut root = tree(b"abc");
    root.children.push(file("b.txt", b"de"));
    let original = encoded(&root);
    // Directory metadata (15 bytes), then the first file record (32 bytes).
    let second_offset = 16 + 15 + 32 + 1 + 2 + 5 + 8;
    for (offset, size, expected) in [(4u64, 1u64, 3), (2, 3, 2), (0, 5, 0)] {
        let mut bytes = original.clone();
        bytes[second_offset..second_offset + 8].copy_from_slice(&offset.to_le_bytes());
        bytes[second_offset + 8..second_offset + 16].copy_from_slice(&size.to_le_bytes());
        for mode in MODES {
            assert!(matches!(
                decode_legacy(&bytes, mode, LegacyLimits::default()),
                Err(LegacyError::CorruptData { offset }) if offset == expected
            ));
        }
    }
    let mut trailing = original.clone();
    trailing.push(b'?');
    let mut no_files = encoded(&node(NodeType::Directory, ""));
    no_files.push(b'?');
    let mut empty_file = encoded(&file("empty", b""));
    empty_file.push(b'?');
    for (bytes, expected) in [(&trailing, 5), (&no_files, 0), (&empty_file, 0)] {
        for mode in MODES {
            assert!(matches!(
                decode_legacy(bytes, mode, LegacyLimits::default()),
                Err(LegacyError::CorruptData { offset }) if offset == expected
            ));
        }
    }
}

#[test]
fn decoder_accepts_reordered_ranges_and_empty_files() {
    let mut root = tree(b"abc");
    root.children.push(file("empty", b""));
    root.children.push(file("b.txt", b"de"));
    let mut bytes = encoded(&root);
    // Swap the two nonempty bodies' ranges without changing metadata order.
    let first_offset = 16 + 15 + 1 + 2 + 5 + 8;
    let empty_offset = first_offset + 32;
    let last_offset = empty_offset + 32;
    bytes[first_offset..first_offset + 8].copy_from_slice(&2u64.to_le_bytes());
    bytes[empty_offset..empty_offset + 8].copy_from_slice(&3u64.to_le_bytes());
    bytes[last_offset..last_offset + 8].copy_from_slice(&0u64.to_le_bytes());
    for mode in MODES {
        let decoded = decode_legacy(&bytes, mode, LegacyLimits::default()).unwrap();
        if mode == LegacyBodies::Include {
            assert_eq!(
                decoded.root.children[0].file_content.as_deref(),
                Some(&b"cde"[..])
            );
            assert_eq!(
                decoded.root.children[1].file_content.as_deref(),
                Some(&b""[..])
            );
            assert_eq!(
                decoded.root.children[2].file_content.as_deref(),
                Some(&b"ab"[..])
            );
        }
    }
}

#[test]
#[ignore = "BUG: packing at the exhausted major revision publishes an invalid version zero"]
fn pack_refuses_exhausted_major_before_publication() {
    let source = source(b"new");
    let mut old = tree(b"old");
    old.children[0].last_modified_timestamp = 6;
    let previous = snapshot(
        old,
        Version {
            major: u32::MAX,
            minor: 0,
        },
    );
    let mut store = MemoryFactStore::new();
    store
        .publish(&ViewKey::parse("hostile").unwrap(), &previous)
        .unwrap();
    let mut cache = MemoryCache::new(u64::MAX);
    for _ in 0..2 {
        assert!(run_pack(
            &source,
            PackLimits::unbounded(),
            &mut store,
            &mut cache,
            u32::MAX
        )
        .is_err());
        assert_eq!(store.publishes(), 1);
        assert_eq!((cache.reserved(), cache.committed()), (0, 0));
        assert_eq!(
            store
                .load(&ViewKey::parse("hostile").unwrap())
                .unwrap()
                .unwrap()
                .version(),
            previous.version()
        );
    }
}
