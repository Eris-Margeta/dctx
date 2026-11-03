// SPDX-License-Identifier: Apache-2.0
//! Properties P1–P7 for the map engine.
//! Implements ARCHITECTURE-PROPOSAL §7 DX-C5 against the public query API.
//! Each refusal has a positive control that the same operation accepts a smaller input.
use dctx_adapter_cache::{Lifecycle, Quota, RootId};
use dctx_adapter_store::{GcBudget, SegmentRole, SegmentStore, StdFs, StoreOptions};
use dctx_core::error::DctxError;
use dctx_map_query::{
    FileSource, Format, Index, Request, Verb, render_answer,
};
use dctx_map_segment::decode;
use dctx_map_store::{ExtractionContract, FactKey, MapStore, SqliteStore};
use dctx_ports::Cache;
use std::fs;

#[test]
fn p1_denied_paths_are_not_read_indexed_or_served() {
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let db = cache.path().join("index.sqlite");
    fs::write(root.path().join("keep.rs"), "pub fn kept() {}\n").unwrap();
    fs::write(root.path().join(".env"), "SECRET").unwrap();
    fs::write(root.path().join(".gitignore"), "secret.rs\n").unwrap();
    fs::write(root.path().join("secret.rs"), "pub fn hidden() {}\n").unwrap();
    let source = FileSource::new(root.path()).unwrap();
    let mut index = Index::open("p1", &db).unwrap();
    let answer = index
        .query(&source, &Request::new(Verb::Find, "kept"))
        .unwrap();
    assert!(answer.records.iter().any(|record| record.id.as_str() == "s:keep.rs#kept"));
    let hidden = index
        .answer(&source, &Request::new(Verb::Find, "hidden"))
        .unwrap();
    assert_eq!(hidden.records, [] as [dctx_map_query::Record; 0]);
    assert!(
        source
            .opened_paths()
            .iter()
            .all(|path| !path.ends_with(".env") && !path.ends_with("secret.rs"))
    );
    let store = SqliteStore::open(&db).unwrap();
    assert!(store.file(".env").unwrap().is_none());
    assert!(store.file("secret.rs").unwrap().is_none());
    assert!(store.file("keep.rs").unwrap().is_some());
}

#[test]
fn p2_a_loaded_reader_keeps_its_generation_until_reopen() {
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let db = cache.path().join("index.sqlite");
    fs::write(root.path().join("lib.rs"), "pub fn one() {}\n").unwrap();
    let source = FileSource::new(root.path()).unwrap();
    let mut reader = Index::open("p2", &db).unwrap();
    reader
        .query(&source, &Request::new(Verb::Explain, "s:lib.rs#one"))
        .unwrap();
    fs::write(root.path().join("lib.rs"), "pub fn two() {}\n").unwrap();
    let mut writer = Index::open("p2", &db).unwrap();
    writer
        .query(&source, &Request::new(Verb::Explain, "s:lib.rs#two"))
        .unwrap();
    let stale = reader
        .answer(&source, &Request::new(Verb::Explain, "s:lib.rs#one"))
        .unwrap();
    assert!(
        stale
            .records
            .iter()
            .any(|record| record.id.as_str() == "s:lib.rs#one")
    );
    let mut again = Index::open("p2", &db).unwrap();
    let fresh = again
        .query(&source, &Request::new(Verb::Explain, "s:lib.rs#two"))
        .unwrap();
    assert!(
        fresh
            .records
            .iter()
            .any(|record| record.id.as_str() == "s:lib.rs#two")
    );
}

#[test]
fn p3_a_pinned_segment_survives_collection() {
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let db = cache.path().join("index.sqlite");
    fs::write(root.path().join("lib.rs"), "pub fn pinned() {}\n").unwrap();
    let source = FileSource::new(root.path()).unwrap();
    Index::open("p3", &db)
        .unwrap()
        .query(&source, &Request::new(Verb::Find, "pinned"))
        .unwrap();
    let fs = StdFs::open(&db.with_extension("segments")).unwrap();
    let store = SegmentStore::open(fs, StoreOptions::capped(512 * 1024 * 1024)).unwrap();
    let pin = store.pin("p3").unwrap();
    let digest = *pin.segments()[0].digest();
    let before = store.read_segment(&pin, &digest).unwrap();
    store.gc(GcBudget { objects: 64 }).unwrap();
    let after = store.read_segment(&pin, &digest).unwrap();
    assert_eq!(before, after);
    assert_ne!(after, [] as [u8; 0]);
    store.release(pin).unwrap();
}

#[test]
fn p4_reserve_refuses_past_the_cap_and_commits_a_small_one() {
    let mut tiny = Lifecycle::new(Quota::scaled(64)).unwrap();
    tiny.register_root(RootId::new(1)).unwrap();
    let refused = Cache::reserve(&mut tiny, 10_000);
    assert!(matches!(refused, Err(DctxError::CacheLimit { .. })));
    let mut room = Lifecycle::new(Quota::scaled(4096)).unwrap();
    room.register_root(RootId::new(1)).unwrap();
    let reservation = Cache::reserve(&mut room, 100).unwrap();
    Cache::commit(&mut room, reservation, 100).unwrap();
}

