// SPDX-License-Identifier: Apache-2.0
//! `DiskFull` is the filesystem refusal. Cache admission is a different error.
//! A full disk leaves the previous generation readable, and a cleared budget can publish.
//! Durable mode refuses a filesystem that cannot sync. Ephemeral mode does not survive a crash.

mod common;

use common::{open, req, segment};
use dctx_adapter_store::ports::StoreFs;
use dctx_adapter_store::{DurabilityMode, MemoryFs, SegmentRole, StoreError, StoreOptions};
use dctx_map_segment::decode;

#[test]
fn disk_full_keeps_the_committed_generation() {
    let fs = MemoryFs::posix();
    let store = open(fs.clone());
    let first = segment("kept");
    store.publish(req("main", 1, first.clone())).unwrap();
    fs.set_disk_budget(Some(0));
    let err = store.publish(req("main", 2, segment("blocked"))).unwrap_err();
    assert_eq!(err, StoreError::DiskFull);
    assert_ne!(err, StoreError::CacheLimit { requested: 0, available: 0 });
    let pin = store.pin("main").unwrap();
    assert_eq!(pin.generation(), 1);
    let bytes = store.read_segment(&pin, pin.segments()[0].digest()).unwrap();
    assert_eq!(bytes, first);
    decode(&bytes, SegmentRole::Fact).unwrap();
    fs.set_disk_budget(None);
    store.publish(req("main", 2, segment("retry"))).unwrap();
    assert_eq!(store.committed("main").unwrap(), Some(2));
}

#[test]
fn durable_mode_refuses_a_filesystem_without_sync() {
    let fs = MemoryFs::no_sync();
    let store = open(fs.clone());
    let err = store.publish(req("main", 1, segment("unsynced"))).unwrap_err();
    assert_eq!(err, StoreError::UnsupportedDurability { capability: "sync" });
    assert_eq!(fs.list_prefix("segments/").unwrap(), [] as [std::string::String; 0]);

    let fs = MemoryFs::posix();
    let store = open(fs.clone());
    store.publish(req("main", 1, segment("durable"))).unwrap();
    fs.crash();
    assert_eq!(store.committed("main").unwrap(), Some(1));
}

#[test]
fn ephemeral_publish_does_not_survive_a_crash() {
    let fs = MemoryFs::posix();
    let mut options = StoreOptions::capped(8 * 1024 * 1024);
    options.mode = DurabilityMode::Ephemeral;
    let store = dctx_adapter_store::SegmentStore::open(fs.clone(), options).unwrap();
    store.publish(req("main", 1, segment("volatile"))).unwrap();
    assert_eq!(store.committed("main").unwrap(), Some(1));
    fs.crash();
    assert_eq!(store.committed("main").unwrap(), None);
}
