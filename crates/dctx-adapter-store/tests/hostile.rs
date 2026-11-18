// SPDX-License-Identifier: Apache-2.0

#![cfg(any(unix, windows))]

mod common;

use common::{digest_of, req, segment, segment_rel};
use dctx_adapter_store::ports::StoreFs;
use dctx_adapter_store::{SegmentStore, StdFs, StoreError, StoreOptions};
use std::fs;

fn open(dir: &tempfile::TempDir) -> SegmentStore<StdFs> {
    SegmentStore::open(
        StdFs::open(dir.path()).unwrap(),
        StoreOptions::capped(8 * 1024 * 1024),
    )
    .unwrap()
}

#[test]
fn valid_segment_with_wrong_hash_is_refused_even_under_an_existing_pin() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(&dir);
    let expected = segment("aaaa");
    let replacement = segment("bbbb");
    assert_eq!(expected.len(), replacement.len());
    assert_ne!(digest_of(&expected), digest_of(&replacement));
    store.publish(req("main", 1, expected.clone())).unwrap();
    let pin = store.pin("main").unwrap();
    let digest = digest_of(&expected);
    assert_eq!(store.read_segment(&pin, &digest).unwrap(), expected);

    fs::write(dir.path().join(segment_rel(&expected)), replacement).unwrap();
    assert!(matches!(
        store.read_segment(&pin, &digest),
        Err(StoreError::CorruptIndex { reason: "segment" })
    ));
    assert!(!store.current_pointer_closed("main").unwrap());
    assert!(matches!(
        store.pin("main"),
        Err(StoreError::CorruptIndex { reason: "pointer" })
    ));
    store.release(pin).unwrap();
}

#[test]
fn valid_manifest_with_wrong_hash_cannot_close_a_pointer() {
    let dir = tempfile::tempdir().unwrap();
    let other_dir = tempfile::tempdir().unwrap();
    let store = open(&dir);
    let other = open(&other_dir);
    let content = segment("same");
    let first = store.publish(req("main", 1, content.clone())).unwrap();
    let mut changed = req("main", 1, content);
    changed.snapshot = [9; 32];
    let second = other.publish(changed).unwrap();
    assert_ne!(first.manifest, second.manifest);
    assert!(store.current_pointer_closed("main").unwrap());
    assert!(other.current_pointer_closed("main").unwrap());

    let manifest = fs::read_dir(dir.path().join("manifests"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let wrong_manifest = fs::read_dir(other_dir.path().join("manifests"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert_eq!(
        fs::metadata(&manifest).unwrap().len(),
        fs::metadata(&wrong_manifest).unwrap().len()
    );
    fs::write(manifest, fs::read(wrong_manifest).unwrap()).unwrap();

    assert!(!store.current_pointer_closed("main").unwrap());
    assert!(matches!(
        store.committed("main"),
        Err(StoreError::CorruptIndex { reason: "pointer" })
    ));
}

#[test]
fn oversized_pointer_is_refused_at_the_filesystem_and_store_boundaries() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(&dir);
    store.publish(req("main", 1, segment("first"))).unwrap();
    let path = "roots/main/current";
    fs::File::options()
        .write(true)
        .open(dir.path().join(path))
        .unwrap()
        .set_len(1024 * 1024 * 1024)
        .unwrap();
    let filesystem = StdFs::open(dir.path()).unwrap();
    assert_eq!(
        filesystem.read(path).unwrap_err(),
        StoreError::CorruptIndex { reason: "limit" }
    );
    assert_eq!(
        store.raw_current_generation("main").unwrap_err(),
        StoreError::CorruptIndex { reason: "limit" }
    );
}

#[test]
fn traversal_cannot_read_a_sibling_file() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("store");
    let store = StdFs::open(&root).unwrap();
    fs::write(dir.path().join("outside"), b"private").unwrap();

    for path in ["../outside", "missing/../../outside", r"..\outside"] {
        assert_eq!(
            store.read(path).unwrap_err(),
            StoreError::InvalidRequest { reason: "path" }
        );
    }
}

#[cfg(unix)]
#[test]
fn linked_ancestors_and_link_loops_cannot_be_opened() {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("store");
    let store = StdFs::open(&root).unwrap();
    fs::write(dir.path().join("outside"), b"private").unwrap();
    symlink(dir.path(), root.join("escape")).unwrap();
    symlink("loop", root.join("loop")).unwrap();

    for path in ["escape/outside", "loop"] {
        assert_eq!(
            store.read(path).unwrap_err(),
            StoreError::InvalidRequest { reason: "path" }
        );
    }
}
