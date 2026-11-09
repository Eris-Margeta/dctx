// SPDX-License-Identifier: Apache-2.0
//! Replacement, isolation and bundled FTS5 evidence for map storage.
//! Tests map spec sections 6.4, 7 and 8 through the public port.
//! Uses generated file facts and deterministic raw-row snapshots.
use super::*;
use crate::domain::*;
use dctx_map_model::{
    Confidence, Edge, EdgeKind, Node, NodeKind, StableId, Summary, SummarySource,
};
use proptest::prelude::*;

fn facts(path: &str, names: &[String], salt: u32) -> FileFacts {
    let nodes: Vec<Node> = names
        .iter()
        .enumerate()
        .map(|(i, name)| Node {
            id: StableId::new(NodeKind::Symbol, path, Some(&format!("{name}_{i}")), None).unwrap(),
            kind: NodeKind::Symbol,
            symbol_kind: None,
            path: path.into(),
            lines: [1, 2],
            name: name.clone(),
            signature: format!("fn {name}()"),
            fingerprint: format!("fingerprint-{salt}-{i}"),
            public: true,
            summary: Summary {
                text: "Documentation aardvark".into(),
                source: SummarySource::Doc,
            },
        })
        .collect();
    let edges = nodes
        .iter()
        .map(|node| Edge {
            src: node.id.clone(),
            dst: "m:foreign.rs".parse().unwrap(),
            kind: EdgeKind::Calls,
            conf: Confidence::Name,
            owner_path: path.into(),
        })
        .collect();
    let summaries = nodes
        .iter()
        .map(|node| StoredSummary {
            id: node.id.clone(),
            fingerprint: node.fingerprint.clone(),
            text: "Generated platypus".into(),
            model: "test-model".into(),
            at: i64::from(salt),
        })
        .collect();
    FileFacts {
        file: File {
            path: path.into(),
            size: u64::from(salt),
            mtime: i64::from(salt),
            sha256: format!("sha-{salt}"),
            lang: "rust".into(),
        },
        nodes,
        edges,
        summaries,
        unresolved: vec![Unresolved {
            name: format!("unknown-{salt}"),
            kind: EdgeKind::Calls,
        }],
    }
}

fn snapshot(store: &SqliteStore, path: &str) -> Vec<Vec<u8>> {
    let queries = [
        "SELECT quote(path)||quote(size)||quote(mtime)||quote(sha256)||quote(lang) FROM files WHERE path=?1",
        "SELECT quote(id)||quote(kind)||quote(path)||quote(l0)||quote(l1)||quote(name)||quote(sig)||quote(fp)||quote(vis)||quote(payload) FROM nodes WHERE path=?1 ORDER BY id",
        "SELECT quote(src)||quote(dst)||quote(kind)||quote(conf)||quote(owner_path)||quote(payload) FROM edges WHERE owner_path=?1 ORDER BY src,dst,kind,conf",
        "SELECT quote(owner_path)||quote(name)||quote(kind) FROM unresolved WHERE owner_path=?1 ORDER BY name,kind",
        "SELECT quote(s.id)||quote(s.fp)||quote(s.text)||quote(s.src)||quote(s.model)||quote(s.at) FROM summaries s JOIN nodes n ON n.id=s.id WHERE n.path=?1 ORDER BY s.id",
        "SELECT quote(f.id)||quote(f.tokens) FROM fts f JOIN nodes n ON n.id=f.id WHERE n.path=?1 ORDER BY f.id",
    ];
    queries
        .iter()
        .flat_map(|sql| {
            store
                .connection
                .prepare(sql)
                .unwrap()
                .query_map([path], |r| r.get::<_, String>(0))
                .unwrap()
                .map(|row| row.unwrap().into_bytes())
                .collect::<Vec<_>>()
        })
        .collect()
}

