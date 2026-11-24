// SPDX-License-Identifier: Apache-2.0
//! P-C10, P-C11 and P-C12: reclaim on publish, evict before refusing, shared read lock.
//! A superseded view is unlinked at publish unless a reader lease holds it; admission evicts reclaimable
//! generations oldest first across roots; readers share the lock and never see a reclaimed generation.
//! Domain tests use the in-memory ledger; the rest go through the filesystem ledger in a temp directory.

use dctx_adapter_cache::adapters::{FsLedgerStore, FsObjectStore, LeaseHold, LedgerUpdateError};
use dctx_adapter_cache::ports::{LedgerStore, LoadedLedger, ObjectStore};
use dctx_adapter_cache::{
    CacheError, GenerationId, LeaseId, Lifecycle, ObjectId, Physical, Quota, RootId, SessionId,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

const MANIFEST: u64 = 8;

fn cache_dir() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("temp");
    let root = dir.path().join("cache");
    fs::create_dir(&root).expect("dir");
    (dir, root)
}

fn state_of(life: &Lifecycle, root: RootId, generation: GenerationId) -> Option<Physical> {
    life.manifest_of(root, generation)
        .and_then(|id| life.manifest_state(id))
}

/// One view generation with a segment of `bytes`, as `admit_snapshot` builds it.
fn view(
    life: &mut Lifecycle,
    root: RootId,
    bytes: u64,
) -> Result<(ObjectId, GenerationId), CacheError> {
    let segment = life.insert_segment(bytes, 0, &[root])?;
    let generation = life.publish_view(root, MANIFEST, &[segment])?;
    Ok((segment, generation))
}

#[test]
fn reclaim_five_publishes_leave_at_most_two_generations() {
    let mut life = Lifecycle::new(Quota::scaled(100_000)).expect("quota");
    let root = RootId::new(1);
    life.register_root(root).expect("root");
    let mut made = Vec::new();
    for _ in 0..5 {
        made.push(view(&mut life, root, 100).expect("publish"));
    }
    let retained = made
        .iter()
        .filter(|(_, generation)| state_of(&life, root, *generation) == Some(Physical::Published))
        .count();
    assert!(retained <= 2, "{retained} generations are still retained");
    assert_eq!(retained, 1);
    let status = life.status();
    assert!(status.reclaimable_bytes > 0);
    assert_eq!(status.reclaimable_bytes, 4 * (100 + MANIFEST));
    assert_eq!(life.reclaimable_ids().len(), 8);
    assert!(life.p3_holds() && life.p4_holds());
}

#[test]
fn reclaim_waits_for_the_lease_and_follows_its_release() {
    let mut life = Lifecycle::new(Quota::scaled(100_000)).expect("quota");
    let root = RootId::new(1);
    life.register_root(root).expect("root");
    let (held, first) = view(&mut life, root, 100).expect("first");
    let lease = life.reader_begin(root, SessionId::new(7)).expect("begin");
    assert_eq!(life.reader_confirm(lease).expect("confirm"), first);
    let (_, second) = view(&mut life, root, 100).expect("second");
    let (_, third) = view(&mut life, root, 100).expect("third");
    // The leased generation survives two publishes; the unleased middle one is already reclaimable.
    assert_eq!(state_of(&life, root, first), Some(Physical::Published));
    assert_eq!(state_of(&life, root, second), Some(Physical::Retired));
    assert_eq!(state_of(&life, root, third), Some(Physical::Published));
    assert!(!life.reclaimable_ids().contains(&held));
    life.begin_reclaim();
    assert_eq!(life.object_state(held), Some(Physical::Published));
    assert!(life.p3_holds());
    life.reader_release(lease).expect("release");
    assert_eq!(state_of(&life, root, first), Some(Physical::Retired));
    assert!(life.reclaimable_ids().contains(&held));
    let removed = life.finish_reclaim();
    assert!(life.begin_reclaim() > 0 || !removed.is_empty());
    assert!(life.p3_holds() && life.p4_holds());
}

