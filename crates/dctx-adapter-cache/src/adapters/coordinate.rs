//! Store coordination and auto-clean that unlinks before dropping a charge.
//! Implements review F05 and F08: one observed revision, and pending bytes until unlink succeeds.
//! Depends on [`super::FsLedgerStore`] and [`super::FsObjectStore`]. The facade calls [`auto_clean`].
//! A stale store cannot replace a newer ledger. A failed unlink keeps the entry and the next slice retries.

use super::{FsLedgerStore, FsObjectStore};
use crate::domain::{Lifecycle, ObjectId};
use crate::error::CacheError;
use crate::ports::ObjectStore;
use crate::{AutoCleanPolicy, ClockSample, MaintainReport};

/// Runs one auto-clean slice. Unlink failures stay charged, are stored, and return an error.
/// # Errors
/// Propagates ledger lock/load/update, auto-clean policy/maintenance, persistence, or object-unlink failures.
pub fn auto_clean(
    ledger: &FsLedgerStore,
    objects: &FsObjectStore,
    life: &mut Lifecycle,
    clock: ClockSample,
    policy: AutoCleanPolicy,
) -> Result<MaintainReport, CacheError> {
    ledger.exclusive(|| auto_clean_locked(ledger, objects, life, clock, policy))
}

fn auto_clean_locked(
    ledger: &FsLedgerStore,
    objects: &FsObjectStore,
    life: &mut Lifecycle,
    clock: ClockSample,
    policy: AutoCleanPolicy,
) -> Result<MaintainReport, CacheError> {
    let original = life.clone();
    let mut preview = original.clone();
    let mut report = preview.auto_clean(clock, policy)?;
    if !report.ran {
        *life = preview;
        return Ok(report);
    }
    let removed: Vec<ObjectId> = original
        .object_ids()
        .into_iter()
        .filter(|id| preview.object_state(*id).is_none())
        .collect();
    let mut restored = 0u64;
    for id in removed {
        if objects.unlink_object(id).is_err() {
            let bytes = original.object_bytes(id).unwrap_or(0);
            preview.restore_pending_charge(&original, id)?;
            restored = restored.saturating_add(bytes);
        }
    }
    report.reclaimed_bytes = report.reclaimed_bytes.saturating_sub(restored);
    report.pending_bytes = preview.buckets().pending;
    *life = preview;
    ledger.store_locked(life)?;
    if restored > 0 {
        return Err(CacheError::LedgerIo { reason: "unlink" });
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::auto_clean;
    use crate::adapters::{FsLedgerStore, FsObjectStore};
    use crate::ports::LedgerStore;
    use crate::{
        AutoCleanPolicy, CacheError, FakeClock, GcRequest, Lifecycle, Physical, Quota, RootId,
        SliceBudget,
    };
    use std::fs;
    use std::path::{Path, PathBuf};

    fn root() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("temp");
        let root = dir.path().join("cache");
        fs::create_dir(&root).expect("dir");
        (dir, root)
    }

    fn payload(root: &Path) -> u64 {
        let objects = root.join("objects");
        let Ok(entries) = fs::read_dir(&objects) else {
            return 0;
        };
        let mut total = 0u64;
        for entry in entries.flatten() {
            let Ok(meta) = fs::symlink_metadata(entry.path()) else {
                continue;
            };
            if meta.is_file() && !meta.file_type().is_symlink() {
                total = total.saturating_add(meta.len());
            }
        }
        total
    }

    #[test]
    fn independent_cache_stores_coordinate() {
        let (_dir, root) = root();
        let quota = Quota::scaled(100);
        let first = FsLedgerStore::open(&root).expect("first");
        let second = FsLedgerStore::open(&root).expect("second");
        let objects = FsObjectStore::open(&root).expect("objects");
        let mut left = first.load(quota).expect("load").lifecycle;
        let mut right = second.load(quota).expect("load").lifecycle;
        let id = RootId::new(1);
        left.register_root(id).expect("root");
        right.register_root(id).expect("root");
        let left_bytes = vec![b'A'; 60];
        let right_bytes = vec![b'B'; 60];
        let left_id = left.insert_segment(60, 0, &[id]).expect("left");
        let right_id = right.insert_segment(60, 0, &[id]).expect("right");
        assert_eq!(left_id, right_id);
        objects.put_object(left_id.raw(), &left_bytes).expect("put");
        let path = root.join("objects").join(left_id.raw().to_string());
        let overwritten = objects.put_object(right_id.raw(), &right_bytes);
        assert!(matches!(
            overwritten,
            Err(CacheError::InvalidRequest { reason: "object already published" })
        ));
        assert_eq!(fs::read(&path).expect("kept"), left_bytes);
        first.store(&left).expect("store");
        let stale = second.store(&right);
        assert!(matches!(stale, Err(CacheError::InvalidRequest { reason: "stale ledger" })));
        assert_eq!(fs::read(&path).expect("still"), left_bytes);

        let mut reloaded = second.load(quota).expect("reload").lifecycle;
        let refused = reloaded.insert_segment(60, 0, &[id]);
        assert!(matches!(refused, Err(CacheError::CacheLimit { .. })));
        assert_eq!(reloaded.admitted(), 60);
        let small_id = reloaded.insert_segment(30, 0, &[id]).expect("fits");
        let small = vec![b'C'; 30];
        objects.put_object(small_id.raw(), &small).expect("small");
        second.store(&reloaded).expect("commit");
        assert_eq!(fs::read(&path).expect("first stays"), left_bytes);
        assert_eq!(fs::read(root.join("objects").join(small_id.raw().to_string())).expect("small"), small);
        let stored = first.load(quota).expect("final").lifecycle;
        assert_eq!(stored.admitted(), 90);
        assert_eq!(payload(&root), stored.admitted());
    }

    #[test]
    fn pending_bytes_match_the_files_until_unlink_succeeds() {
        let (_dir, root) = root();
        let quota = Quota::scaled(100);
        let store = FsLedgerStore::open(&root).expect("ledger");
        let objects = FsObjectStore::open(&root).expect("objects");
        let mut life = Lifecycle::new(quota).expect("quota");
        let id = RootId::new(1);
        life.register_root(id).expect("root");
        let pending = life.insert_segment(10, 0, &[id]).expect("segment");
        let bytes = b"0123456789";
        objects.put_object(pending.raw(), bytes).expect("put");
        let path = root.join("objects").join(pending.raw().to_string());
        life.gc(GcRequest {
            root: None,
            budget: SliceBudget::explicit(16),
            bypass_grace: true,
            max_bytes: None,
            cancel: None,
        })
        .expect("pending");
        assert_eq!(life.object_state(pending), Some(Physical::PendingUnlink));
        let refused = life.insert_segment(95, 0, &[id]);
        assert!(matches!(refused, Err(CacheError::CacheLimit { .. })));
        assert_eq!(fs::read(&path).expect("file"), bytes);
        assert_eq!(life.object_bytes(pending), Some(10));
        assert_eq!(payload(&root), 10);

        let mut policy = AutoCleanPolicy::product_default();
        policy.min_interval_ms = 0;
        let clock = FakeClock::at(1_000).sample();
        auto_clean(&store, &objects, &mut life, clock, policy).expect("unlink");
        assert!(life.object_bytes(pending).is_none());
        assert!(!path.exists());
        assert_eq!(life.admitted(), 0);
        assert_eq!(payload(&root), 0);
        let again = life.insert_segment(95, 0, &[id]).expect("room");
        let wide = vec![b'D'; 95];
        objects.put_object(again.raw(), &wide).expect("wide");
        assert_eq!(life.admitted(), 95);
        assert_eq!(payload(&root), life.admitted());

        let blocked = life.insert_segment(4, 0, &[id]).expect("blocked");
        objects.put_object(blocked.raw(), b"wxyz").expect("put");
        let blocked_path = root.join("objects").join(blocked.raw().to_string());
        life.gc(GcRequest {
            root: None,
            budget: SliceBudget::explicit(16),
            bypass_grace: true,
            max_bytes: None,
            cancel: None,
        })
        .expect("mark");
        assert_eq!(life.object_state(blocked), Some(Physical::PendingUnlink));
        fs::remove_file(&blocked_path).expect("remove");
        fs::create_dir(&blocked_path).expect("dir");
        let error = auto_clean(&store, &objects, &mut life, clock, policy).expect_err("unlink");
        assert!(matches!(error, CacheError::LedgerIo { reason: "unlink" }));
        assert_eq!(life.object_state(blocked), Some(Physical::PendingUnlink));
        assert_eq!(life.object_bytes(blocked), Some(4));
        assert!(blocked_path.is_dir());
        let loaded = store.load(quota).expect("reload").lifecycle;
        assert_eq!(loaded.object_bytes(blocked), Some(4));
        fs::remove_dir(&blocked_path).expect("rmdir");
        let mut life = loaded;
        auto_clean(&store, &objects, &mut life, clock, policy).expect("retry");
        assert!(life.object_bytes(blocked).is_none());
        assert!(!blocked_path.exists());
        let loaded = store.load(quota).expect("clean").lifecycle;
        assert!(loaded.object_bytes(blocked).is_none());
        assert_eq!(payload(&root), loaded.admitted());
    }
}
