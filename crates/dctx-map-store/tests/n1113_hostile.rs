// SPDX-License-Identifier: Apache-2.0
//! Hostile inputs through the public SQLite and comparison-store APIs.
use dctx_core::{
    datatypes::{DirContextTreeNode, NodeType},
    facts::{ContentDigests, PackSnapshot},
    ids::{Timestamp, ViewKey},
    version::Version,
};
use dctx_map_model::{
    Confidence, Edge, EdgeKind, Node, NodeKind, StableId, Summary, SummarySource,
};
use dctx_map_store::*;
use rusqlite::{Connection, params};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

struct TempDb(PathBuf);
impl TempDb {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "n1113-store-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn path(&self) -> PathBuf {
        self.0.join("index.sqlite")
    }
}
impl Drop for TempDb {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

fn contract(revision: &str) -> ExtractionContract {
    ExtractionContract::new(1, "rust", "parser", revision, "plan", 1).unwrap()
}
fn key(bytes: &[u8]) -> FactKey {
    FactKey::from_bytes(bytes, contract("p1")).unwrap()
}
fn facts(count: usize) -> FileFacts {
    let nodes: Vec<_> = (0..count)
        .map(|i| Node {
            id: StableId::new(NodeKind::Symbol, "a.rs", Some(&format!("a{i:03}")), None).unwrap(),
            kind: NodeKind::Symbol,
            symbol_kind: None,
            path: "a.rs".into(),
            lines: [1, 1],
            name: format!("needle{i:03}"),
            signature: "fn needle()".into(),
            fingerprint: "body".into(),
            public: true,
            summary: Summary {
                text: "needle".into(),
                source: SummarySource::Doc,
            },
        })
        .collect();
    let edges = nodes
        .iter()
        .map(|n| Edge {
            src: n.id.clone(),
            dst: n.id.clone(),
            kind: EdgeKind::Calls,
            conf: Confidence::Name,
            owner_path: "a.rs".into(),
        })
        .collect();
    let summaries = nodes
        .iter()
        .map(|n| StoredSummary {
            id: n.id.clone(),
            fingerprint: n.fingerprint.clone(),
            text: "summary".into(),
            model: "fixture".into(),
            at: 0,
        })
        .collect();
    FileFacts {
        file: File {
            path: "a.rs".into(),
            size: 1,
            mtime: 0,
            sha256: "digest".into(),
            lang: "rust".into(),
        },
        nodes,
        edges,
        summaries,
        unresolved: vec![Unresolved {
            name: "missing".into(),
            kind: EdgeKind::Calls,
        }],
    }
}
fn snapshot(tree: DirContextTreeNode) -> PackSnapshot {
    PackSnapshot::new(tree, Version::new_v1(), Timestamp(0), ContentDigests::new()).unwrap()
}
fn root() -> DirContextTreeNode {
    DirContextTreeNode::new(NodeType::Directory, "", PathBuf::new())
}
fn bytes(store: &SqliteStore) -> Vec<u8> {
    format!("{:?}", store.file("a.rs").unwrap()).into_bytes()
}

#[test]
fn empty_store_entry_points_are_repeatable() {
    let mut store = SqliteStore::in_memory().unwrap();
    assert_eq!(store.parse_count().unwrap(), 0);
    assert_eq!(store.cached_bytes().unwrap(), 0);
    assert_eq!(store.cache_cap().unwrap(), HARD_CAP_BYTES);
    assert!(store.file("").unwrap().is_none());
    assert!(store.file("x").unwrap().is_none());
    assert_eq!(store.prune(&[]).unwrap(), 0);
    assert_eq!(store.prune(&["", "x", "x"]).unwrap(), 0);
    assert!(store.search("needle", 1).unwrap().is_empty());
    assert!(store.retained_fact("n", &key(b"")).unwrap().is_none());
    store.release_root("n", "r").unwrap();
    store.release_root("n", "r").unwrap();
    assert_eq!(store.maintain().unwrap(), store.maintain().unwrap());
    assert!(store.load(&ViewKey::parse("v").unwrap()).unwrap().is_none());
    store.reindex(&facts(0)).unwrap();
    assert_eq!(store.file("a.rs").unwrap().unwrap(), facts(0));
}

#[test]
fn fact_bytes_cover_all_prefixes_one_byte_and_invalid_utf8() {
    let sample = b"facts\x00\xff\xc0\xaf".to_vec();
    assert!(std::str::from_utf8(&sample).is_err());
    let mut store = SqliteStore::in_memory().unwrap();
    for end in 0..=sample.len() {
        let payload = &sample[..end];
        let k = key(payload);
        k.check().unwrap();
        assert_eq!(k.content().byte_length(), end as u64);
        assert_eq!(
            key(payload).to_string().as_bytes(),
            k.to_string().as_bytes()
        );
        assert_eq!(
            store.retain_fact("n", "r", &k, payload).unwrap(),
            Retain::Parsed
        );
        assert_eq!(
            store.retain_fact("n", "r", &k, payload).unwrap(),
            Retain::Reused
        );
        assert_eq!(store.retained_fact("n", &k).unwrap().unwrap(), payload);
    }
    assert_eq!(store.parse_count().unwrap(), (sample.len() + 1) as u64);
}

#[test]
fn extraction_contracts_cover_every_text_field_and_byte_boundaries() {
    for field in 0..4 {
        for text in [
            String::new(),
            "a".into(),
            "é".repeat(128),
            "a".repeat(256),
            "a".repeat(257),
            "git:opaque".into(),
            "a|b".into(),
            "a/b".into(),
            "a\\b".into(),
            "\0".into(),
        ] {
            let valid = !text.is_empty()
                && text.len() <= 256
                && !text.contains(['|', '/', '\\', '\0'])
                && !text.starts_with("git:");
            let mut fields = ["d".to_owned(), "p".into(), "r".into(), "e".into()];
            fields[field] = text.clone();
            assert_eq!(
                ExtractionContract::new(1, &fields[0], &fields[1], &fields[2], &fields[3], 1)
                    .is_ok(),
                valid,
                "field {field}: {text:?}"
            );
        }
    }
    for value in [0, 1, u16::MAX] {
        assert_eq!(
            ExtractionContract::new(value, "d", "p", "r", "e", value).is_ok(),
            value != 0
        );
    }
    let sample = "parser-révision";
    for cut in 0..=sample.len() {
        if let Ok(text) = std::str::from_utf8(&sample.as_bytes()[..cut]) {
            let _ = ExtractionContract::new(1, text, text, text, text, 1);
        }
    }
}

#[test]
fn namespace_and_root_limits_apply_to_retain_lookup_and_release() {
    let mut store = SqliteStore::in_memory().unwrap();
    let k = key(b"a");
    for len in [0, 1, 4096, 4097] {
        let label = "x".repeat(len);
        let valid = (1..=4096).contains(&len);
        assert_eq!(store.retain_fact(&label, "r", &k, b"a").is_ok(), valid);
        assert_eq!(store.retained_fact(&label, &k).is_ok(), valid);
        assert_eq!(store.release_root(&label, "r").is_ok(), valid);
        assert_eq!(store.retain_fact("n", &label, &k, b"a").is_ok(), valid);
        assert_eq!(store.release_root("n", &label).is_ok(), valid);
    }
    for bad in ["git:opaque", "a/.git/b", "a\0b", "a\nb"] {
        assert!(store.retain_fact(bad, "r", &k, b"a").is_err());
        assert!(store.retained_fact(bad, &k).is_err());
        assert!(store.release_root("n", bad).is_err());
    }
}

#[test]
fn cache_accounting_is_exact_at_all_integer_boundaries() {
    for cap in [
        0,
        1,
        HARD_CAP_BYTES,
        u64::MAX / 3,
        u64::MAX / 3 + 1,
        i64::MAX as u64,
        u64::MAX,
    ] {
        let expected = (
            (u128::from(cap) * 3 / 4) as u64,
            (u128::from(cap) * 3 / 5) as u64,
        );
        assert_eq!(cache_limits(cap), expected, "cap {cap}");
    }
    assert_eq!(accounted_bytes(0), Some(ACCOUNTING_UNIT));
    assert_eq!(accounted_bytes(1), Some(ACCOUNTING_UNIT + 1));
    assert_eq!(accounted_bytes(u64::MAX - ACCOUNTING_UNIT), Some(u64::MAX));
    assert_eq!(accounted_bytes(u64::MAX - ACCOUNTING_UNIT + 1), None);
    assert_eq!(accounted_bytes(u64::MAX), None);
}

#[test]
fn admission_at_exact_cap_and_over_cap_is_atomic() {
    for size in [0, 1, 256, 257] {
        let mut store = SqliteStore::in_memory().unwrap();
        let payload = vec![0xff; size];
        let k = key(&payload);
        let charge = accounted_bytes(size as u64).unwrap();
        store.set_cache_cap(charge - 1).unwrap();
        assert!(matches!(
            store.retain_fact("n", "r", &k, &payload),
            Err(StoreError::CacheLimit { .. })
        ));
        assert_eq!(store.parse_count().unwrap(), 0);
        assert_eq!(store.cached_bytes().unwrap(), 0);
        store.set_cache_cap(charge).unwrap();
        assert_eq!(
            store.retain_fact("n", "r", &k, &payload).unwrap(),
            Retain::Parsed
        );
        assert_eq!(store.cached_bytes().unwrap(), charge);
        assert!(matches!(
            store.retain_fact("n", "r", &k, b"conflict"),
            Err(StoreError::FactConflict)
        ));
        assert_eq!(store.parse_count().unwrap(), 1);
        assert_eq!(store.retained_fact("n", &k).unwrap().unwrap(), payload);
    }
    let mut store = SqliteStore::in_memory().unwrap();
    store.set_cache_cap(i64::MAX as u64).unwrap();
    assert!(store.set_cache_cap(i64::MAX as u64 + 1).is_err());
    assert!(store.set_cache_cap(u64::MAX).is_err());
    assert_eq!(store.cache_cap().unwrap(), i64::MAX as u64);
    store.set_clock(u64::MAX);
    assert!(store.maintain().is_err());
    store.set_clock(0);
    assert!(store.maintain().is_ok());
}

#[test]
fn graph_reindex_is_byte_identical_under_repeats_and_reordering() {
    let mut store = SqliteStore::in_memory().unwrap();
    let mut f = facts(4);
    f.unresolved.push(Unresolved {
        name: "other".into(),
        kind: EdgeKind::Imports,
    });
    store.reindex(&f).unwrap();
    let expected = bytes(&store);
    let hits = store.search("needle", 40).unwrap();
    store.reindex(&f).unwrap();
    assert_eq!(bytes(&store), expected);
    f.nodes.reverse();
    f.edges.reverse();
    f.summaries.reverse();
    f.unresolved.reverse();
    store.reindex(&f).unwrap();
    assert_eq!(bytes(&store), expected);
    assert_eq!(store.search("needle", 40).unwrap(), hits);
    assert_eq!(store.prune(&["a.rs", "a.rs", "missing"]).unwrap(), 0);
    assert_eq!(bytes(&store), expected);
    assert_eq!(store.prune(&[]).unwrap(), 1);
    assert!(store.file("a.rs").unwrap().is_none());
    assert!(store.search("needle", 40).unwrap().is_empty());
}

#[test]
fn duplicate_graph_items_and_bad_spans_roll_back_without_losing_facts() {
    let mut store = SqliteStore::in_memory().unwrap();
    let good = facts(1);
    store.reindex(&good).unwrap();
    let expected = bytes(&store);
    for field in 0..4 {
        let mut bad = good.clone();
        match field {
            0 => bad.nodes.push(bad.nodes[0].clone()),
            1 => bad.edges.push(bad.edges[0].clone()),
            2 => bad.unresolved.push(bad.unresolved[0].clone()),
            _ => bad.summaries.push(bad.summaries[0].clone()),
        }
        assert!(store.reindex(&bad).is_err());
        assert_eq!(bytes(&store), expected);
    }
    for span in [[0, 1], [2, 1]] {
        let mut bad = good.clone();
        bad.nodes[0].lines = span;
        assert!(store.reindex(&bad).is_err());
        assert_eq!(bytes(&store), expected);
    }
    let mut maximum = good.clone();
    maximum.nodes[0].lines = [u32::MAX, u32::MAX];
    store.reindex(&maximum).unwrap();
    assert_eq!(
        store.file("a.rs").unwrap().unwrap().nodes[0].lines,
        [u32::MAX, u32::MAX]
    );
    maximum.file.size = u64::MAX;
    assert!(store.reindex(&maximum).is_err());
}

#[test]
fn file_paths_are_checked_even_when_no_nodes_are_present() {
    let mut store = SqliteStore::in_memory().unwrap();
    for path in ["", "../x", "/x", "a//b", "a\\b", "a:b", "a\0b"] {
        let mut f = facts(0);
        f.file.path = path.into();
        assert!(store.reindex(&f).is_err(), "{path:?}");
        assert!(store.file(path).unwrap().is_none());
    }
}

#[test]
fn summary_limits_count_characters_and_failures_preserve_old_rows() {
    let mut store = SqliteStore::in_memory().unwrap();
    let mut good = facts(1);
    good.summaries[0].text = "é".repeat(100);
    store.reindex(&good).unwrap();
    let expected = bytes(&store);
    for bad in [String::new(), "é".repeat(101), "a\nb".into(), "a\rb".into()] {
        let mut f = good.clone();
        f.summaries[0].text = bad;
        assert!(store.reindex(&f).is_err());
        assert_eq!(bytes(&store), expected);
    }
}

#[test]
fn fts_handles_all_expression_prefixes_and_clamps_extreme_limits() {
    let mut store = SqliteStore::in_memory().unwrap();
    store.reindex(&facts(45)).unwrap();
    for limit in [0, 1, 40, 41, usize::MAX] {
        let hits = store.search("needle", limit).unwrap();
        assert_eq!(hits.len(), limit.min(40));
        assert_eq!(store.search("needle", limit).unwrap(), hits);
    }
    let sample = "\"needle\" OR \"café\"";
    for end in 0..=sample.len() {
        if let Ok(expr) = std::str::from_utf8(&sample.as_bytes()[..end]) {
            let first = store.search(expr, usize::MAX);
            let second = store.search(expr, usize::MAX);
            match (first, second) {
                (Ok(a), Ok(b)) => assert_eq!(a, b),
                (Err(a), Err(b)) => assert_eq!(a.to_string(), b.to_string()),
                _ => panic!("unstable FTS result at {end}"),
            }
        }
    }
}

#[test]
fn pack_roundtrip_prefixes_invalid_utf8_and_republish_use_stable_bytes() {
    let temp = TempDb::new();
    let mut store = SqliteStore::open(temp.path()).unwrap();
    let raw = Connection::open(temp.path()).unwrap();
    let view = ViewKey::parse("v").unwrap();
    let mut tree = root();
    tree.generated_id_for_llm = "canary".into();
    let snapshot = snapshot(tree);
    store.publish(&view, &snapshot).unwrap();
    let payload: Vec<u8> = raw
        .query_row("SELECT payload FROM pack_snapshots", [], |r| r.get(0))
        .unwrap();
    store.publish(&view, &snapshot).unwrap();
    let again: Vec<u8> = raw
        .query_row("SELECT payload FROM pack_snapshots", [], |r| r.get(0))
        .unwrap();
    assert_eq!(again, payload);
    assert_eq!(
        store.load(&view).unwrap().unwrap().digest(),
        snapshot.digest()
    );
    for cut in 0..payload.len() {
        raw.execute("UPDATE pack_snapshots SET payload=?1", [&payload[..cut]])
            .unwrap();
        assert!(store.load(&view).is_err(), "cut {cut}");
    }
    let mut invalid = payload.clone();
    let at = invalid.windows(6).position(|w| w == b"canary").unwrap();
    invalid[at] = 0xff;
    let end = invalid.len() - 32;
    let digest = dctx_hash::Sha256::digest(&invalid[..end]).unwrap();
    invalid[end..].copy_from_slice(digest.as_bytes());
    raw.execute("UPDATE pack_snapshots SET payload=?1", [&invalid])
        .unwrap();
    assert!(store.load(&view).is_err());
    raw.execute("UPDATE pack_snapshots SET payload=?1", [&payload])
        .unwrap();
    assert_eq!(
        store.load(&view).unwrap().unwrap().digest(),
        snapshot.digest()
    );
    for label in ["git:opaque", "a/.git/b"] {
        let v = ViewKey::parse(label).unwrap();
        assert!(store.publish(&v, &snapshot).is_err());
        assert!(store.load(&v).is_err());
    }
}

#[test]
fn pack_depth_limit_and_one_over_leave_the_previous_snapshot_intact() {
    let mut store = SqliteStore::in_memory().unwrap();
    let view = ViewKey::parse("v").unwrap();
    let base = snapshot(root());
    store.publish(&view, &base).unwrap();
    for depth in [0, 1, 128, 129] {
        let mut tree = root();
        for level in (0..depth).rev() {
            let mut parent =
                DirContextTreeNode::new(NodeType::Directory, &format!("d{level}"), PathBuf::new());
            parent.children.push(tree);
            tree = parent;
        }
        let candidate = snapshot(tree);
        if depth <= 128 {
            store.publish(&view, &candidate).unwrap();
        } else {
            assert!(store.publish(&view, &candidate).is_err());
        }
        let loaded = store.load(&view).unwrap().unwrap();
        if depth <= 128 {
            assert_eq!(loaded.digest(), candidate.digest());
        } else {
            assert_ne!(loaded.digest(), candidate.digest());
        }
    }
}

#[test]
fn sqlite_open_refuses_small_header_prefixes_and_invalid_utf8() {
    let temp = TempDb::new();
    let path = temp.path();
    let sample = b"SQLite format 3\0\xff\xff\x00\x01";
    for cut in 1..=sample.len() {
        std::fs::write(&path, &sample[..cut]).unwrap();
        assert!(SqliteStore::open(&path).is_err(), "cut {cut}");
        assert_eq!(std::fs::read(&path).unwrap(), sample[..cut]);
    }
    std::fs::write(&path, []).unwrap();
    assert!(SqliteStore::open(&path).is_ok());
}

#[test]
fn concurrent_openers_initialize_a_new_database_once() {
    let temp = TempDb::new();
    let barrier = std::sync::Barrier::new(4);
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..4)
            .map(|_| {
                scope.spawn(|| {
                    barrier.wait();
                    let store = SqliteStore::open(temp.path()).unwrap();
                    assert_eq!(store.cache_cap().unwrap(), HARD_CAP_BYTES);
                    assert_eq!(store.parse_count().unwrap(), 0);
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
    });
}

#[test]
fn independent_connections_read_concurrently_and_return_identical_facts() {
    let temp = TempDb::new();
    let mut store = SqliteStore::open(temp.path()).unwrap();
    store.reindex(&facts(4)).unwrap();
    let expected = bytes(&store);
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..4)
            .map(|_| {
                scope.spawn(|| {
                    let store = SqliteStore::open(temp.path()).unwrap();
                    (bytes(&store), store.search("needle", usize::MAX).unwrap())
                })
            })
            .collect();
        for h in handles {
            let (actual, hits) = h.join().unwrap();
            assert_eq!(actual, expected);
            assert_eq!(hits, store.search("needle", 40).unwrap());
        }
    });
}