fn write_view(
    life: &mut Lifecycle,
    objects: &FsObjectStore,
    root: RootId,
    payload: &[u8],
) -> Result<(ObjectId, ObjectId, GenerationId), CacheError> {
    let (segment, generation) = view(life, root, u64::try_from(payload.len()).expect("len"))?;
    let manifest = life
        .manifest_object(life.manifest_of(root, generation).expect("manifest"))
        .expect("object");
    objects.put_object(segment.raw(), payload)?;
    objects.put_object(manifest.raw(), &segment.raw().to_le_bytes())?;
    Ok((segment, manifest, generation))
}

/// One publish through the exclusive section.
fn publish_files(
    ledger: &FsLedgerStore,
    objects: &FsObjectStore,
    quota: Quota,
    root: RootId,
    payload: &[u8],
) -> Result<(ObjectId, ObjectId, GenerationId), LedgerUpdateError<CacheError>> {
    ledger
        .update_reclaiming(quota, objects, |life| {
            if life.status().roots.iter().all(|row| row.id != root) {
                life.register_root(root)?;
            }
            Ok((write_view(life, objects, root, payload)?, true))
        })
        .map(|(value, _)| value)
}

fn lease_current(
    ledger: &FsLedgerStore,
    objects: &FsObjectStore,
    quota: Quota,
    root: RootId,
) -> LeaseId {
    ledger
        .update_reclaiming(quota, objects, |life| {
            let lease = life.reader_begin(root, SessionId::new(9))?;
            life.reader_confirm(lease)?;
            Ok::<_, CacheError>((lease, true))
        })
        .map(|(lease, _)| lease)
        .expect("lease")
}

fn release(ledger: &FsLedgerStore, objects: &FsObjectStore, quota: Quota, lease: LeaseId) {
    ledger
        .update_reclaiming(quota, objects, |life| {
            life.reader_release(lease)?;
            Ok::<_, CacheError>(((), true))
        })
        .expect("release");
}

fn object_files(root: &Path) -> usize {
    fs::read_dir(root.join("objects")).map_or(0, |dir| {
        dir.flatten().filter(|e| e.path().is_file()).count()
    })
}

/// The current generation read the way a reader does: ledger and object files under one shared lock.
fn read_current(
    ledger: &FsLedgerStore,
    root_dir: &Path,
    quota: Quota,
    root: RootId,
) -> Result<Option<Vec<u8>>, CacheError> {
    ledger
        .read(quota, |life| {
            let Some(generation) = life.current_generation(root) else {
                return Ok(None);
            };
            let manifest = life
                .manifest_object(life.manifest_of(root, generation).expect("manifest"))
                .expect("object");
            let named = fs::read(root_dir.join("objects").join(manifest.raw().to_string()))
                .map_err(|_| CacheError::LedgerIo {
                    reason: "manifest file missing",
                })?;
            let raw = u64::from_le_bytes(
                named
                    .as_slice()
                    .try_into()
                    .map_err(|_| CacheError::LedgerIo { reason: "manifest" })?,
            );
            fs::read(root_dir.join("objects").join(raw.to_string()))
                .map(Some)
                .map_err(|_| CacheError::LedgerIo {
                    reason: "segment file missing",
                })
        })
        .map_err(|error| match error {
            LedgerUpdateError::Cache(error) | LedgerUpdateError::Body(error) => error,
        })
}

