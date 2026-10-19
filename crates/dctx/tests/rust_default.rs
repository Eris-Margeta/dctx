//! Default Rust extraction goes through the bounded syn adapter.
//! Implements PARSERS-v1 (Rust row) and facade review F12.
//! Depends on Session and `syntax-rust` (`dctx-lex-rust`, syn 3.0.9). The dctx nextest target runs it.
//! Comment and string decoys are not symbols; `Real` is the positive control.

#![forbid(unsafe_code)]

#[path = "common/mod.rs"]
mod common;

use dctx::memory::MemorySource;
use dctx::{SnapshotRequest, VecSink};

use common::open_session;

const SOURCE: &[u8] = b"// fn Phantom() {}\n/* fn Blocked() {} */\nfn Real() {\n    let _text = \"fn Ghost() {}\";\n    let _raw = r#\"fn RawDecoy() {}\"#;\n}\n";

#[test]
fn default_snapshot_uses_bounded_syn_and_rejects_comment_and_string_decoys() {
    let mut source = MemorySource::new();
    source.insert_file("a.rs", SOURCE, 0).expect("file");
    let mut session = open_session(source);
    let handle = session.snapshot(&SnapshotRequest::immutable()).expect("snapshot");

    let mut sink = VecSink::new();
    let real = session.find(&handle, "Real", &mut sink).expect("real");
    assert_eq!(real.precision.coverage, "syntax");
    assert_eq!(real.precision.parser, "dctx-lex-rust/syn-3.0.9");
    assert_eq!(real.precision.completion, "complete");
    assert_eq!(real.records.len(), 1);
    assert_eq!(real.records[0].id, "s:a.rs#Real");
    assert_eq!(real.records[0].kind, "fn");

    for decoy in ["Phantom", "Blocked", "Ghost", "RawDecoy"] {
        let mut sink = VecSink::new();
        let found = session.find(&handle, decoy, &mut sink).expect(decoy);
        assert!(found.records.is_empty(), "{decoy} was fabricated");
    }
}