#[test]
fn stored_json_prefixes_and_invalid_utf8_return_errors_without_panicking() {
    let temp = TempDb::new();
    let mut store = SqliteStore::open(temp.path()).unwrap();
    store.reindex(&facts(1)).unwrap();
    let raw = Connection::open(temp.path()).unwrap();
    let payload: String = raw
        .query_row("SELECT payload FROM nodes", [], |r| r.get(0))
        .unwrap();
    for cut in 0..payload.len() {
        raw.execute("UPDATE nodes SET payload=?1", [&payload[..cut]])
            .unwrap();
        assert!(store.file("a.rs").is_err(), "cut {cut}");
    }
    raw.execute(
        "UPDATE nodes SET payload=CAST(?1 AS TEXT)",
        params![vec![0xffu8]],
    )
    .unwrap();
    assert!(store.file("a.rs").is_err());
    raw.execute("UPDATE nodes SET payload=?1", [&payload])
        .unwrap();
    assert_eq!(store.file("a.rs").unwrap().unwrap(), facts(1));
}

#[test]
fn public_storage_errors_name_a_next_step_and_preserve_sources() {
    let errors = [
        StoreError::Sqlite(rusqlite::Error::InvalidQuery),
        StoreError::Payload(serde_json::from_str::<serde_json::Value>("{").unwrap_err()),
        StoreError::RebuildRequired(2),
        StoreError::InvalidFacts("ownership"),
        StoreError::InvalidId("bad".parse::<StableId>().unwrap_err()),
        StoreError::Corrupt("payload"),
        StoreError::CacheLimit {
            requested: 1,
            available: 0,
        },
        StoreError::InvalidKey("key"),
        StoreError::FactConflict,
    ];
    for error in errors {
        assert!(error.to_string().contains("; next: "), "{error}");
    }
    let error = StoreError::Sqlite(rusqlite::Error::InvalidQuery);
    assert!(std::error::Error::source(&error).is_some());
}