#[test]
fn reclaim_unlinks_the_previous_generation_at_publish_but_not_a_leased_one() {
    let (_dir, dir) = cache_dir();
    let ledger = FsLedgerStore::open(&dir).expect("ledger");
    let objects = FsObjectStore::open(&dir).expect("objects");
    let quota = Quota::scaled(100_000);
    let root = RootId::new(1);
    let (first_segment, _, _) =
        publish_files(&ledger, &objects, quota, root, b"gen-1").expect("one");
    assert_eq!(object_files(&dir), 2);
    for n in 2..=5 {
        publish_files(
            &ledger,
            &objects,
            quota,
            root,
            format!("gen-{n}").as_bytes(),
        )
        .expect("publish");
        assert_eq!(
            object_files(&dir),
            2,
            "only the current segment and manifest stay after publish {n}"
        );
    }
    assert!(
        !dir.join("objects")
            .join(first_segment.raw().to_string())
            .exists()
    );
    assert_eq!(
        read_current(&ledger, &dir, quota, root).expect("read"),
        Some(b"gen-5".to_vec())
    );

    let (leased_segment, _, _) =
        publish_files(&ledger, &objects, quota, root, b"gen-6").expect("six");
    let lease = lease_current(&ledger, &objects, quota, root);
    publish_files(&ledger, &objects, quota, root, b"gen-7").expect("seven");
    publish_files(&ledger, &objects, quota, root, b"gen-8").expect("eight");
    // Leased generation 6 and current generation 8; generation 7 was unleased and is gone.
    assert_eq!(object_files(&dir), 4);
    assert_eq!(
        fs::read(dir.join("objects").join(leased_segment.raw().to_string())).expect("leased"),
        b"gen-6"
    );
    release(&ledger, &objects, quota, lease);
    assert_eq!(object_files(&dir), 2);
    assert!(
        !dir.join("objects")
            .join(leased_segment.raw().to_string())
            .exists()
    );
    let loaded = ledger.load(quota).expect("load");
    assert!(loaded.fault.is_none());
    assert!(loaded.lifecycle.p3_holds() && loaded.lifecycle.p4_holds());
    assert_eq!(loaded.lifecycle.status().buckets.pending, 0);
    assert_eq!(
        read_current(&ledger, &dir, quota, root).expect("read"),
        Some(b"gen-8".to_vec())
    );
}

#[test]
fn reclaim_frees_the_generation_of_a_reader_that_died_but_never_of_one_that_lives() {
    let (_dir, dir) = cache_dir();
    let ledger = FsLedgerStore::open(&dir).expect("ledger");
    let objects = FsObjectStore::open(&dir).expect("objects");
    let quota = Quota::scaled(100_000);
    let root = RootId::new(1);
    publish_files(&ledger, &objects, quota, root, b"gen-1").expect("one");
    let (lease, hold): (LeaseId, LeaseHold) = ledger
        .update_reclaiming(quota, &objects, |life| {
            let lease = life.reader_begin(root, SessionId::new(9))?;
            life.reader_confirm(lease)?;
            let hold = ledger.hold_lease(lease)?;
            Ok::<_, CacheError>(((lease, hold), true))
        })
        .map(|(value, _)| value)
        .expect("lease");
    for n in 2..=4 {
        publish_files(
            &ledger,
            &objects,
            quota,
            root,
            format!("gen-{n}").as_bytes(),
        )
        .expect("publish");
        // The reader is alive: its generation and the current one stay, the rest goes.
        assert_eq!(object_files(&dir), 4, "publish {n}");
    }
    assert_eq!(
        ledger
            .load(quota)
            .expect("load")
            .lifecycle
            .status()
            .active_leases,
        1
    );
    // The reader is killed: its lock file stays behind, unlocked.
    let path = hold.path().to_path_buf();
    drop(hold);
    assert!(!path.exists());
    fs::write(&path, b"").expect("what a killed process leaves");
    publish_files(&ledger, &objects, quota, root, b"gen-5").expect("publish");
    assert_eq!(
        object_files(&dir),
        2,
        "the dead reader's generation is reclaimed"
    );
    assert!(!path.exists(), "its liveness file is cleared");
    let status = ledger.load(quota).expect("load").lifecycle.status();
    assert_eq!(status.active_leases, 0);
    assert_eq!(
        read_current(&ledger, &dir, quota, root).expect("read"),
        Some(b"gen-5".to_vec())
    );
    let _ = lease;
}

const CUT_VIEW_BYTES: u64 = MANIFEST + 5;

fn assert_charges(loaded: &LoadedLedger, pending: u64, admitted: u64) {
    assert!(loaded.fault.is_none(), "{:?}", loaded.fault);
    let status = loaded.lifecycle.status();
    assert_eq!(status.buckets.pending, pending);
    assert_eq!(status.admitted, admitted);
}