proptest! {
    #[test]
    fn replacement_changes_exactly_one_files_rows(
        old in prop::collection::vec("[a-zA-Z_]{1,20}", 0..8),
        new in prop::collection::vec("[a-zA-Z_]{1,20}", 0..8),
        other in prop::collection::vec("[a-zA-Z_]{1,20}", 1..8),
        salt in any::<u32>(),
    ) {
        let mut store = SqliteStore::in_memory().unwrap();
        let old = facts("a.rs", &old, 0);
        let new = facts("a.rs", &new, salt);
        let mut other = facts("b.rs", &other, 2);
        for edge in &mut other.edges {
            edge.dst = old.nodes.first().map_or_else(|| "m:a.rs".parse().unwrap(), |node| node.id.clone());
        }
        store.reindex(&old).unwrap();
        store.reindex(&other).unwrap();
        let before = snapshot(&store, "b.rs");
        store.reindex(&new).unwrap();
        prop_assert_eq!(&snapshot(&store, "b.rs"), &before);
        let mut expected = SqliteStore::in_memory().unwrap();
        expected.reindex(&new).unwrap();
        prop_assert_eq!(snapshot(&store, "a.rs"), snapshot(&expected, "a.rs"));
        prop_assert_eq!(store.file("a.rs").unwrap(), expected.file("a.rs").unwrap());
        prop_assert_eq!(store.prune(&["b.rs"]).unwrap(), 1);
        prop_assert!(store.file("a.rs").unwrap().is_none());
        prop_assert_eq!(snapshot(&store, "b.rs"), before);
        prop_assert!(store.search("platypus", 40).unwrap().iter()
            .all(|hit| hit.id.as_str().starts_with("s:b.rs#")));
    }
}

