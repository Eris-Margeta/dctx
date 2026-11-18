// SPDX-License-Identifier: Apache-2.0

use dctx_adapter_cache::adapters::{FsLedgerStore, FsObjectStore, LEDGER_FILE, LedgerUpdateError};
use dctx_adapter_cache::ports::LedgerStore;
use dctx_adapter_cache::{CacheError, LEDGER_MAX_BYTES, LEDGER_VERSION, Lifecycle, Quota};
use std::{fs, io::Read, sync::Barrier};

fn detect_and_rebuild(
    damage: impl FnOnce(&mut Vec<u8>),
    matches_fault: impl Fn(&CacheError) -> bool,
) {
    let dir = tempfile::tempdir().unwrap();
    let quota = Quota::scaled(4096);
    let writer = FsLedgerStore::open(dir.path()).unwrap();
    let objects = FsObjectStore::open(dir.path()).unwrap();
    let mut original = Lifecycle::new(quota).unwrap();
    let old = original.load_staged(17, 0).unwrap();
    objects.put_object(old.raw(), b"stale cache bytes").unwrap();
    writer.store(&original).unwrap();

    let path = dir.path().join(LEDGER_FILE);
    let mut damaged = fs::read(&path).unwrap();
    damage(&mut damaged);
    fs::write(&path, damaged).unwrap();

    let repair = FsLedgerStore::open(dir.path()).unwrap();
    let result = repair.read(quota, |life| Ok::<_, CacheError>(life.object_bytes(old)));
    assert!(matches!(
        result,
        Err(LedgerUpdateError::Cache(ref error)) if matches_fault(error)
    ));
    let loaded = repair.load(quota).unwrap();
    assert!(loaded.fault.as_ref().is_some_and(matches_fault));
    assert_eq!(loaded.lifecycle.admitted(), 0);
    assert_eq!(loaded.lifecycle.object_bytes(old), None);
    assert!(
        loaded
            .orphans
            .iter()
            .any(|file| { file.relative == format!("objects/{}", old.raw()) })
    );

    let mut rebuilt = loaded.lifecycle;
    let new = rebuilt.load_staged(4, 0).unwrap();
    for orphan in loaded.orphans {
        objects
            .unlink_relative(std::path::Path::new(&orphan.relative))
            .unwrap();
    }
    objects.put_object(new.raw(), b"new!").unwrap();
    repair.store(&rebuilt).unwrap();

    let reopened = FsLedgerStore::open(dir.path()).unwrap();
    let mut bytes = Vec::new();
    reopened
        .read(quota, |life| {
            assert_eq!(life.admitted(), 4);
            assert_eq!(life.object_bytes(new), Some(4));
            objects
                .open_object(new.raw())
                .unwrap()
                .unwrap()
                .read_to_end(&mut bytes)
        })
        .unwrap();
    assert_eq!(bytes, b"new!");
}

#[test]
fn truncated_ledger_is_detected_and_rebuilt() {
    detect_and_rebuild(
        |bytes| bytes.truncate(bytes.len() - 1),
        |error| {
            matches!(
                error,
                CacheError::CorruptLedger {
                    reason: "ledger length"
                }
            )
        },
    );
}

#[test]
fn bit_flipped_ledger_is_detected_and_rebuilt() {
    detect_and_rebuild(
        |bytes| bytes[16] ^= 1,
        |error| matches!(error, CacheError::CorruptLedger { reason: "checksum" }),
    );
}

#[test]
fn wrong_version_ledger_is_detected_and_rebuilt() {
    let version = LEDGER_VERSION + 1;
    detect_and_rebuild(
        |bytes| bytes[8..12].copy_from_slice(&version.to_le_bytes()),
        |error| matches!(error, CacheError::UnknownLedgerVersion { found } if *found == version),
    );
}

#[test]
fn sparse_ledger_over_the_cap_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    fs::File::create(dir.path().join(LEDGER_FILE))
        .unwrap()
        .set_len(u64::try_from(LEDGER_MAX_BYTES).unwrap() + 1)
        .unwrap();
    let ledger = FsLedgerStore::open(dir.path()).unwrap();
    let loaded = ledger.load(Quota::scaled(4096)).unwrap();
    assert!(matches!(
        loaded.fault,
        Some(CacheError::CorruptLedger {
            reason: "ledger is too large"
        })
    ));
    assert!(loaded.lifecycle.object_ids().is_empty());
}

