//! Windows sharing: an open handle blocks replace and unlink without truncating bytes.
//! The refused name stays pending and is collected after the handle closes.
//! POSIX replace keeps the old inode readable through the open handle.

mod common;

use common::{digest_of, open, req, segment, segment_rel};
use dctx_adapter_store::ports::StoreFs;
use dctx_adapter_store::{GcBudget, MemoryFs, StoreError};

#[test]
fn windows_open_segment_is_pending_until_the_handle_closes() {
    let fs = MemoryFs::windows();
    let store = open(fs.clone());
    let first = segment("held");
    let path = segment_rel(&first);
    let digest = digest_of(&first);
    store.publish(req("main", 1, first.clone())).unwrap();
    store.publish(req("main", 2, segment("next"))).unwrap();
    store.drop_previous("main").unwrap();
    let token = fs.hold(&path).unwrap();
    let report = store.gc(GcBudget { objects: 8 }).unwrap();
    assert!(report.pending > 0, "{report:?}");
    assert!(store.contains_segment(&digest).unwrap());
    assert_eq!(fs.read(&path).unwrap(), first);
    fs.release_hold(token).unwrap();
    store.gc(GcBudget { objects: 8 }).unwrap();
    assert!(!store.contains_segment(&digest).unwrap());
}

#[test]
fn windows_open_pointer_refuses_replace_and_posix_keeps_the_old_inode() {
    let fs = MemoryFs::windows();
    let store = open(fs.clone());
    store.publish(req("main", 1, segment("one"))).unwrap();
    store.publish(req("main", 2, segment("two"))).unwrap();
    let token = fs.hold("roots/main/current").unwrap();
    let err = store.publish(req("main", 3, segment("three"))).unwrap_err();
    assert_eq!(err, StoreError::SharingViolation);
    assert_eq!(store.raw_current_generation("main").unwrap(), Some(2));
    fs.release_hold(token).unwrap();
    store.publish(req("main", 3, segment("three"))).unwrap();
    assert_eq!(store.committed("main").unwrap(), Some(3));

    let fs = MemoryFs::posix();
    let store = open(fs.clone());
    store.publish(req("main", 1, segment("old"))).unwrap();
    let old = fs.read("roots/main/current").unwrap();
    let token = fs.hold("roots/main/current").unwrap();
    store.publish(req("main", 2, segment("new"))).unwrap();
    assert_eq!(fs.read_hold(token).unwrap(), old);
    assert_ne!(fs.read("roots/main/current").unwrap(), old);
    assert_eq!(store.committed("main").unwrap(), Some(2));
}