#[test]
fn reclaim_reopens_before_rename_and_retries_with_the_existing_objects() {
    let (_dir, dir) = cache_dir();
    let ledger = FsLedgerStore::open(&dir).expect("ledger");
    let objects = FsObjectStore::open(&dir).expect("objects");
    let quota = Quota::scaled(100_000);
    let root = RootId::new(1);
    publish_files(&ledger, &objects, quota, root, b"gen-1").expect("one");
    // Construct the cut after staging the first reclaim image, before its rename.
    let mut next = ledger.load(quota).expect("load").lifecycle;
    let successor = write_view(&mut next, &objects, root, b"gen-2").expect("successor objects");
    assert_eq!(next.begin_reclaim(), 2);
    ledger.stage(&next).expect("stage successor");
    drop(ledger);
    drop(objects);

    let restarted = FsLedgerStore::open(&dir).expect("restart");
    assert_eq!(
        read_current(&restarted, &dir, quota, root).expect("read"),
        Some(b"gen-1".to_vec())
    );
    let loaded = restarted.load(quota).expect("load");
    assert_charges(&loaded, 0, CUT_VIEW_BYTES);
    assert_eq!(loaded.orphans.len(), 3, "{:?}", loaded.orphans);
    for id in [successor.0, successor.1] {
        let relative = format!("objects/{}", id.raw());
        assert!(loaded.orphans.iter().any(|file| file.relative == relative));
    }
    let staged = loaded
        .orphans
        .iter()
        .find(|file| file.relative.starts_with("ledger.") && file.relative.ends_with(".tmp"))
        .expect("staged ledger orphan");
    assert_eq!(
        fs::read(dir.join(&staged.relative)).expect("read stage"),
        next.encode_ledger().expect("encode successor")
    );

    // Replay the same publication against the immutable files already at its IDs.
    let again = FsObjectStore::open(&dir).expect("objects");
    assert_eq!(
        publish_files(&restarted, &again, quota, root, b"gen-2").expect("retry publish"),
        successor
    );
    let reopened = FsLedgerStore::open(&dir).expect("reopen after retry");
    assert_eq!(
        read_current(&reopened, &dir, quota, root).expect("read"),
        Some(b"gen-2".to_vec())
    );
    let settled = reopened.load(quota).expect("load after retry");
    assert_charges(&settled, 0, CUT_VIEW_BYTES);
    assert_eq!(settled.orphans, std::slice::from_ref(staged));
    assert_eq!(object_files(&dir), 2);
}

fn reopen_pending_image(unlink_one: bool) {
    let (_dir, dir) = cache_dir();
    let ledger = FsLedgerStore::open(&dir).expect("ledger");
    let objects = FsObjectStore::open(&dir).expect("objects");
    let quota = Quota::scaled(100_000);
    let root = RootId::new(1);
    let (old_segment, old_manifest, _) =
        publish_files(&ledger, &objects, quota, root, b"gen-1").expect("one");
    // Construct the first committed image. Optionally complete the first unlink before reopening.
    let mut life = ledger.load(quota).expect("load").lifecycle;
    write_view(&mut life, &objects, root, b"gen-2").expect("successor objects");
    assert_eq!(life.begin_reclaim(), 2);
    ledger.store(&life).expect("first commit");
    if unlink_one {
        objects.unlink_object(old_segment).expect("first unlink");
    }
    drop(ledger);
    drop(objects);

    let restarted = FsLedgerStore::open(&dir).expect("restart");
    let again = FsObjectStore::open(&dir).expect("objects");
    assert_eq!(
        read_current(&restarted, &dir, quota, root).expect("read"),
        Some(b"gen-2".to_vec())
    );
    let loaded = restarted.load(quota).expect("load");
    assert_charges(&loaded, CUT_VIEW_BYTES, 2 * CUT_VIEW_BYTES);
    for id in [old_segment, old_manifest] {
        assert_eq!(
            loaded.lifecycle.object_state(id),
            Some(Physical::PendingUnlink)
        );
    }
    assert_eq!(
        again
            .open_object(old_segment.raw())
            .expect("old segment")
            .is_some(),
        !unlink_one
    );
    assert!(
        again
            .open_object(old_manifest.raw())
            .expect("old manifest")
            .is_some()
    );
    assert!(loaded.orphans.is_empty());

    publish_files(&restarted, &again, quota, root, b"gen-3").expect("publish");
    let reopened = FsLedgerStore::open(&dir).expect("reopen after publish");
    let settled = reopened.load(quota).expect("load after publish");
    assert_charges(&settled, 0, CUT_VIEW_BYTES);
    assert!(settled.orphans.is_empty(), "{:?}", settled.orphans);
    for id in [old_segment, old_manifest] {
        assert_eq!(settled.lifecycle.object_state(id), None);
        assert!(again.open_object(id.raw()).expect("old object").is_none());
    }
    assert_eq!(object_files(&dir), 2);
    assert_eq!(
        read_current(&reopened, &dir, quota, root).expect("read"),
        Some(b"gen-3".to_vec())
    );
}

