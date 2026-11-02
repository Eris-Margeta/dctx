//! `map m:<file>` puts a file's explicit export list first (DCX-55c) from the index alone.
//! The list is read once at snapshot time and stored on the module node, so the outline rereads nothing,
//! and the stored list never shows up as a module signature.

#![forbid(unsafe_code)]

#[path = "common/mod.rs"]
mod common;

use dctx::memory::MemorySource;
use dctx::{MapRequest, SnapshotRequest, VecSink, Verb};

use common::open_session;

const MODULE: &[u8] = b"def alpha():\n    return 1\n\ndef beta():\n    return 2\n\n__all__ = [\"beta\"]\n";
const PLAIN: &[u8] = b"def alpha():\n    return 1\n\ndef beta():\n    return 2\n";

fn position(ids: &[String], id: &str) -> usize {
    ids.iter().position(|item| item == id).unwrap_or_else(|| panic!("{id} missing from {ids:?}"))
}

#[test]
fn outline_orders_by_the_indexed_export_list_without_rereading() {
    let mut source = MemorySource::new();
    source.insert_file("pkg/mod.py", MODULE, 0).expect("module");
    source.insert_file("pkg/plain.py", PLAIN, 0).expect("plain");
    let mut session = open_session(source);
    let handle = session.snapshot(&SnapshotRequest::immutable()).expect("snapshot");
    let reads = session.content_reads();
    assert_eq!(reads, 2, "the walk reads each file once");

    let outline = session.map(&handle, "m:pkg/mod.py", &mut VecSink::new()).expect("outline");
    let ids: Vec<String> = outline.records.iter().map(|record| record.id.clone()).collect();
    assert!(position(&ids, "s:pkg/mod.py#beta") < position(&ids, "s:pkg/mod.py#alpha"), "exported first: {ids:?}");
    let plain = session.map(&handle, "m:pkg/plain.py", &mut VecSink::new()).expect("plain outline");
    let ids: Vec<String> = plain.records.iter().map(|record| record.id.clone()).collect();
    assert!(position(&ids, "s:pkg/plain.py#alpha") < position(&ids, "s:pkg/plain.py#beta"), "source order: {ids:?}");
    assert_eq!(session.content_reads(), reads, "the outline order comes from the index");

    let mut explain = MapRequest::new(Verb::Explain);
    explain.target = "m:pkg/mod.py".to_string();
    let answer = session.query(&handle, &explain, &mut VecSink::new()).expect("explain");
    assert!(answer.records.iter().filter(|record| record.id.starts_with("m:")).all(|record| record.signature.is_none()));
}
