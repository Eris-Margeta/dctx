// SPDX-License-Identifier: Apache-2.0
//! Concurrent production readers, serialized writers and coherent WAL snapshots.
//! Implements CM-6c and SPEC-MAP-MODE section 8 on a shared on-disk index.
//! Channels bound waits so a mistakenly exclusive reader lock fails promptly.
use dctx_map_query::{FileSource, Index, Request, Verb};
use std::{
    fs,
    sync::{Arc, Barrier, mpsc},
    thread,
    time::Duration,
};

#[test]
fn eight_readers_and_one_writer_keep_consistent_snapshots_without_busy_errors() {
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let db = cache.path().join("index.sqlite");
    fs::write(root.path().join("version.rs"), "pub fn version_old() {}").unwrap();
    let source = FileSource::new(root.path()).unwrap();
    Index::open("fixture", &db)
        .unwrap()
        .query(&source, &Request::new(Verb::Find, "version"))
        .unwrap();
    let lock_path = db.with_extension("writer.lock");
    fs::write(&lock_path, "legacy pid 12345").unwrap();
    let lock = fs::File::options()
        .read(true)
        .write(true)
        .open(&lock_path)
        .unwrap();
    lock.lock().unwrap();
    let (ready_tx, ready_rx) = mpsc::channel();
    let published = Arc::new(Barrier::new(9));
    let mut readers = Vec::new();
    for _ in 0..8 {
        let root = root.path().to_owned();
        let db = db.clone();
        let ready = ready_tx.clone();
        let published = published.clone();
        readers.push(thread::spawn(move || {
            let source = FileSource::new(&root).unwrap();
            let mut reader = Index::open("fixture", &db).unwrap();
            let request = Request::new(Verb::Find, "version");
            let answer = reader.query(&source, &request).unwrap();
            assert!(
                answer
                    .records
                    .iter()
                    .any(|r| r.id.as_str() == "s:version.rs#version_old")
            );
            assert_eq!(source.content_reads(), 0);
            ready.send(()).unwrap();
            published.wait();
            let old = reader.answer(&source, &request).unwrap();
            assert!(
                old.records
                    .iter()
                    .any(|r| r.id.as_str() == "s:version.rs#version_old" && r.stale)
            );
            assert!(
                !old.records
                    .iter()
                    .any(|r| r.id.as_str().contains("version_new"))
            );
            drop(reader);
            let mut reader = Index::open("fixture", &db).unwrap();
            let new = reader.query(&source, &request).unwrap();
            assert!(
                new.records
                    .iter()
                    .any(|r| r.id.as_str() == "s:version.rs#version_new" && !r.stale)
            );
            assert!(
                !new.records
                    .iter()
                    .any(|r| r.id.as_str().contains("version_old"))
            );
            assert_eq!(source.content_reads(), 0);
        }));
    }
    for _ in 0..8 {
        ready_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("warm reader tried to acquire writer lock");
    }
    drop(lock);
    fs::write(root.path().join("version.rs"), "pub fn version_new() {}").unwrap();
    let mut writer = Index::open("fixture", &db).unwrap();
    let answer = writer
        .query(&source, &Request::new(Verb::Find, "version"))
        .unwrap();
    assert!(
        answer
            .records
            .iter()
            .any(|r| r.id.as_str() == "s:version.rs#version_new")
    );
    assert_eq!(writer.report().parsed, 1);
    assert_eq!(writer.report().replaced, 1);
    published.wait();
    for reader in readers {
        reader.join().unwrap();
    }
}

#[test]
fn competing_cold_queries_recheck_after_waiting_for_the_single_writer() {
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let db = cache.path().join("index.sqlite");
    fs::write(root.path().join("version.rs"), "pub fn version_new() {}").unwrap();
    let barrier = Arc::new(Barrier::new(8));
    let mut readers = Vec::new();
    for _ in 0..8 {
        let root = root.path().to_owned();
        let db = db.clone();
        let barrier = barrier.clone();
        readers.push(thread::spawn(move || {
            barrier.wait();
            let source = FileSource::new(root).unwrap();
            let mut index = Index::open("fixture", db).unwrap();
            let answer = index
                .query(&source, &Request::new(Verb::Find, "version"))
                .unwrap();
            assert!(
                answer
                    .records
                    .iter()
                    .any(|r| r.id.as_str() == "s:version.rs#version_new")
            );
            index.report().parsed
        }));
    }
    let parsed: usize = readers.into_iter().map(|r| r.join().unwrap()).sum();
    assert_eq!(parsed, 1);
}