#[test]
fn reclaim_reopens_at_the_first_pending_image_commit() {
    reopen_pending_image(false);
}

#[test]
fn reclaim_reopens_after_one_pending_unlink() {
    reopen_pending_image(true);
}

#[cfg(unix)]
#[test]
fn orphan_diagnostics_escape_hostile_names() {
    let (_dir, dir) = cache_dir();
    let name = "orphan\n\x1b\"";
    fs::write(dir.join(name), b"stray").expect("write orphan");
    let loaded = FsLedgerStore::open(&dir)
        .expect("ledger")
        .load(Quota::scaled(100_000))
        .expect("load");
    assert_eq!(loaded.orphans.len(), 1, "{:?}", loaded.orphans);
    assert_eq!(loaded.orphans[0].relative, name);
    assert_eq!(
        format!("{:?}", loaded.orphans),
        r#"[OrphanFile { relative: "orphan\n\u{1b}\"" }]"#
    );
}

#[test]
fn evict_ten_publishes_under_a_cap_of_three_generations_refuse_nothing() {
    let generation = 1_000 + MANIFEST;
    let mut life = Lifecycle::new(Quota::scaled(3 * generation)).expect("quota");
    let root = RootId::new(1);
    life.register_root(root).expect("root");
    for round in 0..10 {
        view(&mut life, root, 1_000).unwrap_or_else(|error| panic!("publish {round}: {error}"));
        assert!(life.p4_holds() && life.p3_holds());
    }
    assert!(life.admitted() <= 3 * generation);
    assert!(!life.take_evicted().is_empty());
}

#[test]
fn evict_never_takes_a_leased_generation_and_refuses_only_when_nothing_is_reclaimable() {
    let generation = 1_000 + MANIFEST;
    let mut life = Lifecycle::new(Quota::scaled(3 * generation)).expect("quota");
    let root = RootId::new(1);
    life.register_root(root).expect("root");
    let (first_segment, first) = view(&mut life, root, 1_000).expect("first");
    let lease = life.reader_begin(root, SessionId::new(1)).expect("begin");
    life.reader_confirm(lease).expect("confirm");
    for round in 0..10 {
        view(&mut life, root, 1_000).unwrap_or_else(|error| panic!("publish {round}: {error}"));
    }
    assert_eq!(state_of(&life, root, first), Some(Physical::Published));
    assert_eq!(life.object_bytes(first_segment), Some(1_000));
    assert!(!life.take_evicted().contains(&first_segment));
    // Leases on three generations fill the cap of three; a fourth publication has nothing to evict.
    let second = life.reader_begin(root, SessionId::new(2)).expect("begin");
    life.reader_confirm(second).expect("confirm");
    view(&mut life, root, 1_000).expect("two held generations and the new one fit");
    let third = life.reader_begin(root, SessionId::new(3)).expect("begin");
    life.reader_confirm(third).expect("confirm");
    let refused = view(&mut life, root, 1_000);
    assert!(
        matches!(refused, Err(CacheError::CacheLimit { .. })),
        "{refused:?}"
    );
    assert!(life.p3_holds() && life.p4_holds());
    life.reader_release(lease).expect("release");
    view(&mut life, root, 1_000).expect("room again once the lease is gone");
}

