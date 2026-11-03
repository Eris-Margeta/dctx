// SPDX-License-Identifier: Apache-2.0
//! Map mode never serves the bytes of env-shaped files (`.env`, `.env.*`, `*.env`, `.envrc`).
//! A canary in each of them must appear in no read, map, find, grep or export answer.
//! The fixtures here are throwaway in-memory canaries, not the planted fake secrets of other tests.

#![forbid(unsafe_code)]

#[path = "common/mod.rs"]
mod common;

use dctx::memory::MemorySource;
use dctx::{MapRequest, SnapshotRequest, VecSink, Verb};

use common::{open_session, PING};

/// Paths of the env-shaped canary files.
const ENV_PATHS: [&str; 4] = [".env", "settings.env", "config/prod.env", ".envrc"];

fn canary_source() -> MemorySource {
    let mut source = MemorySource::new();
    source.insert_file("src/lib.rs", PING, 0).expect("lib");
    for path in ENV_PATHS {
        source.insert_file(path, b"K=CANARYENVVALUE\n", 0).expect("env file");
    }
    source
}

fn assert_clean(label: &str, bytes: &[u8]) {
    assert!(!String::from_utf8_lossy(bytes).contains("CANARYENVVALUE"), "{label} leaked the canary");
}

#[test]
fn no_map_answer_contains_an_env_canary() {
    for request in [SnapshotRequest::immutable(), SnapshotRequest::metadata()] {
        let mut session = open_session(canary_source());
        let handle = session.snapshot(&request).expect("snapshot");
        for path in ENV_PATHS {
            for verb in [Verb::Read, Verb::Map, Verb::Explain] {
                let mut query = MapRequest::new(verb);
                query.target = format!("m:{path}");
                let mut sink = VecSink::new();
                let _ = session.query(&handle, &query, &mut sink);
                assert_clean(path, sink.as_bytes());
            }
        }
        let mut grep = MapRequest::new(Verb::Grep);
        grep.query = "CANARY".to_string();
        let mut sink = VecSink::new();
        let _ = session.query(&handle, &grep, &mut sink);
        assert_clean("grep", sink.as_bytes());
        let mut sink = VecSink::new();
        let _ = session.find(&handle, "CANARY", &mut sink);
        assert_clean("find", sink.as_bytes());
        for format in ["json", "mermaid"] {
            let exported = session.export_graph(&handle, format, &MapRequest::new(Verb::Map)).expect("export");
            assert_clean(format, exported.as_bytes());
        }
    }
}

#[test]
fn an_env_file_is_never_read_from_the_source() {
    let mut session = open_session(canary_source());
    session.snapshot(&SnapshotRequest::immutable()).expect("snapshot");
    // Only `src/lib.rs` is read; the four env-shaped files cost nothing.
    assert_eq!(session.content_reads(), 1);
}

#[test]
fn a_metadata_snapshot_refuses_to_read_an_env_node() {
    let mut session = open_session(canary_source());
    let handle = session.snapshot(&SnapshotRequest::metadata()).expect("snapshot");
    let mut sink = VecSink::new();
    let error = session.read(&handle, "m:.env", &mut sink).expect_err("env read");
    assert_eq!(error, dctx::DctxError::PolicyDenied);
    assert!(sink.is_empty());
}
