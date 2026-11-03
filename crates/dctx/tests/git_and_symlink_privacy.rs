// SPDX-License-Identifier: Apache-2.0
//! v1 card 1 on the shipped engine: `.git` stays closed under rule negations, and symlinks never leak outside bytes.
//! Ports desktop-api's `git_negations_never_list_or_read_git` to the `dctx` facade with a real [`dctx::FsSource`].
//! Every fixture lives in a private temporary directory; nothing here reads a home directory or spawns git.

#![forbid(unsafe_code)]

#[path = "common/mod.rs"]
mod common;

use std::fs;
use std::path::Path;
use std::sync::Arc;

use dctx::memory::CancelFlag;
use dctx::{FsSource, HostGrants, MapRequest, RuleTexts, SelectionPolicy, Session, SnapshotRequest, StoragePolicy, VecSink, Verb};

/// Bytes that must never appear in any answer.
const SENTINEL: &str = "PRIVATE_SENTINEL";

fn open(root: &Path, local_ignore: Option<&str>) -> Session {
    common::session_parts(
        FsSource::open(root).expect("source"),
        Arc::new(CancelFlag::new()),
        HostGrants::PermitAll,
        StoragePolicy::Ephemeral { max_bytes: 8 * 1024 * 1024 },
        SelectionPolicy::from_texts(&RuleTexts { local_ignore, ..RuleTexts::default() }),
    )
}

/// Runs read, map, explain, grep and find on a fresh session per snapshot mode and returns everything they printed,
/// followed by both graph exports (returned separately, since a refused target may echo its own id in an answer).
fn every_answer(root: &Path, rules: Option<&str>, targets: &[&str]) -> (String, String) {
    let mut text = String::new();
    let mut graph = String::new();
    for request in [SnapshotRequest::immutable(), SnapshotRequest::metadata()] {
        let mut session = open(root, rules);
        let handle = session.snapshot(&request).expect("snapshot");
        for target in targets {
            for verb in [Verb::Read, Verb::Map, Verb::Explain] {
                let mut query = MapRequest::new(verb);
                query.target = format!("m:{target}");
                let mut sink = VecSink::new();
                let _ = session.query(&handle, &query, &mut sink);
                text.push_str(&String::from_utf8_lossy(sink.as_bytes()));
            }
        }
        for verb in [Verb::Grep, Verb::Find] {
            let mut query = MapRequest::new(verb);
            query.query = "PRIVATE".to_string();
            let mut sink = VecSink::new();
            let _ = session.query(&handle, &query, &mut sink);
            text.push_str(&String::from_utf8_lossy(sink.as_bytes()));
        }
        for format in ["json", "mermaid"] {
            graph.push_str(&session.export_graph(&handle, format, &MapRequest::new(Verb::Map)).expect("export"));
        }
    }
    (text, graph)
}

#[test]
fn git_negations_never_list_or_read_git() {
    for (label, rules, secret) in [
        ("literal", "!.git/\n", ".git/config"),
        ("wildcard", "!.g*/\n", ".git/config"),
        ("nested", "!nested/.git/\n!nested/.git/**\n", "nested/.git/config"),
    ] {
        let dir = tempfile::tempdir().expect("temp");
        let root = dir.path();
        fs::create_dir_all(root.join(secret).parent().expect("parent")).expect("parent dir");
        fs::write(root.join("keep.rs"), b"fn keep_marker() {}\n").expect("keep");
        fs::write(root.join(secret), SENTINEL).expect("secret");
        let mut first = open(root, Some(rules));
        first.snapshot(&SnapshotRequest::immutable()).expect("snapshot");
        assert_eq!(first.content_reads(), 1, "{label}: only keep.rs is read");
        let (text, graph) = every_answer(root, Some(rules), &[secret, ".git"]);
        assert!(!text.contains(SENTINEL) && !graph.contains(SENTINEL), "{label} leaked .git bytes");
        assert!(!graph.contains(".git"), "{label} listed a .git path: {graph}");
        assert!(graph.contains("keep.rs"), "{label}: {graph}");
    }
}

#[cfg(unix)]
#[test]
fn symlinks_out_of_the_root_never_leak() {
    let outside = tempfile::tempdir().expect("outside");
    fs::write(outside.path().join("secret.txt"), SENTINEL).expect("secret");
    let dir = tempfile::tempdir().expect("temp");
    let root = dir.path();
    fs::write(root.join("keep.rs"), b"fn keep_marker() {}\n").expect("keep");
    std::os::unix::fs::symlink(outside.path().join("secret.txt"), root.join("link.txt")).expect("file link");
    std::os::unix::fs::symlink(outside.path(), root.join("linkdir")).expect("dir link");
    let (text, graph) = every_answer(root, None, &["link.txt", "linkdir", "linkdir/secret.txt"]);
    assert!(!text.contains(SENTINEL) && !graph.contains(SENTINEL), "a symlink leaked outside bytes");
    assert!(!graph.contains("secret.txt"), "a symlinked directory was walked: {graph}");
    let mut session = open(root, None);
    let handle = session.snapshot(&SnapshotRequest::immutable()).expect("snapshot");
    let mut sink = VecSink::new();
    session.read(&handle, "m:keep.rs", &mut sink).expect("read keep");
    assert!(String::from_utf8_lossy(sink.as_bytes()).contains("keep_marker"));
}
