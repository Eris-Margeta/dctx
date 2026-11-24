// SPDX-License-Identifier: Apache-2.0
//! `Cache` port and the filesystem object store.
//! Implements API-PORTS-v1 `Cache.reserve/commit/abort` and CACHE-v1 unlink inside the cache root.
//! Depends on [`crate::Lifecycle`] for admission and on std filesystem calls for reclaimed names.
//! Root ceilings stay on `reserve_for`. Unlink refuses symlinks and `..` instead of following them.

use super::publish::{publish_immutable, sync_publication_directory};
use super::{ensure_dir, locate, orphans, real_root, require_directory};
use crate::domain::{Lifecycle, ObjectId, Purpose};
use crate::error::CacheError;
use crate::ports::{ObjectStore, OrphanFile};
use dctx_core::budget::Reservation;
use dctx_core::error::DctxError;
use dctx_ports::Cache;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

impl Cache for Lifecycle {
    fn reserve(&mut self, bytes: u64) -> Result<Reservation, DctxError> {
        self.reserve_for(None, Purpose::Payload, bytes)
            .map_err(CacheError::into_dctx)
    }

    fn commit(&mut self, reservation: Reservation, used_bytes: u64) -> Result<(), DctxError> {
        self.commit_reservation(reservation, used_bytes)
            .map_err(CacheError::into_dctx)
    }

    fn abort(&mut self, reservation: Reservation) {
        self.abort_reservation(reservation);
    }
}

/// Object files under `{root}/objects/{id}`. Names that leave the root are refused.
#[derive(Debug)]
pub struct FsObjectStore {
    root: PathBuf,
}

impl FsObjectStore {
    /// `root` must already be a real directory. A symlink root is refused.
    /// # Errors
    /// Refuses an unsafe/non-directory cache root or filesystem metadata failures.
    pub fn open(root: &Path) -> Result<Self, CacheError> {
        Ok(Self {
            root: real_root(root)?,
        })
    }

    /// Publishes one object file. An existing object is never truncated.
    /// On Unix, success includes syncing the object directory and cache root, even on reuse.
    /// # Errors
    /// Refuses unsafe paths or directory/object write failures under the cache root.
    pub fn put_object(&self, raw: u64, bytes: &[u8]) -> Result<(), CacheError> {
        require_directory(&self.root)?;
        let dir = self.root.join("objects");
        ensure_dir(&dir)?;
        let path = dir.join(raw.to_string());
        if !path.starts_with(&self.root) {
            return Err(CacheError::OutsideCacheRoot);
        }
        publish_immutable(&path, bytes)?;
        // An existing `objects` entry may have been created by an interrupted publisher.
        sync_publication_directory(&self.root)
    }

    /// Opens an existing regular object for a bounded read by the caller.
    /// # Errors
    /// Refuses linked storage, non-regular objects, unsafe Unix ownership/write permissions, or I/O failures.
    pub fn open_object(&self, raw: u64) -> Result<Option<std::fs::File>, CacheError> {
        require_directory(&self.root)?;
        let directory = self.root.join("objects");
        match std::fs::symlink_metadata(&directory) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => {
                return Err(CacheError::LedgerIo {
                    reason: "object directory metadata",
                });
            }
            Ok(_) => {
                real_root(&directory)?;
            }
        }
        let mut options = std::fs::File::options();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(super::no_follow() | rustix::fs::OFlags::NONBLOCK.bits() as i32);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.custom_flags(0x0020_0000); // FILE_FLAG_OPEN_REPARSE_POINT
        }
        let file = match options.open(directory.join(raw.to_string())) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => {
                return Err(CacheError::LedgerIo {
                    reason: "object open",
                });
            }
        };
        let meta = file.metadata().map_err(|_| CacheError::LedgerIo {
            reason: "object metadata",
        })?;
        if !meta.is_file() || super::is_link(&meta) {
            return Err(CacheError::OutsideCacheRoot);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if meta.uid() != rustix::process::geteuid().as_raw() || meta.mode() & 0o022 != 0 {
                return Err(CacheError::OutsideCacheRoot);
            }
        }
        Ok(Some(file))
    }

    /// Unlinks `relative` only when every ancestor inside the root is a real directory.
    /// # Errors
    /// Refuses escaping/symlink/non-file paths or metadata/unlink failures; missing files are accepted.
    pub fn unlink_relative(&self, relative: &Path) -> Result<(), CacheError> {
        let Some(path) = locate(&self.root, relative)? else {
            return Ok(());
        };
        match std::fs::symlink_metadata(&path) {
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(CacheError::LedgerIo { reason: "metadata" }),
            Ok(meta) if super::is_link(&meta) => Err(CacheError::OutsideCacheRoot),
            Ok(meta) if meta.is_file() => {
                std::fs::remove_file(&path).map_err(|_| CacheError::LedgerIo { reason: "unlink" })
            }
            Ok(_) => Err(CacheError::OutsideCacheRoot),
        }
    }
}