#[test]
fn pack_node_and_digest_count_limits_accept_maximum_and_refuse_one_over() {
    let mut store = SqliteStore::in_memory().unwrap();
    let view = ViewKey::parse("v").unwrap();
    let mut tree = root();
    tree.children = (1..100_000)
        .map(|i| DirContextTreeNode::new(NodeType::File, &format!("f{i}"), PathBuf::new()))
        .collect();
    let maximum = snapshot(tree);
    store.publish(&view, &maximum).unwrap();
    assert_eq!(
        store.load(&view).unwrap().unwrap().digest(),
        maximum.digest()
    );
    let expected = maximum.digest();
    let mut tree = maximum.into_tree();
    tree.children.push(DirContextTreeNode::new(
        NodeType::File,
        "over",
        PathBuf::new(),
    ));
    assert!(store.publish(&view, &snapshot(tree)).is_err());
    assert_eq!(store.load(&view).unwrap().unwrap().digest(), expected);
    let digest = dctx_hash::Sha256::digest(b"").unwrap();
    let mut digests: ContentDigests = (0..100_000).map(|i| (format!("f{i}"), digest)).collect();
    let maximum =
        PackSnapshot::new(root(), Version::new_v1(), Timestamp(0), digests.clone()).unwrap();
    store.publish(&view, &maximum).unwrap();
    assert_eq!(
        store.load(&view).unwrap().unwrap().digests(),
        maximum.digests()
    );
    digests.insert("over".into(), digest);
    let over = PackSnapshot::new(root(), Version::new_v1(), Timestamp(0), digests).unwrap();
    assert!(store.publish(&view, &over).is_err());
    assert_eq!(
        store.load(&view).unwrap().unwrap().digests(),
        maximum.digests()
    );
}

