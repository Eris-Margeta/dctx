// SPDX-License-Identifier: Apache-2.0
//! P2: a crash at every publication cut leaves one valid generation.
//! Concurrent readers may observe the old or new closure, with matching bytes.
//! The pointer-first mutation is not recovered as a new generation.
//! Identical segment bytes are reused. A different payload at that name is corrupt.

mod common;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use common::{digest_of, open, req, segment, segment_rel};
use dctx_adapter_store::ports::StoreFs;
use dctx_adapter_store::{Boundary, MemoryFs, SegmentRole, StoreError};
use dctx_map_segment::decode;

#[test]
fn p2_publish_crash_matrix() {
    for boundary in Boundary::ALL {
        let fs = MemoryFs::posix();
        let writer = open(fs.clone());
        let first = segment("base");
        writer.publish(req("main", 1, first.clone())).unwrap();
        let second = segment(boundary.as_str());
        let stop = Arc::new(AtomicBool::new(false));
        let errors = Arc::new(Mutex::new(Vec::new()));
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let reader = {
            let fs = fs.clone();
            let stop = Arc::clone(&stop);
            let errors = Arc::clone(&errors);
            let first = first.clone();
            let second = second.clone();
            thread::spawn(move || {
                let store = open(fs);
                ready_tx.send(()).unwrap();
                while !stop.load(Ordering::SeqCst) {
                    observe(&store, &first, &second, &errors);
                    thread::yield_now();
                }
            })
        };
        ready_rx.recv().unwrap();
        fs.arm_crash(Some(boundary));
        let err = writer.publish(req("main", 2, second.clone())).unwrap_err();
        stop.store(true, Ordering::SeqCst);
        reader.join().unwrap();
        assert_eq!(err, StoreError::Crashed { boundary: boundary.as_str() });
        assert!(errors.lock().unwrap().is_empty(), "{boundary:?} {:?}", errors.lock().unwrap());

        let store = open(fs);
        let recovery = store.recover().unwrap();
        let expect = if boundary.publishes_new_generation() { 2 } else { 1 };
        assert_eq!(recovery.generation, Some(expect), "{boundary:?}");
        assert!(!recovery.repaired_pointer);
        assert!(store.current_pointer_closed("main").unwrap());
        assert_eq!(store.raw_current_generation("main").unwrap(), Some(expect));
        assert_eq!(store.committed("main").unwrap(), Some(expect));
        let pin = store.pin("main").unwrap();
        assert_eq!(pin.generation(), expect);
        let expected = if expect == 1 { &first } else { &second };
        let digest = *pin.segments()[0].digest();
        let bytes = store.read_segment(&pin, &digest).unwrap();
        assert_eq!(bytes, *expected);
        decode(&bytes, SegmentRole::Fact).unwrap();
    }
}

fn observe(
    store: &dctx_adapter_store::SegmentStore<MemoryFs>,
    first: &[u8],
    second: &[u8],
    errors: &Mutex<Vec<String>>,
) {
    let pin = match store.pin("main") {
        Ok(pin) => pin,
        Err(StoreError::Crashed { .. } | StoreError::Busy | StoreError::InvalidRequest { .. }) => return,
        Err(err) => {
            errors.lock().unwrap().push(err.to_string());
            return;
        }
    };
    let expected = match pin.generation() {
        1 => first,
        2 => second,
        other => {
            errors.lock().unwrap().push(format!("generation {other}"));
            let _ = store.release(pin);
            return;
        }
    };
    let Some(digest) = pin.segments().first().map(|segment| *segment.digest()) else {
        errors.lock().unwrap().push("empty pin".to_string());
        let _ = store.release(pin);
        return;
    };
    match store.read_segment(&pin, &digest) {
        Ok(bytes) if bytes == expected => {}
        Ok(_) => errors.lock().unwrap().push(format!("bytes {}", pin.generation())),
        Err(StoreError::Crashed { .. } | StoreError::Busy | StoreError::InvalidRequest { .. }) => {}
        Err(err) => errors.lock().unwrap().push(err.to_string()),
    }
    if let Err(err) = store.release(pin)
        && !matches!(err, StoreError::Crashed { .. } | StoreError::Busy | StoreError::InvalidRequest { .. }) {
            errors.lock().unwrap().push(err.to_string());
        }
}

#[test]
fn pointer_first_mutation_is_not_a_committed_generation() {
    let fs = MemoryFs::posix();
    let store = open(fs.clone());
    let first = segment("kept");
    store.publish(req("main", 1, first.clone())).unwrap();
    store.publish_unvalidated_pointer("main", 2).unwrap();
    assert_eq!(store.raw_current_generation("main").unwrap(), Some(2));
    assert!(!store.current_pointer_closed("main").unwrap());
    let pin = store.pin("main").unwrap();
    assert_eq!(pin.generation(), 1);
    assert_eq!(store.read_segment(&pin, pin.segments()[0].digest()).unwrap(), first);
    drop(pin);

    let store = open(fs);
    let recovery = store.recover().unwrap();
    assert!(recovery.repaired_pointer);
    assert_eq!(recovery.generation, Some(1));
    assert!(store.current_pointer_closed("main").unwrap());
    assert_eq!(store.raw_current_generation("main").unwrap(), Some(1));
    assert_eq!(store.committed("main").unwrap(), Some(1));
    store.publish(req("main", 2, segment("repaired"))).unwrap();
    assert_eq!(store.committed("main").unwrap(), Some(2));
    assert!(store.current_pointer_closed("main").unwrap());
}

#[test]
fn identical_segment_bytes_are_reused_and_a_collision_is_corrupt() {
    let fs = MemoryFs::posix();
    let store = open(fs.clone());
    let same = segment("same");
    store.publish(req("main", 1, same.clone())).unwrap();
    let published = store.publish(req("main", 2, same)).unwrap();
    assert_eq!(published.segments, vec![digest_of(&segment("same"))]);
    assert_eq!(fs.list_prefix("segments/").unwrap().len(), 1);

    let other = segment("other");
    let path = segment_rel(&other);
    fs.create(&path).unwrap();
    fs.write(&path, b"different-bytes").unwrap();
    fs.sync_file(&path).unwrap();
    fs.sync_dir().unwrap();
    let err = store.publish(req("main", 3, other)).unwrap_err();
    assert_eq!(err, StoreError::CorruptIndex { reason: "segment" });
    assert_eq!(store.committed("main").unwrap(), Some(2));
    let pin = store.pin("main").unwrap();
    assert_eq!(pin.generation(), 2);
    decode(&store.read_segment(&pin, pin.segments()[0].digest()).unwrap(), SegmentRole::Fact).unwrap();
}
