//! P8: a crash, a cancel, and a short GC budget still finish or refuse cleanly.
//! The maintenance cursor resumes on a new store. Live bytes stay readable.
//! `CacheLimit` is a different refusal from `DiskFull`. A stuck lock is not cleared by crash or recovery.

mod common;

use common::{open, req, segment};
use dctx_adapter_store::ports::StoreFs;
use dctx_adapter_store::{Boundary, GcBudget, MemoryFs, SegmentRole, StoreError, StoreOptions};
use dctx_map_segment::decode;

#[test]
fn p8_recovery_progress() {
    let fs = MemoryFs::posix();
    let store = open(fs.clone());
    fs.arm_crash(Some(Boundary::AfterSegmentWrite));
    let err = store.publish(req("main", 1, segment("live"))).unwrap_err();
    assert!(matches!(err, StoreError::Crashed { .. }));
    assert!(!fs.lock_held("main"));
    let store = open(fs.clone());
    assert_eq!(store.recover().unwrap().generation, None);
    let live = segment("live");
    store.publish(req("main", 1, live.clone())).unwrap();
    assert_eq!(store.committed("main").unwrap(), Some(1));

    store.cancel_at(Boundary::AfterReserve);
    assert_eq!(
        store.publish(req("main", 2, segment("cancel-a"))).unwrap_err(),
        StoreError::Cancelled
    );
    store.request_cancel();
    assert_eq!(
        store.publish(req("main", 2, segment("cancel-b"))).unwrap_err(),
        StoreError::Cancelled
    );
    assert_eq!(store.committed("main").unwrap(), Some(1));
    assert!(!fs.lock_held("main"));

    for index in 0..3 {
        let store = open(fs.clone());
        fs.arm_crash(Some(Boundary::AfterSegmentDirSync));
        let err = store.publish(req("main", 2, segment(&format!("orphan-{index}")))).unwrap_err();
        assert_eq!(err, StoreError::Crashed { boundary: "after-segment-dir-sync" });
    }
    assert_eq!(segment_count(&fs), 4);
    let store = open(fs.clone());
    let report = store.gc(GcBudget { objects: 1 }).unwrap();
    assert_eq!(report.unlinked, 1);
    assert!(!report.done);
    assert_eq!(segment_count(&fs), 3);
    assert!(fs.exists("maintenance/cursor").unwrap());
    store.request_cancel();
    assert_eq!(store.gc(GcBudget { objects: 32 }).unwrap_err(), StoreError::Cancelled);
    assert_eq!(segment_count(&fs), 3);
    assert!(fs.exists("maintenance/cursor").unwrap());
    let store = open(fs.clone());
    let report = store.gc(GcBudget { objects: 32 }).unwrap();
    assert!(report.done, "{report:?}");
    assert_eq!(segment_count(&fs), 1);
    assert!(!fs.exists("maintenance/cursor").unwrap());
    let pin = store.pin("main").unwrap();
    assert_eq!(pin.generation(), 1);
    let bytes = store.read_segment(&pin, pin.segments()[0].digest()).unwrap();
    assert_eq!(bytes, live);
    decode(&bytes, SegmentRole::Fact).unwrap();

    let tiny = SegmentStoreCap::open_tiny(fs.clone());
    let err = tiny.publish(req("main", 2, segment("too-big"))).unwrap_err();
    assert!(matches!(err, StoreError::CacheLimit { .. }), "{err}");
    assert_ne!(err, StoreError::DiskFull);
    assert_eq!(store.committed("main").unwrap(), Some(1));

    let fs = MemoryFs::posix();
    fs.stick_lock("main");
    let store = open(fs.clone());
    assert_eq!(store.publish(req("main", 1, segment("stuck"))).unwrap_err(), StoreError::Busy);
    fs.crash();
    assert!(fs.lock_held("main"));
    assert_eq!(store.recover().unwrap_err(), StoreError::Busy);
    assert!(fs.lock_held("main"));
    assert_eq!(store.publish(req("main", 1, segment("again"))).unwrap_err(), StoreError::Busy);

    let fs = MemoryFs::posix();
    let store = open(fs.clone());
    fs.arm_crash(Some(Boundary::AfterLock));
    assert!(matches!(
        store.publish(req("main", 1, segment("freed"))).unwrap_err(),
        StoreError::Crashed { .. }
    ));
    assert!(!fs.lock_held("main"));
    let store = open(fs);
    store.recover().unwrap();
    store.publish(req("main", 1, segment("freed"))).unwrap();
    assert_eq!(store.committed("main").unwrap(), Some(1));
}

fn segment_count(fs: &MemoryFs) -> usize {
    fs.list_prefix("segments/").unwrap().len()
}

struct SegmentStoreCap;

impl SegmentStoreCap {
    fn open_tiny(fs: MemoryFs) -> dctx_adapter_store::SegmentStore<MemoryFs> {
        dctx_adapter_store::SegmentStore::open(fs, StoreOptions::capped(1)).unwrap()
    }
}
