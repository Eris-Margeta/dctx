// SPDX-License-Identifier: Apache-2.0
//! `Cache` port and the filesystem object store.
//! Implements API-PORTS-v1 `Cache.reserve/commit/abort` and CACHE-v1 unlink inside the cache root.
//! Depends on [`crate::Lifecycle`] for admission and on std filesystem calls for reclaimed names.
//! Root ceilings stay on `reserve_for`. Unlink refuses symlinks and `..` instead of following them.

use super::publish::publish_immutable;
use super::{ensure_dir, locate, orphans, real_root};
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
    /// # Errors
    /// Refuses unsafe paths or directory/object write failures under the cache root.
    pub fn put_object(&self, raw: u64, bytes: &[u8]) -> Result<(), CacheError> {
        let dir = self.root.join("objects");
        ensure_dir(&dir)?;
        let path = dir.join(raw.to_string());
        if !path.starts_with(&self.root) {
            return Err(CacheError::OutsideCacheRoot);
        }
        publish_immutable(&path, bytes)
    }

    /// Opens an existing regular object for a bounded read by the caller.
    /// # Errors
    /// Refuses linked storage, non-regular objects, unsafe Unix ownership/write permissions, or I/O failures.
    pub fn open_object(&self, raw: u64) -> Result<Option<std::fs::File>, CacheError> {
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
        if !meta.is_file() {
            return Err(CacheError::OutsideCacheRoot);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if meta.uid() != rustix::process::geteuid().as_raw() || meta.mode() & 0o022 != 0 {
                return Err(CacheError::OutsideCacheRoot);
            }
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if meta.file_attributes() & 0x400 != 0 {
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
            Ok(meta) if meta.file_type().is_symlink() => Err(CacheError::OutsideCacheRoot),
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
}