#[test]
fn evict_takes_the_oldest_retired_generation_across_roots_first() {
    let mut life = Lifecycle::new(Quota::scaled(3_000)).expect("quota");
    let (left, right, third) = (RootId::new(1), RootId::new(2), RootId::new(3));
    for root in [left, right, third] {
        life.register_root(root).expect("root");
    }
    let mut clock = dctx_adapter_cache::FakeClock::at(10);
    life.follow(&clock);
    let (old_left, _) = view(&mut life, left, 700).expect("left one");
    clock.advance(10);
    life.follow(&clock);
    let (old_right, _) = view(&mut life, right, 700).expect("right one");
    clock.advance(10);
    life.follow(&clock);
    view(&mut life, left, 700).expect("left two retires left one");
    clock.advance(10);
    life.follow(&clock);
    view(&mut life, right, 700).expect("right two retires right one");
    assert!(
        life.reclaimable_ids().contains(&old_left) && life.reclaimable_ids().contains(&old_right)
    );
    // 4 x 708 = 2832 admitted of 3000. A 700-byte segment for a third root needs one retired generation gone.
    let (made, _) = view(&mut life, third, 700).expect("third root fits after eviction");
    let evicted = life.take_evicted();
    assert!(
        evicted.contains(&old_left),
        "the oldest retired generation goes first: {evicted:?}"
    );
    assert!(!evicted.contains(&old_right));
    assert_eq!(life.object_bytes(old_right), Some(700));
    assert_eq!(life.object_bytes(made), Some(700));
}

#[test]
fn evict_keeps_an_object_the_new_publication_references() {
    let mut life = Lifecycle::new(Quota::scaled(1_500)).expect("quota");
    let root = RootId::new(1);
    life.register_root(root).expect("root");
    let shared = life.insert_fact(1_000, &[root]).expect("fact");
    life.publish_view(root, MANIFEST, &[shared]).expect("first");
    life.publish_view(root, MANIFEST, &[])
        .expect("second retires the first");
    assert!(life.reclaimable_ids().contains(&shared));
    // The next view names the fact the retired one named; making room must not drop it.
    life.publish_view(root, 400, &[shared])
        .expect("third reuses the fact");
    assert!(!life.take_evicted().contains(&shared));
    assert_eq!(life.object_bytes(shared), Some(1_000));
    assert!(life.p3_holds() && life.p4_holds());
}

#[test]
fn evicted_files_are_unlinked_before_the_ledger_commits() {
    let (_dir, dir) = cache_dir();
    let ledger = FsLedgerStore::open(&dir).expect("ledger");
    let objects = FsObjectStore::open(&dir).expect("objects");
    let generation = 8 + 5;
    let quota = Quota::scaled(2 * generation);
    let root = RootId::new(1);
    // The ledger holds two retired generations that never went through a reclaiming commit.
    let mut life = Lifecycle::new(quota).expect("quota");
    life.register_root(root).expect("root");
    for n in 0..3 {
        let segment = life.insert_segment(5, 0, &[root]).ok();
        if let Some(segment) = segment {
            let generation = life.publish_view(root, MANIFEST, &[segment]).expect("view");
            let manifest = life
                .manifest_object(life.manifest_of(root, generation).expect("m"))
                .expect("o");
            objects.put_object(segment.raw(), b"data!").expect("put");
            objects
                .put_object(manifest.raw(), &segment.raw().to_le_bytes())
                .expect("put");
        } else {
            panic!("publish {n} refused");
        }
    }
    assert!(!life.evicted_ids().is_empty());
    let before = object_files(&dir);
    ledger.store(&life).expect("store");
    assert!(
        object_files(&dir) < before,
        "{before} -> {}",
        object_files(&dir)
    );
    let loaded = ledger.load(quota).expect("load");
    assert!(
        loaded.orphans.is_empty(),
        "no file outlives its charge: {:?}",
        loaded.orphans
    );
}

