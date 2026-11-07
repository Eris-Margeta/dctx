// SPDX-License-Identifier: Apache-2.0
//! v1 privacy gate: no map verb, export or pack ever serves the bytes of a sensitive file, and no snapshot reads one.
//! Covers env files, PEM keys, SSH private keys, credentials files, `.npmrc` and `secrets.yaml` (ADR-014; no override).
//! Ordinary files with look-alike names (`src/credentials.py`) stay readable, so the deny is not over-broad.
//! The fixtures are throwaway in-memory canaries, not the planted fake secrets of other tests.

#![forbid(unsafe_code)]

#[path = "common/mod.rs"]
mod common;

use dctx::memory::MemorySource;
use dctx::{MapRequest, PackRequest, SnapshotRequest, VecSink, Verb};

use common::{PING, open_session};

/// Paths of the sensitive canary files, one per shape the deny-list must cover.
const SENSITIVE: [&str; 10] = [
    ".env",
    ".env.local",
    "config/prod.env",
    "certs/server.pem",
    "id_rsa",
    "keys/id_ed25519",
    "credentials",
    "config/credentials.json",
    ".npmrc",
    "deploy/secrets.yaml",
];

/// Canary written into every sensitive file.
const CANARY: &str = "CANARYV1SECRETVALUE";

/// A look-alike source file that must stay readable.
const LOOK_ALIKE: &[u8] = b"def load_credentials():\n    return 'PLAINV1CRED'\n";

fn canary_source() -> MemorySource {
    let mut source = MemorySource::new();
    source.insert_file("src/lib.rs", PING, 0).expect("lib");
    source
        .insert_file("src/credentials.py", LOOK_ALIKE, 0)
        .expect("look-alike");
    for path in SENSITIVE {
        let body = format!("api_key: {CANARY}\npassword = {CANARY}\n");
        source
            .insert_file(path, body.as_bytes(), 0)
            .expect("sensitive file");
    }
    source
}

fn assert_clean(label: &str, bytes: &[u8]) {
    assert!(
        !String::from_utf8_lossy(bytes).contains(CANARY),
        "{label} leaked the canary"
    );
}

#[test]
fn no_map_answer_contains_a_sensitive_canary() {
    for request in [SnapshotRequest::immutable(), SnapshotRequest::metadata()] {
        let mut session = open_session(canary_source());
        let handle = session.snapshot(&request).expect("snapshot");
        for path in SENSITIVE {
            for verb in [
                Verb::Read,
                Verb::Map,
                Verb::Explain,
                Verb::Neighbors,
                Verb::Skeleton,
            ] {
                let mut query = MapRequest::new(verb);
                query.target = format!("m:{path}");
                let mut sink = VecSink::new();
                let _ = session.query(&handle, &query, &mut sink);
                assert_clean(path, sink.as_bytes());
            }
        }
        for literal in ["CANARYV1", "api_key", "password"] {
            for within in [
                None,
                Some("config"),
                Some("deploy"),
                Some("keys"),
                Some("certs"),
            ] {
                for ignore_case in [false, true] {
                    let mut grep = MapRequest::new(Verb::Grep);
                    grep.query = literal.to_string();
                    grep.within = within.map(str::to_string);
                    grep.ignore_case = ignore_case;
                    let mut sink = VecSink::new();
                    let _ = session.query(&handle, &grep, &mut sink);
                    assert_clean("grep", sink.as_bytes());
                }
                let mut find = MapRequest::new(Verb::Find);
                find.query = literal.to_string();
                find.within = within.map(str::to_string);
                let mut sink = VecSink::new();
                let _ = session.query(&handle, &find, &mut sink);
                assert_clean("find", sink.as_bytes());
            }
        }
        let mut sink = VecSink::new();
        let _ = session.hubs(&handle, &MapRequest::new(Verb::Neighbors), &mut sink);
        assert_clean("hubs", sink.as_bytes());
        for format in ["json", "mermaid"] {
            let exported = session
                .export_graph(&handle, format, &MapRequest::new(Verb::Map))
                .expect("export");
            assert_clean(format, exported.as_bytes());
        }
    }
}

#[test]
fn a_snapshot_reads_only_the_ordinary_files() {
    let mut session = open_session(canary_source());
    session
        .snapshot(&SnapshotRequest::immutable())
        .expect("snapshot");
    // `src/lib.rs` and `src/credentials.py` are read; the ten sensitive files cost nothing.
    assert_eq!(session.content_reads(), 2);
}

#[test]
fn a_metadata_snapshot_refuses_to_read_any_sensitive_node() {
    let mut session = open_session(canary_source());
    let handle = session
        .snapshot(&SnapshotRequest::metadata())
        .expect("snapshot");
    for path in SENSITIVE {
        let mut sink = VecSink::new();
        let error = session
            .read(&handle, &format!("m:{path}"), &mut sink)
            .expect_err(path);
        assert_eq!(error, dctx::DctxError::PolicyDenied, "{path}");
        assert!(sink.is_empty(), "{path}");
    }
}

#[test]
fn the_look_alike_source_file_stays_readable() {
    let mut session = open_session(canary_source());
    let handle = session
        .snapshot(&SnapshotRequest::immutable())
        .expect("snapshot");
    let mut sink = VecSink::new();
    session
        .read(&handle, "m:src/credentials.py", &mut sink)
        .expect("read look-alike");
    assert!(String::from_utf8_lossy(sink.as_bytes()).contains("PLAINV1CRED"));
}

#[test]
fn pack_never_prints_a_sensitive_canary() {
    for format in [1, 2] {
        let mut session = open_session(canary_source());
        let mut request = PackRequest::new();
        request.output_format = format;
        let mut sink = VecSink::new();
        session.pack(&request, &mut sink).expect("pack");
        assert_clean("pack", sink.as_bytes());
    }
}
