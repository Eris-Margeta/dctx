// SPDX-License-Identifier: Apache-2.0
//! Publish section: one exclusive lock covers the fresh load, the edit, the unlink of reclaimed files and the commit.
//! Implements P-C10 and P-C12: a superseded view is unlinked right after the new one is durable, and only
//! this section takes the exclusive lock. Depends on [`super::FsLedgerStore`] and [`super::FsObjectStore`].
//! A crash after the first commit leaves pending charges that the next section unlinks. A reader holds the shared lock.

use super::{FsLedgerStore, FsObjectStore, LedgerUpdateError, lock};
use crate::domain::{LeaseId, Lifecycle, Quota};
use crate::error::CacheError;
use crate::ports::ObjectStore;
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};

/// Name of the file whose lock proves a reader is alive: `lease.<id>.lock` in the cache root.
fn lease_lock_name(id: LeaseId) -> String {
    format!("lease.{}.lock", id.raw())
}

/// True for a reader's liveness file. Orphan listing skips these.
pub(super) fn is_lease_lock(relative: &str) -> bool {
    relative
        .strip_prefix("lease.")
        .and_then(|rest| rest.strip_suffix(".lock"))
        .is_some_and(|id| !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit()))
}

/// A reader's proof of life. The reader keeps an exclusive lock on its file for as long as it holds the lease;
/// the kernel drops the lock when the process dies, which is how the next publish learns the lease is orphaned.
#[derive(Debug)]
pub struct LeaseHold {
    _lock: File,
    path: PathBuf,
}

impl LeaseHold {
    /// The file this hold locks.
    #[allow(clippy::must_use_candidate, reason = "Read-only accessor.")]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Dropping the hold ends the proof of life: the file goes first, the lock with the handle.
/// Drop it only after the lease is released in the ledger.
impl Drop for LeaseHold {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// An unlocked liveness file proves its owner is gone. Keep the lock through the release commit.
struct DeadLease {
    file: File,
    path: PathBuf,
}

impl DeadLease {
    fn probe(path: PathBuf) -> Option<Self> {
        let file = open_lease(&path, false).ok()?;
        file.try_lock().ok()?;
        Some(Self { file, path })
    }

    /// Remove evidence only while the ledger lock is still held and the path has no live holder.
    fn remove(self) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if let (Ok(held), Ok(named)) = (self.file.metadata(), fs::symlink_metadata(&self.path))
                && (held.dev(), held.ino()) == (named.dev(), named.ino())
            {
                let _ = fs::remove_file(&self.path);
            }
        }
        #[cfg(not(unix))]
        {
            // Check the current file for a holder again on platforms without stable inode metadata.
            drop(self.file);
            if let Some(evidence) = Self::probe(self.path) {
                let _ = fs::remove_file(&evidence.path);
            }
        }
    }
}

impl FsLedgerStore {
    /// Loads the ledger fresh under the exclusive lock, runs `body`, and when `body` commits, reclaims what the edit
    /// superseded and stores. `body` returns `(value, commit)`. A dead lease also triggers a commit.
    /// The committed ledger first names the reclaimable objects as pending, then the files are unlinked and a second
    /// commit drops their charge. An unlink that refuses stays pending and is retried by the next section.
    /// Returns `value` and the ledger as committed.
    /// # Errors
    /// Returns `LedgerUpdateError::Cache` for locking/load/persistence faults, or `LedgerUpdateError::Body`.
    pub fn update_reclaiming<T, E>(
        &self,
        empty_quota: Quota,
        objects: &FsObjectStore,
        body: impl FnOnce(&mut Lifecycle) -> Result<(T, bool), E>,
    ) -> Result<(T, Lifecycle), LedgerUpdateError<E>> {
        let _guard = lock::LockGuard::acquire(&self.root).map_err(LedgerUpdateError::Cache)?;
        let loaded = self
            .load_locked(empty_quota)
            .map_err(LedgerUpdateError::Cache)?;
        if let Some(error) = loaded.fault {
            return Err(LedgerUpdateError::Cache(error));
        }
        let mut life = loaded.lifecycle;
        let reaped = self.reap_dead_leases(&mut life);
        let (value, commit) = body(&mut life).map_err(LedgerUpdateError::Body)?;
        if commit || !reaped.is_empty() {
            self.commit_reclaiming(&mut life, objects, reaped)
                .map_err(LedgerUpdateError::Cache)?;
        }
        Ok((value, life))
    }

    /// Takes the proof-of-life lock for `lease`. Call it while the publish section is still open, so the file
    /// is locked before any other section can look for it.
    /// # Errors
    /// Refuses symlinks, non-regular or unsafe lock files, and creation/locking failures.
    /// Keep the hold alive until the corresponding ledger lease has been released.
    pub fn hold_lease(&self, lease: LeaseId) -> Result<LeaseHold, CacheError> {
        let path = self.root.join(lease_lock_name(lease));
        let file = open_lease(&path, true)?;
        file.try_lock().map_err(|_| CacheError::LedgerIo {
            reason: "lease lock",
        })?;
        Ok(LeaseHold { _lock: file, path })
    }

    /// Marks leases with proof their owner died. Missing, unreadable, and locked files preserve their leases.
    fn reap_dead_leases(&self, life: &mut Lifecycle) -> Vec<DeadLease> {
        let mut reaped = Vec::new();
        for id in life.open_leases() {
            if let Some(evidence) = DeadLease::probe(self.root.join(lease_lock_name(id)))
                && life.reader_owner_dead(id).is_ok()
            {
                reaped.push(evidence);
            }
        }
        reaped
    }

    fn commit_reclaiming(
        &self,
        life: &mut Lifecycle,
        objects: &FsObjectStore,
        mut reaped: Vec<DeadLease>,
    ) -> Result<(), CacheError> {
        if life.begin_reclaim() > 0 {
            self.store_locked(life)?;
            // The release is durable even if the final commit leaves the unlink charges pending.
            reaped.drain(..).for_each(DeadLease::remove);
        }
        let prior = life.clone();
        let ids = life.finish_reclaim();
        for id in ids {
            if objects.unlink_object(id).is_err() {
                life.restore_pending_charge(&prior, id)?;
            }
        }
        self.store_locked(life)?;
        reaped.into_iter().for_each(DeadLease::remove);
        Ok(())
    }
}

/// Opens liveness evidence without following links or blocking on special files.
fn open_lease(path: &Path, create: bool) -> Result<File, CacheError> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(create)
        .create(create)
        .truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(super::no_follow() | rustix::fs::OFlags::NONBLOCK.bits() as i32);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000); // FILE_FLAG_OPEN_REPARSE_POINT
    }
    let file = options.open(path).map_err(|_| CacheError::LedgerIo {
        reason: "lease lock",
    })?;
    let meta = file.metadata().map_err(|_| CacheError::LedgerIo {
        reason: "lease lock metadata",
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
    Ok(file)
}