#[test]
fn evict_reopens_after_pressure_unlinks_before_commit() {
    let (_dir, dir) = cache_dir();
    let ledger = FsLedgerStore::open(&dir).expect("ledger");
    let objects = FsObjectStore::open(&dir).expect("objects");
    let quota = Quota::scaled(2 * CUT_VIEW_BYTES);
    let root = RootId::new(1);
    let mut life = Lifecycle::new(quota).expect("quota");
    life.register_root(root).expect("root");
    let (retired_segment, retired_manifest, _) =
        write_view(&mut life, &objects, root, b"gen-1").expect("retired view");
    let (_, _, current) = write_view(&mut life, &objects, root, b"gen-2").expect("current view");
    ledger.store(&life).expect("commit at capacity");

    // Admission evicts the retired view to fit the successor. Construct the cut after the
    // adapter has unlinked those names and staged the first image, before the rename.
    let successor = write_view(&mut life, &objects, root, b"gen-3").expect("admit under pressure");
    let evicted = [retired_segment, retired_manifest];
    assert_eq!(life.evicted_ids(), evicted);
    assert_eq!(life.begin_reclaim(), 2);
    for id in evicted {
        assert!(
            objects
                .open_object(id.raw())
                .expect("eviction target")
                .is_some()
        );
        objects.unlink_object(id).expect("evict object file");
    }
    ledger.stage(&life).expect("stage successor");
    drop(ledger);
    drop(objects);

    let restarted = FsLedgerStore::open(&dir).expect("restart");
    let again = FsObjectStore::open(&dir).expect("objects");
    let loaded = restarted.load(quota).expect("load");
    assert_charges(&loaded, 0, 2 * CUT_VIEW_BYTES);
    assert_eq!(loaded.lifecycle.current_generation(root), Some(current));
    assert_eq!(loaded.lifecycle.status().reclaimable_bytes, CUT_VIEW_BYTES);
    for id in evicted {
        assert!(loaded.lifecycle.object_bytes(id).is_some());
        assert!(again.open_object(id.raw()).expect("evicted name").is_none());
    }
    assert_eq!(
        read_current(&restarted, &dir, quota, root).expect("read"),
        Some(b"gen-2".to_vec())
    );

    assert_eq!(
        publish_files(&restarted, &again, quota, root, b"gen-3").expect("retry admission"),
        successor
    );
    let reopened = FsLedgerStore::open(&dir).expect("reopen after retry");
    let settled = reopened.load(quota).expect("load after retry");
    assert_charges(&settled, 0, CUT_VIEW_BYTES);
    for id in evicted {
        assert_eq!(settled.lifecycle.object_state(id), None);
    }
    assert_eq!(settled.orphans.len(), 1, "{:?}", settled.orphans);
    assert!(settled.orphans[0].relative.ends_with(".tmp"));
    assert_eq!(object_files(&dir), 2);
    assert_eq!(
        read_current(&reopened, &dir, quota, root).expect("read"),
        Some(b"gen-3".to_vec())
    );
}

#[test]
fn shared_read_eight_parallel_readers_hold_the_lock_together_and_none_is_busy() {
    let (_dir, dir) = cache_dir();
    let quota = Quota::scaled(100_000);
    let root = RootId::new(1);
    {
        let ledger = FsLedgerStore::open(&dir).expect("ledger");
        let objects = FsObjectStore::open(&dir).expect("objects");
        publish_files(&ledger, &objects, quota, root, b"gen-1").expect("one");
    }
    let inside = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let met = Arc::new(AtomicBool::new(false));
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let (dir, inside, peak, met) = (
                dir.clone(),
                Arc::clone(&inside),
                Arc::clone(&peak),
                Arc::clone(&met),
            );
            std::thread::spawn(move || {
                let ledger = FsLedgerStore::open(&dir).expect("ledger");
                ledger.read(quota, |_life| {
                    let now = inside.fetch_add(1, Ordering::SeqCst) + 1;
                    if peak.fetch_max(now, Ordering::SeqCst).max(now) >= 8 {
                        met.store(true, Ordering::SeqCst);
                    }
                    let deadline = Instant::now() + Duration::from_secs(5);
                    while !met.load(Ordering::SeqCst) && Instant::now() < deadline {
                        std::thread::sleep(Duration::from_millis(2));
                    }
                    inside.fetch_sub(1, Ordering::SeqCst);
                    Ok::<_, CacheError>(())
                })
            })
        })
        .collect();
    for handle in handles {
        let result = handle.join().expect("thread");
        assert!(
            result.is_ok(),
            "a reader was refused: {:?}",
            result.err().map(|error| match error {
                LedgerUpdateError::Cache(error) | LedgerUpdateError::Body(error) => error,
            })
        );
    }
    assert_eq!(
        peak.load(Ordering::SeqCst),
        8,
        "readers excluded each other"
    );
}