#[test]
fn bundled_fts5_queries_all_lexical_fields() {
    let mut store = SqliteStore::in_memory().unwrap();
    let enabled: i64 = store
        .connection
        .query_row(
            "SELECT sqlite_compileoption_used('ENABLE_FTS5')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(enabled, 1);
    store
        .reindex(&facts(
            "source_widgets/read_file.rs",
            &["HTTPServer_readFile".into()],
            1,
        ))
        .unwrap();
    for query in [
        "http", "server", "read", "file", "widgets", "aardvark", "platypus",
    ] {
        assert_eq!(store.search(query, 10).unwrap().len(), 1, "{query}");
    }
    assert_eq!(
        store.search("absent", 10).unwrap(),
        [] as [crate::domain::SearchHit; 0]
    );
    assert_eq!(
        store.search("http", 0).unwrap(),
        [] as [crate::domain::SearchHit; 0]
    );
    assert!(store.search("\"", 10).is_err());
    let many = facts(
        "many.rs",
        &(0..50).map(|i| format!("read{i}")).collect::<Vec<_>>(),
        1,
    );
    store.reindex(&many).unwrap();
    assert_eq!(store.search("aardvark", usize::MAX).unwrap().len(), 40);
}

#[test]
fn constraint_failure_rolls_back_the_complete_replacement() {
    let mut store = SqliteStore::in_memory().unwrap();
    let original = facts("a.rs", &["original".into()], 1);
    store.reindex(&original).unwrap();
    let before = snapshot(&store, "a.rs");
    let mut bad = facts("a.rs", &["replacement".into()], 2);
    bad.edges.push(bad.edges[0].clone());
    assert!(store.reindex(&bad).is_err());
    assert_eq!(snapshot(&store, "a.rs"), before);
    bad.edges.pop();
    store.reindex(&bad).unwrap();
    assert_ne!(snapshot(&store, "a.rs"), before);
    assert_eq!(
        store.search("original", 10).unwrap(),
        [] as [crate::domain::SearchHit; 0]
    );
    assert_eq!(store.search("replacement", 10).unwrap().len(), 1);
}

#[test]
fn invalid_ownership_and_stale_summaries_refuse_without_changes() {
    let mut store = SqliteStore::in_memory().unwrap();
    let valid = facts("a.rs", &["valid".into()], 1);
    store.reindex(&valid).unwrap();
    let before = snapshot(&store, "a.rs");
    let mut variants = vec![];
    let mut bad = valid.clone();
    bad.nodes[0].path = "other.rs".into();
    variants.push(bad);
    let mut bad = valid.clone();
    bad.edges[0].owner_path = "other.rs".into();
    variants.push(bad);
    let mut bad = valid.clone();
    bad.summaries[0].fingerprint = "stale".into();
    variants.push(bad);
    let mut bad = valid.clone();
    bad.nodes[0].lines = [2, 1];
    variants.push(bad);
    let mut bad = valid.clone();
    bad.edges[0].conf = Confidence::Exact;
    variants.push(bad);
    for bad in variants {
        assert!(matches!(
            store.reindex(&bad),
            Err(StoreError::InvalidFacts(_))
        ));
        assert_eq!(snapshot(&store, "a.rs"), before);
    }
    store.reindex(&valid).unwrap();
    assert_eq!(store.file("missing.rs").unwrap(), None);
}

#[test]
fn incompatible_schema_requires_rebuild() {
    let connection = Connection::open_in_memory().unwrap();
    connection.pragma_update(None, "user_version", 99).unwrap();
    assert!(matches!(
        SqliteStore::from_connection(connection),
        Err(StoreError::RebuildRequired(99))
    ));
    let mut store = SqliteStore::in_memory().unwrap();
    schema::initialize(&mut store.connection).unwrap();
}

#[test]
fn five_verb_rows_stay_stable_beside_shared_facts() {
    use crate::{ExtractionContract, FactKey, Retain};
    use dctx_core::datatypes::{DirContextTreeNode, NodeType};
    use dctx_core::facts::{ContentDigests, PackSnapshot};
    use dctx_core::ids::{Timestamp, ViewKey};
    use dctx_core::version::Version;
    use dctx_ports::FactStore;
    use std::path::PathBuf;

    let mut store = SqliteStore::in_memory().unwrap();
    let version: i64 = store
        .connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(version, 1);
    store
        .reindex(&facts("src/job.rs", &["work".into()], 1))
        .unwrap();
    let hits = store.search("work", 10).unwrap();
    assert_eq!(hits.len(), 1);
    let key = FactKey::from_bytes(
        b"pub fn work() {}",
        ExtractionContract::new(1, "rust-2024", "dctx-map-parse", "rev-1", "plan-1", 1).unwrap(),
    )
    .unwrap();
    assert_eq!(
        store.retain_fact("team", "root-a", &key, b"facts").unwrap(),
        Retain::Parsed
    );
    assert_eq!(
        store.retain_fact("team", "root-b", &key, b"facts").unwrap(),
        Retain::Reused
    );
    assert_eq!(
        store.retain_fact("team", "root-c", &key, b"facts").unwrap(),
        Retain::Reused
    );
    assert_eq!(store.parse_count().unwrap(), 1);
    assert_eq!(
        store.cached_bytes().unwrap(),
        crate::accounted_bytes(5).unwrap()
    );
    let view = ViewKey::parse("root-a").unwrap();
    let snapshot = PackSnapshot::new(
        DirContextTreeNode::new(NodeType::Directory, "", PathBuf::new()),
        Version::new_v1(),
        Timestamp(1),
        ContentDigests::new(),
    )
    .unwrap();
    store.publish(&view, &snapshot).unwrap();
    assert_eq!(
        store.load(&view).unwrap().unwrap().digest(),
        snapshot.digest()
    );
    let again = store.search("work", 10).unwrap();
    assert_eq!(again.len(), 1);
    assert_eq!(again[0].id.as_str(), hits[0].id.as_str());
    assert_eq!(store.file("src/job.rs").unwrap().unwrap().nodes.len(), 1);
}

#[test]
fn bundled_sqlite_is_at_least_3_54() {
    // SQLite before 3.54 has the WAL uninitialized-memory flaw behind RUSTSEC-2026-0459.
    assert!(
        rusqlite::version_number() >= 3_054_000,
        "bundled SQLite {} is older than 3.54.0",
        rusqlite::version()
    );
}