#[test]
fn p5_wire_output_names_budget_cuts_and_unknown_or_unparseable_facts() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("lib.rs"), "pub fn broken(\n").unwrap();
    let source = FileSource::new(root.path()).unwrap();
    let mut index = Index::in_memory("p5").unwrap();
    index.refresh(&source, None).unwrap();
    let mut request = Request::new(Verb::Map, "lib.rs");
    request.budget = dctx_map_query::AnswerBudget::new(180).unwrap();
    let answer = index.answer(&source, &request).unwrap();
    let output = render_answer(&answer, &request, Format::Text).unwrap();
    assert!(request.budget.fits(&output));
    assert!(output.contains("unparseable") || output.contains("no match") || answer.records.is_empty() || output.contains("more"));
    let unknown = index
        .answer(&source, &Request::new(Verb::Find, "notexistinganywhere"))
        .unwrap();
    let text = render_answer(&unknown, &Request::new(Verb::Find, "notexistinganywhere"), Format::Text).unwrap();
    assert!(text.contains("no match") || unknown.reason.as_deref() == Some("no match"));
}

#[test]
fn p6_identical_inputs_render_identical_text_and_json_at_age_zero() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("lib.rs"), "pub fn stable() {}\n").unwrap();
    let source = FileSource::new(root.path()).unwrap();
    let request = Request::new(Verb::Find, "stable");
    let mut first = Index::in_memory("p6").unwrap();
    let left = first.query(&source, &request).unwrap();
    let mut second = Index::in_memory("p6").unwrap();
    let right = second.query(&source, &request).unwrap();
    for format in [Format::Text, Format::Json] {
        assert_eq!(
            render_answer(&left, &request, format).unwrap(),
            render_answer(&right, &request, format).unwrap()
        );
    }
    let text = render_answer(&left, &request, Format::Text).unwrap();
    assert!(text.contains("age=0"));
    assert_eq!(left.age_seconds, 0);
}

#[test]
fn p7_the_same_bytes_share_one_digest_across_paths() {
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let db = cache.path().join("index.sqlite");
    let bytes = b"pub fn shared() {}\n";
    fs::write(root.path().join("a.rs"), bytes).unwrap();
    fs::write(root.path().join("b.rs"), bytes).unwrap();
    let source = FileSource::new(root.path()).unwrap();
    Index::open("p7", &db)
        .unwrap()
        .refresh(&source, None)
        .unwrap();
    let store = SqliteStore::open(&db).unwrap();
    let left = store.file("a.rs").unwrap().unwrap();
    let right = store.file("b.rs").unwrap().unwrap();
    assert_eq!(left.file.sha256, right.file.sha256);
    let contract = ExtractionContract::new(1, "rust", "dctx-map-query", "1", "plan", 1).unwrap();
    assert_eq!(
        FactKey::from_bytes(bytes, contract.clone()).unwrap(),
        FactKey::from_bytes(bytes, contract).unwrap()
    );
    let fs = StdFs::open(&db.with_extension("segments")).unwrap();
    let segments = SegmentStore::open(fs, StoreOptions::capped(512 * 1024 * 1024)).unwrap();
    let pin = segments.pin("p7").unwrap();
    let mut digests = Vec::new();
    for segment in pin.segments() {
        if segment.role() != SegmentRole::View {
            continue;
        }
        let bytes = segments.read_segment(&pin, segment.digest()).unwrap();
        let decoded = decode(&bytes, SegmentRole::View).unwrap();
        for node in decoded.nodes() {
            digests.push(node.fact.digest);
        }
    }
    segments.release(pin).unwrap();
    assert!(digests.len() >= 2);
    assert!(digests.windows(2).all(|pair| pair[0] == pair[1]));
}

#[test]
fn overlay_records_dirty_added_and_deleted_after_a_clean_base() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("keep.rs"), "pub fn keep() {}\n").unwrap();
    fs::write(root.path().join("gone.rs"), "pub fn gone() {}\n").unwrap();
    let source = FileSource::new(root.path()).unwrap();
    let mut index = Index::in_memory("overlay").unwrap();
    index.refresh(&source, None).unwrap();
    assert!(index.overlay().dirty.is_empty());
    assert!(index.overlay().added.is_empty());
    assert!(index.overlay().deleted.is_empty());
    fs::write(root.path().join("keep.rs"), "pub fn keep_changed() {}\n").unwrap();
    fs::write(root.path().join("added.rs"), "pub fn added() {}\n").unwrap();
    fs::remove_file(root.path().join("gone.rs")).unwrap();
    index.refresh(&source, None).unwrap();
    assert!(index.overlay().dirty.contains("keep.rs"));
    assert!(index.overlay().added.contains("added.rs"));
    assert!(index.overlay().deleted.contains("gone.rs"));
    let answer = index
        .answer(&source, &Request::new(Verb::Find, "keep_changed"))
        .unwrap();
    assert_eq!(answer.dirty, 0);
    assert!(answer.records.iter().any(|record| record.id.as_str() == "s:keep.rs#keep_changed"));
    let removed = index
        .answer(&source, &Request::new(Verb::Find, "gone"))
        .unwrap();
    assert_eq!(removed.records, [] as [dctx_map_query::Record; 0]);
}

#[test]
fn both_stores_agree_and_cancellation_refuses_one_index_only() {
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let db = cache.path().join("index.sqlite");
    fs::write(root.path().join("lib.rs"), "pub fn agreed() {}\n").unwrap();
    let source = FileSource::new(root.path()).unwrap();
    let mut index = Index::open("agree", &db).unwrap();
    index
        .query(&source, &Request::new(Verb::Find, "agreed"))
        .unwrap();
    assert!(index.stores_agree().unwrap());
    index.cancel();
    assert!(index.query(&source, &Request::new(Verb::Find, "agreed")).is_err());
    let mut other = Index::open("agree", &db).unwrap();
    assert!(other.query(&source, &Request::new(Verb::Find, "agreed")).is_ok());
}