#[test]
fn concurrent_fact_and_pack_writers_preserve_one_parse_and_complete_payloads() {
    let temp = TempDb::new();
    // Open serially so the barrier specifically exercises data transactions.
    let stores: Vec<_> = (0..4)
        .map(|_| SqliteStore::open(temp.path()).unwrap())
        .collect();
    let barrier = std::sync::Barrier::new(4);
    let k = key(b"source");
    let view = ViewKey::parse("v").unwrap();
    let snapshot = snapshot(root());
    std::thread::scope(|scope| {
        let handles: Vec<_> = stores
            .into_iter()
            .enumerate()
            .map(|(i, mut store)| {
                let barrier = &barrier;
                let k = &k;
                let view = &view;
                let snapshot = &snapshot;
                scope.spawn(move || {
                    barrier.wait();
                    store.reindex(&facts(1)).unwrap();
                    let root = format!("r{i}");
                    let admitted = store.retain_fact("n", &root, k, b"payload").unwrap();
                    assert_eq!(store.retained_fact("n", k).unwrap().unwrap(), b"payload");
                    store.publish(view, snapshot).unwrap();
                    assert_eq!(
                        store.load(view).unwrap().unwrap().digest(),
                        snapshot.digest()
                    );
                    store.release_root("n", &root).unwrap();
                    store.set_cache_cap(HARD_CAP_BYTES).unwrap();
                    store.maintain().unwrap();
                    assert_eq!(store.file("a.rs").unwrap().unwrap(), facts(1));
                    admitted
                })
            })
            .collect();
        let admitted: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert_eq!(admitted.iter().filter(|r| **r == Retain::Parsed).count(), 1);
        assert_eq!(admitted.iter().filter(|r| **r == Retain::Reused).count(), 3);
    });
    let mut final_store = SqliteStore::open(temp.path()).unwrap();
    assert_eq!(final_store.parse_count().unwrap(), 1);
    assert_eq!(
        final_store.retained_fact("n", &k).unwrap().unwrap(),
        b"payload"
    );
}