#[test]
fn concurrent_updates_to_one_directory_keep_every_committed_value() {
    const WRITERS: usize = 8;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let quota = Quota::scaled(4096);
    let start = Barrier::new(WRITERS);
    let mut written = std::thread::scope(|scope| {
        let threads: Vec<_> = (1..=WRITERS)
            .map(|index| {
                let start = &start;
                scope.spawn(move || {
                    let ledger = FsLedgerStore::open(root).unwrap();
                    let objects = FsObjectStore::open(root).unwrap();
                    let size = u64::try_from(index).unwrap();
                    start.wait();
                    ledger
                        .update(quota, |life| {
                            let id = life.load_staged(size, 0)?;
                            objects.put_object(id.raw(), &size.to_le_bytes()[..index])?;
                            Ok::<_, CacheError>(((id, size), true))
                        })
                        .unwrap()
                })
            })
            .collect();
        threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect::<Vec<_>>()
    });
    written.sort_unstable();
    assert!(written.windows(2).all(|pair| pair[0].0 != pair[1].0));
    let ledger = FsLedgerStore::open(root).unwrap();
    let objects = FsObjectStore::open(root).unwrap();
    ledger
        .read(quota, |life| {
            assert_eq!(life.object_ids().len(), WRITERS);
            for (id, size) in written {
                assert_eq!(life.object_bytes(id), Some(size));
                let mut bytes = Vec::new();
                objects
                    .open_object(id.raw())
                    .unwrap()
                    .unwrap()
                    .read_to_end(&mut bytes)
                    .unwrap();
                assert_eq!(bytes, size.to_le_bytes()[..usize::try_from(size).unwrap()]);
            }
            Ok::<_, CacheError>(())
        })
        .unwrap();
}

#[test]
fn reloading_after_a_rival_commit_cannot_refresh_an_older_stage() {
    let dir = tempfile::tempdir().unwrap();
    let quota = Quota::scaled(4096);
    let first = FsLedgerStore::open(dir.path()).unwrap();
    let rival = FsLedgerStore::open(dir.path()).unwrap();
    first.store(&Lifecycle::new(quota).unwrap()).unwrap();

    let mut older = first.load(quota).unwrap().lifecycle;
    older.load_staged(17, 0).unwrap();
    first.stage(&older).unwrap();
    rival
        .update(quota, |life| life.load_staged(23, 0).map(|_| ((), true)))
        .unwrap();
    assert_eq!(first.load(quota).unwrap().lifecycle.admitted(), 23);
    assert!(matches!(
        first.commit(),
        Err(CacheError::InvalidRequest {
            reason: "stale ledger"
        })
    ));
    assert_eq!(rival.load(quota).unwrap().lifecycle.admitted(), 23);
}

#[test]
fn different_images_with_the_same_revision_do_not_bypass_conflict_detection() {
    let dir = tempfile::tempdir().unwrap();
    let quota = Quota::scaled(4096);
    let writer = FsLedgerStore::open(dir.path()).unwrap();
    let rival = FsLedgerStore::open(dir.path()).unwrap();
    let original = Lifecycle::new(quota).unwrap();
    let replacement = Lifecycle::new(Quota::scaled(2048)).unwrap();
    assert_eq!(original.logical_revision(), replacement.logical_revision());

    writer.store(&original).unwrap();
    let mut stale = writer.load(quota).unwrap().lifecycle;
    stale.load_staged(17, 0).unwrap();
    rival.load(quota).unwrap();
    rival.store(&replacement).unwrap();

    assert!(matches!(
        writer.store(&stale),
        Err(CacheError::InvalidRequest {
            reason: "stale ledger"
        })
    ));
    let active = writer.load(quota).unwrap().lifecycle;
    assert_eq!(active.status().hard_cap, 2048);
    assert_eq!(active.admitted(), 0);
}

#[test]
fn a_replaced_stage_is_refused_even_if_it_decodes() {
    let dir = tempfile::tempdir().unwrap();
    let quota = Quota::scaled(4096);
    let ledger = FsLedgerStore::open(dir.path()).unwrap();
    let original = Lifecycle::new(quota).unwrap();
    ledger.store(&original).unwrap();
    let mut edited = original.clone();
    edited.load_staged(17, 0).unwrap();
    ledger.stage(&edited).unwrap();
    let loaded = ledger.load(quota).unwrap();
    let [stage] = loaded.orphans.as_slice() else {
        panic!("expected exactly one staged ledger");
    };
    let mut wrong = original;
    wrong.load_staged(23, 0).unwrap();
    fs::write(
        dir.path().join(&stage.relative),
        wrong.encode_ledger().unwrap(),
    )
    .unwrap();

    assert!(matches!(
        ledger.commit(),
        Err(CacheError::CorruptLedger {
            reason: "staged ledger changed"
        })
    ));
    assert_eq!(ledger.load(quota).unwrap().lifecycle.admitted(), 0);
}

#[cfg(unix)]
#[test]
fn a_cache_root_replaced_by_a_link_cannot_read_or_write_its_target() {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("cache");
    let outside = dir.path().join("outside");
    fs::create_dir(&root).unwrap();
    fs::create_dir(&outside).unwrap();
    fs::create_dir(outside.join("objects")).unwrap();
    fs::write(outside.join("objects/7"), b"outside").unwrap();
    let ledger = FsLedgerStore::open(&root).unwrap();
    let objects = FsObjectStore::open(&root).unwrap();
    fs::rename(&root, dir.path().join("moved")).unwrap();
    symlink(&outside, &root).unwrap();

    assert!(matches!(
        ledger.load(Quota::scaled(4096)),
        Err(CacheError::OutsideCacheRoot)
    ));
    assert!(matches!(
        objects.open_object(7),
        Err(CacheError::OutsideCacheRoot)
    ));
    assert!(matches!(
        objects.put_object(8, b"write"),
        Err(CacheError::OutsideCacheRoot)
    ));
    assert!(!outside.join("ledger.lock").exists());
    assert!(!outside.join("objects/8").exists());
}
