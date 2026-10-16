//! P3: reachable segments survive pin, compact, purge, and shared-root schedules.
//! A lease keeps bytes after the pointer is gone. GC unlinks them only after release.
//! Omitting leases from the mark set is the falsifying mutation and does unlink.
//! A pin held on another thread still reads after GC.

mod common;

use std::sync::Arc;
use std::thread;

use common::{open, req, segment};
use dctx_adapter_store::{GcBudget, MarkMode, MemoryFs};

#[test]
fn p3_pin_gc_interleavings() {
    shared_roots_survive_until_the_last_root_drops_them();
    retire_root_keeps_leased_bytes();
    omit_leases_unlinks_a_pinned_segment();
    pin_held_across_gc_still_reads();
}

fn shared_roots_survive_until_the_last_root_drops_them() {
    let store = open(MemoryFs::posix());
    let shared = segment("shared");
    store.publish(req("a", 1, shared.clone())).unwrap();
    store.publish(req("b", 1, shared.clone())).unwrap();
    let pin = store.pin("a").unwrap();
    let digest = *pin.segments()[0].digest();
    store.publish(req("a", 2, segment("next-a"))).unwrap();
    store.gc(GcBudget { objects: 64 }).unwrap();
    assert!(store.contains_segment(&digest).unwrap());
    assert_eq!(store.read_segment(&pin, &digest).unwrap(), shared);
    store.drop_previous("a").unwrap();
    store.gc(GcBudget { objects: 64 }).unwrap();
    assert!(store.contains_segment(&digest).unwrap());
    store.release(pin).unwrap();
    store.gc(GcBudget { objects: 64 }).unwrap();
    assert!(store.contains_segment(&digest).unwrap());
    store.publish(req("b", 2, segment("next-b"))).unwrap();
    store.drop_previous("b").unwrap();
    store.gc(GcBudget { objects: 64 }).unwrap();
    assert!(!store.contains_segment(&digest).unwrap());
}

fn retire_root_keeps_leased_bytes() {
    let store = open(MemoryFs::posix());
    let bytes = segment("leased");
    store.publish(req("a", 1, bytes.clone())).unwrap();
    let pin = store.pin("a").unwrap();
    let digest = *pin.segments()[0].digest();
    store.retire_root("a").unwrap();
    store.gc(GcBudget { objects: 64 }).unwrap();
    assert!(store.contains_segment(&digest).unwrap());
    assert_eq!(store.read_segment(&pin, &digest).unwrap(), bytes);
    store.release(pin).unwrap();
    store.gc(GcBudget { objects: 64 }).unwrap();
    assert!(!store.contains_segment(&digest).unwrap());
}

fn omit_leases_unlinks_a_pinned_segment() {
    let store = open(MemoryFs::posix());
    let bytes = segment("pinned");
    store.publish(req("main", 1, bytes)).unwrap();
    let pin = store.pin("main").unwrap();
    let digest = *pin.segments()[0].digest();
    store.publish(req("main", 2, segment("replacement"))).unwrap();
    store.drop_previous("main").unwrap();
    store.gc(GcBudget { objects: 64 }).unwrap();
    assert!(store.contains_segment(&digest).unwrap());
    store.gc_with_mark(GcBudget { objects: 64 }, MarkMode::OmitLeases).unwrap();
    assert!(!store.contains_segment(&digest).unwrap());
    store.release(pin).unwrap();
}

fn pin_held_across_gc_still_reads() {
    let store = Arc::new(open(MemoryFs::posix()));
    let held = segment("held");
    store.publish(req("main", 1, held.clone())).unwrap();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let (go_tx, go_rx) = std::sync::mpsc::channel::<()>();
    let worker = {
        let store = Arc::clone(&store);
        thread::spawn(move || {
            let pin = store.pin("main").unwrap();
            let digest = *pin.segments()[0].digest();
            ready_tx.send(digest).unwrap();
            go_rx.recv().unwrap();
            let bytes = store.read_segment(&pin, &digest).unwrap();
            store.release(pin).unwrap();
            bytes
        })
    };
    let digest = ready_rx.recv().unwrap();
    store.publish(req("main", 2, segment("later"))).unwrap();
    store.drop_previous("main").unwrap();
    store.gc(GcBudget { objects: 64 }).unwrap();
    let kept = store.contains_segment(&digest).unwrap();
    go_tx.send(()).unwrap();
    let bytes = worker.join().unwrap();
    assert!(kept);
    assert_eq!(bytes, held);
}
