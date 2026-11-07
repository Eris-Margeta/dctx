// SPDX-License-Identifier: Apache-2.0
//! Queries share one generation across files. Pages and the digest stay the same.
//! Spec: DCTX-COLD-SCAN.md C2 (E-02i).
//! Depends on the session facade and an in-memory multi-file source.
//! Used by `cargo nextest -p dctx`.
//! A query raises the generation Arc count by one and does not keep a second copy.

#![forbid(unsafe_code)]

#[path = "common/mod.rs"]
mod common;

use dctx::memory::MemorySource;
use dctx::{MapRequest, SnapshotRequest, VecSink, Verb};

use common::open_session;

#[test]
fn queries_share_one_generation_and_keep_records_and_digest() {
    let mut source = MemorySource::new();
    source
        .insert_file("src/a.rs", b"fn alpha() {}\nfn beta() {}\n", 0)
        .expect("a");
    source
        .insert_file("src/b.rs", b"fn gamma() {}\n", 0)
        .expect("b");
    source
        .insert_file("README.md", b"# Fixture\n", 0)
        .expect("readme");
    let mut session = open_session(source);
    let handle = session
        .snapshot(&SnapshotRequest::immutable())
        .expect("snapshot");
    let digest = handle.digest().to_string();
    assert_eq!(handle.generation(), 1);
    let (live, seen) = session.generation_shares(&handle).expect("before");
    assert_eq!((live, seen), (1, 0));

    let full = map_children(&mut session, &handle, None);
    let paged = map_children(&mut session, &handle, Some(1));
    assert_eq!(paged.records, full.records);
    assert!(paged.records.len() >= 2, "continuation needs two children");
    assert_eq!(paged.records[0].id, "s:src/a.rs#alpha");
    assert_eq!(paged.records[1].id, "s:src/a.rs#beta");
    assert_eq!(paged.cursors.len(), paged.records.len() - 1);
    assert!(
        paged
            .cursors
            .iter()
            .all(|cursor| cursor.starts_with("v1|") && cursor.contains("|map|"))
    );
    assert_eq!(full.snapshot, digest);
    assert_eq!(paged.snapshot, digest);

    let again = map_children(&mut session, &handle, None);
    assert_eq!(again.records, full.records);
    assert_eq!(again.snapshot, digest);

    let alpha = find(&mut session, &handle, "alpha");
    let gamma = find(&mut session, &handle, "gamma");
    let fixture = find(&mut session, &handle, "Fixture");
    assert_eq!(alpha.records.len(), 1);
    assert_eq!(alpha.records[0].path, "src/a.rs");
    assert_eq!(alpha.records[0].id, "s:src/a.rs#alpha");
    assert_eq!(gamma.records.len(), 1);
    assert_eq!(gamma.records[0].path, "src/b.rs");
    assert_eq!(gamma.records[0].id, "s:src/b.rs#gamma");
    assert_eq!(fixture.records.len(), 1);
    assert_eq!(fixture.records[0].path, "README.md");
    assert_eq!(fixture.records[0].kind, "heading");
    assert_eq!(alpha.snapshot, digest);
    assert_eq!(gamma.snapshot, digest);
    assert_eq!(fixture.snapshot, digest);
    assert_eq!(find(&mut session, &handle, "alpha").records, alpha.records);

    let (live, seen) = session.generation_shares(&handle).expect("after");
    assert_eq!(live, 1, "a query must not retain another generation");
    assert_eq!(seen, 2, "a query borrows the stored generation");
}

struct Page {
    records: Vec<dctx::AnswerRecord>,
    snapshot: String,
    cursors: Vec<String>,
}

fn map_children(
    session: &mut dctx::Session,
    handle: &dctx::SnapshotHandle,
    records: Option<u64>,
) -> Page {
    let mut request = MapRequest::new(Verb::Map);
    request.target = "m:src/a.rs".to_string();
    request.limits.records = records;
    let mut page = Page {
        records: Vec::new(),
        snapshot: String::new(),
        cursors: Vec::new(),
    };
    let mut finished = false;
    for _ in 0..8 {
        let mut sink = VecSink::new();
        let answer = session.query(handle, &request, &mut sink).expect("map");
        assert!(!sink.is_empty());
        page.snapshot = answer.snapshot;
        let count = answer.records.len();
        page.records.extend(answer.records);
        if answer.more == 0 {
            assert!(answer.next.is_none());
            finished = true;
            break;
        }
        let cursor = answer.next.expect("cursor");
        if let Some(cap) = records {
            assert_eq!(u64::try_from(count).unwrap_or(u64::MAX), cap);
        }
        request.continuation = Some(cursor.clone());
        page.cursors.push(cursor);
    }
    assert!(finished, "pages did not cover the map");
    page
}

fn find(session: &mut dctx::Session, handle: &dctx::SnapshotHandle, query: &str) -> Page {
    let mut request = MapRequest::new(Verb::Find);
    request.query = query.to_string();
    let mut sink = VecSink::new();
    let answer = session.query(handle, &request, &mut sink).expect("find");
    Page {
        records: answer.records,
        snapshot: answer.snapshot,
        cursors: Vec::new(),
    }
}