impl ObjectStore for FsObjectStore {
    fn unlink_object(&self, id: ObjectId) -> Result<(), CacheError> {
        let relative = PathBuf::from(format!("objects/{}", id.raw()));
        self.unlink_relative(&relative)
    }

    fn orphans(&self, known: &BTreeSet<ObjectId>) -> Result<Vec<OrphanFile>, CacheError> {
        let ids = known.iter().copied().map(ObjectId::raw).collect();
        orphans::list_orphans(&self.root, &ids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn root() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("temp");
        let root = dir.path().join("cache");
        fs::create_dir(&root).expect("dir");
        (dir, root)
    }

    #[test]
    fn lexical_escape_and_a_real_file_are_opposite_outcomes() {
        let (dir, root) = root();
        let secret = dir.path().join("secret");
        fs::write(&secret, b"keep").expect("secret");
        let objects = FsObjectStore::open(&root).expect("open");
        assert!(matches!(
            objects.unlink_relative(Path::new("../secret")),
            Err(CacheError::OutsideCacheRoot)
        ));
        assert_eq!(fs::read(&secret).expect("secret"), b"keep");
        objects.put_object(7, b"inside").expect("put");
        assert!(root.join("objects/7").is_file());
        objects
            .unlink_relative(Path::new("objects/7"))
            .expect("unlink");
        assert!(!root.join("objects/7").exists());
        assert_eq!(fs::read(&secret).expect("secret"), b"keep");
    }

    #[cfg(unix)]
    #[test]
    fn symlink_escape_does_not_unlink_the_outside_target() {
        let (dir, root) = root();
        let secret = dir.path().join("secret");
        fs::write(&secret, b"keep").expect("secret");
        std::os::unix::fs::symlink(&secret, root.join("escape")).expect("link");
        let outside = dir.path().join("outside");
        fs::create_dir(&outside).expect("dir");
        fs::write(outside.join("victim"), b"v").expect("victim");
        std::os::unix::fs::symlink(&outside, root.join("via")).expect("dir link");
        let objects = FsObjectStore::open(&root).expect("open");
        assert!(objects.unlink_relative(Path::new("escape")).is_err());
        assert!(objects.unlink_relative(Path::new("via/victim")).is_err());
        assert_eq!(fs::read(&secret).expect("secret"), b"keep");
        assert_eq!(fs::read(outside.join("victim")).expect("victim"), b"v");
        assert!(root.join("escape").exists());
    }

    #[cfg(unix)]
    #[test]
    fn first_publication_syncs_the_cache_root_before_ledger_commit() {
        use super::super::publish::before_next_directory_sync;
        use super::super::{FsLedgerStore, LEDGER_FILE, LedgerUpdateError};
        use crate::{Quota, RootId};
        use std::cell::Cell;
        use std::rc::Rc;

        let (_dir, root) = root();
        let ledger = FsLedgerStore::open(&root).expect("ledger");
        let objects = FsObjectStore::open(&root).expect("objects");
        let quota = Quota::scaled(100_000);
        let id = RootId::new(1);
        let payload = b"first";
        let object_dir = root.join("objects");
        let ledger_path = root.join(LEDGER_FILE);
        assert!(!object_dir.exists());

        let publish = |root_sync_result| {
            let root_sync_reached = Rc::new(Cell::new(false));
            let result = ledger.update_reclaiming(quota, &objects, |life| {
                life.register_root(id)?;
                let object = life.insert_segment(5, 0, &[id])?;
                let cache_root = root.clone();
                let object_dir = object_dir.clone();
                let ledger_path = ledger_path.clone();
                let root_sync_reached = Rc::clone(&root_sync_reached);
                before_next_directory_sync(move |directory| {
                    assert_eq!(directory, object_dir);
                    assert_eq!(
                        fs::read(directory.join(object.raw().to_string())).expect("linked object"),
                        payload
                    );
                    assert!(!ledger_path.exists());
                    before_next_directory_sync(move |directory| {
                        assert_eq!(directory, cache_root);
                        assert!(!ledger_path.exists());
                        root_sync_reached.set(true);
                        root_sync_result
                    });
                    Ok(())
                });
                objects.put_object(object.raw(), payload)?;
                Ok::<_, CacheError>((object, true))
            });
            assert!(
                root_sync_reached.get(),
                "publication must reach cache root sync"
            );
            result
        };

        let result = publish(Err(CacheError::LedgerIo {
            reason: "directory sync",
        }));
        assert!(matches!(
            result,
            Err(LedgerUpdateError::Body(CacheError::LedgerIo {
                reason: "directory sync"
            }))
        ));
        assert!(object_dir.is_dir());
        assert!(!ledger_path.exists());

        let (object, _) = publish(Ok(())).expect("retry first publication");
        assert_eq!(
            ledger
                .read(quota, |life| Ok::<_, CacheError>(life.object_bytes(object)))
                .expect("read committed ledger"),
            Some(5)
        );
    }

    #[cfg(unix)]
    #[test]
    fn directory_sync_failure_preserves_the_committed_view_and_allows_retry() {
        use super::super::publish::before_next_directory_sync;
        use super::super::{FsLedgerStore, LEDGER_FILE, LedgerUpdateError};
        use crate::{Quota, RootId};
        use std::cell::Cell;
        use std::rc::Rc;

        let dir = tempfile::tempdir().expect("temp");
        let root = dir.path().join("cache\n\x1b\"");
        fs::create_dir(&root).expect("cache directory");
        let ledger = FsLedgerStore::open(&root).expect("ledger");
        let objects = FsObjectStore::open(&root).expect("objects");
        let quota = Quota::scaled(100_000);
        let id = RootId::new(1);
        let old_payload = b"old view\0\xff";
        let new_payload = b"new view\0\xfe";
        let object_dir = root.join("objects");
        let ledger_path = root.join(LEDGER_FILE);

        let publish = |payload: &[u8], before_manifest: &dyn Fn(ObjectId, ObjectId)| {
            ledger
                .update_reclaiming(quota, &objects, |life| {
                    if life.current_generation(id).is_none() {
                        life.register_root(id)?;
                    }
                    let bytes = u64::try_from(payload.len()).expect("payload length");
                    let segment = life.insert_segment(bytes, 0, &[id])?;
                    let generation = life.publish_view(id, 8, &[segment])?;
                    let manifest = life
                        .manifest_object(life.manifest_of(id, generation).expect("manifest"))
                        .expect("manifest object");
                    objects.put_object(segment.raw(), payload)?;
                    before_manifest(segment, manifest);
                    objects.put_object(manifest.raw(), &segment.raw().to_le_bytes())?;
                    Ok::<_, CacheError>((generation, true))
                })
                .map(|(generation, _)| generation)
        };
        let read_current = || {
            ledger
                .read(quota, |life| {
                    let generation = life.current_generation(id).expect("current view");
                    let manifest = life
                        .manifest_object(life.manifest_of(id, generation).expect("manifest"))
                        .expect("manifest object");
                    let bytes = fs::read(object_dir.join(manifest.raw().to_string()))
                        .expect("manifest bytes");
                    let segment = u64::from_le_bytes(bytes.try_into().expect("segment id"));
                    let payload = fs::read(object_dir.join(segment.to_string())).expect("payload");
                    Ok::<_, CacheError>((generation, payload))
                })
                .expect("read committed view")
        };

        let old = publish(old_payload, &|_, _| {}).expect("commit old view");
        let old_ledger = fs::read(&ledger_path).expect("old ledger");
        let replace = |fail_sync| {
            let sync_reached = Rc::new(Cell::new(false));
            let result = publish(new_payload, &|segment, manifest| {
                let sync_reached = Rc::clone(&sync_reached);
                let object_dir = object_dir.clone();
                let ledger_path = ledger_path.clone();
                let old_ledger = old_ledger.clone();
                before_next_directory_sync(move |directory| {
                    sync_reached.set(true);
                    assert_eq!(directory, object_dir);
                    assert_eq!(
                        fs::read(directory.join(manifest.raw().to_string()))
                            .expect("linked manifest"),
                        segment.raw().to_le_bytes()
                    );
                    assert_eq!(
                        fs::read(directory.join(segment.raw().to_string()))
                            .expect("linked segment"),
                        new_payload
                    );
                    assert_eq!(fs::read(ledger_path).expect("committed ledger"), old_ledger);
                    if fail_sync {
                        return Err(CacheError::LedgerIo {
                            reason: "directory sync",
                        });
                    }
                    Ok(())
                });
            });
            assert!(sync_reached.get(), "manifest must reach directory sync");
            result
        };

        let error = replace(true).expect_err("directory sync must prevent commit");
        assert!(matches!(
            error,
            LedgerUpdateError::Body(CacheError::LedgerIo {
                reason: "directory sync"
            })
        ));
        assert!(!error.to_string().contains(['\n', '\x1b', '"']));
        assert_eq!(
            fs::read(&ledger_path).expect("preserved ledger"),
            old_ledger
        );
        assert_eq!(read_current(), (old, old_payload.to_vec()));

        let new = replace(false).expect("retry with identical object files");
        assert_ne!(new, old);
        assert_eq!(read_current(), (new, new_payload.to_vec()));
    }
}
