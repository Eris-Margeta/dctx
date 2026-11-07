// SPDX-License-Identifier: Apache-2.0
//! One writer, two readers, and a budget-1 GC agree on complete generations.
//! Readers skip an empty store and otherwise decode the generation they pinned.
//! The std adapter reopens those bytes. A second process lock is busy until unlock.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use common::{open, req, segment};
#[cfg(any(unix, windows))]
use dctx_adapter_store::StoreOptions;
use dctx_adapter_store::ports::StoreFs;
use dctx_adapter_store::{GcBudget, MemoryFs, SegmentRole, StdFs, StoreError};
use dctx_map_segment::decode;

#[test]
fn writer_readers_and_gc_agree_on_valid_generations() {
    let store = Arc::new(open(MemoryFs::posix()));
    let stop = Arc::new(AtomicBool::new(false));
    let writer = {
        let store = Arc::clone(&store);
        thread::spawn(move || {
            for generation in 1..=6 {
                store
                    .publish(req("main", generation, segment(&format!("c{generation}"))))
                    .unwrap();
                thread::yield_now();
            }
        })
    };
    let mut readers = Vec::new();
    for _ in 0..2 {
        let store = Arc::clone(&store);
        let stop = Arc::clone(&stop);
        readers.push(thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                match store.pin("main") {
                    Ok(pin) => {
                        let digest = *pin.segments()[0].digest();
                        let bytes = store.read_segment(&pin, &digest).expect("read");
                        decode(&bytes, SegmentRole::Fact).expect("decode");
                        store.release(pin).expect("release");
                    }
                    Err(StoreError::InvalidRequest { reason: "empty" } | StoreError::Busy) => {}
                    Err(err) => panic!("{err}"),
                }
                thread::yield_now();
            }
        }));
    }
    let gc = {
        let store = Arc::clone(&store);
        let stop = Arc::clone(&stop);
        thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                store.gc(GcBudget { objects: 1 }).unwrap();
                thread::yield_now();
            }
            store.gc(GcBudget { objects: 64 }).unwrap();
        })
    };
    writer.join().unwrap();
    stop.store(true, Ordering::SeqCst);
    for reader in readers {
        reader.join().unwrap();
    }
    gc.join().unwrap();
    let pin = store.pin("main").unwrap();
    assert_eq!(pin.generation(), 6);
    let bytes = store
        .read_segment(&pin, pin.segments()[0].digest())
        .unwrap();
    assert_eq!(bytes, segment("c6"));
    decode(&bytes, SegmentRole::Fact).unwrap();
}

#[test]
#[cfg(any(unix, windows))]
fn std_round_trip_reopens_and_the_writer_lock_is_exclusive() {
    let dir = tempfile::tempdir().unwrap();
    let store = dctx_adapter_store::SegmentStore::open(
        StdFs::open(dir.path()).unwrap(),
        StoreOptions::capped(8 * 1024 * 1024),
    )
    .unwrap();
    let first = segment("std-1");
    store.publish(req("main", 1, first.clone())).unwrap();
    let old_pin = store.pin("main").unwrap();
    store.publish(req("main", 2, segment("std-2"))).unwrap();
    assert_eq!(
        store
            .read_segment(&old_pin, old_pin.segments()[0].digest())
            .unwrap(),
        first
    );
    store.release(old_pin).unwrap();
    drop(store);
    let reopened = dctx_adapter_store::SegmentStore::open(
        StdFs::open(dir.path()).unwrap(),
        StoreOptions::capped(8 * 1024 * 1024),
    )
    .unwrap();
    let pin = reopened.pin("main").unwrap();
    assert_eq!(pin.generation(), 2);
    let bytes = reopened
        .read_segment(&pin, pin.segments()[0].digest())
        .unwrap();
    assert_eq!(bytes, segment("std-2"));
    drop(reopened);
    assert_writer_lock_exclusive(dir.path());
}

fn assert_writer_lock_exclusive(root: &std::path::Path) {
    let fs1 = StdFs::open(root).unwrap();
    fs1.try_lock("main").unwrap();
    let fs2 = StdFs::open(root).unwrap();
    assert_eq!(fs2.try_lock("main").unwrap_err(), StoreError::Busy);
    fs1.unlock("main").unwrap();
    fs2.try_lock("main").unwrap();
    fs2.unlock("main").unwrap();
}

#[cfg(windows)]
#[test]
fn std_windows_durability_publishes_every_generation_and_reopens() {
    use dctx_adapter_store::SegmentStore;
    let dir = tempfile::tempdir().unwrap();
    let fs = StdFs::open(dir.path()).unwrap();
    assert!(fs.capabilities().sync);
    assert!(fs.capabilities().atomic_replace);
    fs.sync_dir().unwrap();
    let store = SegmentStore::open(fs, StoreOptions::capped(8 * 1024 * 1024)).unwrap();
    // Third publish replaces both current and previous; all objects and the
    // recovered pointer must remain valid after dropping the original adapter.
    for generation in 1..=3 {
        store
            .publish(req(
                "main",
                generation,
                segment(&format!("windows-{generation}")),
            ))
            .unwrap();
    }
    drop(store);
    let reopened = SegmentStore::open(
        StdFs::open(dir.path()).unwrap(),
        StoreOptions::capped(8 * 1024 * 1024),
    )
    .unwrap();
    let recovered = reopened.recover().unwrap();
    assert_eq!(recovered.generation, Some(3));
    assert!(!recovered.repaired_pointer);
    let pin = reopened.pin("main").unwrap();
    assert_eq!(pin.generation(), 3);
    assert_eq!(
        reopened
            .read_segment(&pin, pin.segments()[0].digest())
            .unwrap(),
        segment("windows-3")
    );
    reopened.release(pin).unwrap();
    drop(reopened);
    assert_writer_lock_exclusive(dir.path());
}