#[test]
fn shared_read_waits_for_a_publish_instead_of_failing_busy() {
    let (_dir, dir) = cache_dir();
    let quota = Quota::scaled(100_000);
    let root = RootId::new(1);
    let ledger = FsLedgerStore::open(&dir).expect("ledger");
    let objects = FsObjectStore::open(&dir).expect("objects");
    publish_files(&ledger, &objects, quota, root, b"gen-1").expect("one");
    let entered = Arc::new(AtomicUsize::new(0));
    let reader = {
        let (dir, entered) = (dir.clone(), Arc::clone(&entered));
        std::thread::spawn(move || {
            while entered.load(Ordering::SeqCst) == 0 {
                std::thread::sleep(Duration::from_millis(1));
            }
            let ledger = FsLedgerStore::open(&dir).expect("reader ledger");
            read_current(&ledger, &dir, quota, root)
        })
    };
    // The publish section holds the exclusive lock for 150 ms; the reader arrives in the middle of it.
    ledger
        .update_reclaiming(quota, &objects, |life| {
            let segment = life.insert_segment(5, 0, &[root])?;
            let generation = life.publish_view(root, MANIFEST, &[segment])?;
            let manifest = life
                .manifest_object(life.manifest_of(root, generation).expect("m"))
                .expect("o");
            objects.put_object(segment.raw(), b"gen-2")?;
            objects.put_object(manifest.raw(), &segment.raw().to_le_bytes())?;
            entered.store(1, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(150));
            Ok::<_, CacheError>(((), true))
        })
        .expect("publish");
    let seen = reader
        .join()
        .expect("reader")
        .expect("the reader waited, it was not refused");
    assert_eq!(
        seen,
        Some(b"gen-2".to_vec()),
        "a waiting reader sees the committed generation"
    );
}

#[test]
fn shared_read_never_sees_a_half_published_or_reclaimed_generation() {
    let (_dir, dir) = cache_dir();
    let quota = Quota::scaled(1_000_000);
    let root = RootId::new(1);
    {
        let ledger = FsLedgerStore::open(&dir).expect("ledger");
        let objects = FsObjectStore::open(&dir).expect("objects");
        publish_files(&ledger, &objects, quota, root, &[0u8; 4096]).expect("seed");
    }
    let stop = Arc::new(AtomicUsize::new(0));
    let readers: Vec<_> = (0..4)
        .map(|_| {
            let (dir, stop) = (dir.clone(), Arc::clone(&stop));
            std::thread::spawn(move || {
                let ledger = FsLedgerStore::open(&dir).expect("reader");
                let mut reads = 0u32;
                while stop.load(Ordering::SeqCst) == 0 {
                    let bytes = read_current(&ledger, &dir, quota, root)
                        .expect("a reader found a generation whose file was reclaimed")
                        .expect("a current generation exists");
                    assert_eq!(bytes.len(), 4096);
                    assert!(
                        bytes.iter().all(|byte| *byte == bytes[0]),
                        "torn generation"
                    );
                    reads += 1;
                }
                reads
            })
        })
        .collect();
    let ledger = FsLedgerStore::open(&dir).expect("writer");
    let objects = FsObjectStore::open(&dir).expect("objects");
    for round in 1..=25u8 {
        publish_files(&ledger, &objects, quota, root, &[round; 4096]).expect("publish");
    }
    stop.store(1, Ordering::SeqCst);
    let total: u32 = readers
        .into_iter()
        .map(|handle| handle.join().expect("reader"))
        .sum();
    assert!(total > 0);
    assert_eq!(object_files(&dir), 2);
}
