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

enum Owner {
    Alive,
    Dead,
    Unknown,
}

fn probe(root: &Path, id: LeaseId) -> Owner {
    let path = root.join(lease_lock_name(id));
    let Ok(file) = OpenOptions::new().read(true).open(&path) else {
        return Owner::Unknown;
    };
    match file.try_lock() {
        Ok(()) => {
            let _ = file.unlock();
            Owner::Dead
        }
        Err(std::fs::TryLockError::WouldBlock) => Owner::Alive,
        Err(std::fs::TryLockError::Error(_)) => Owner::Unknown,
    }
}

impl FsLedgerStore {
    /// Loads the ledger fresh under the exclusive lock, runs `body`, and when `body` commits, reclaims what the edit
    /// superseded and stores. `body` returns `(value, commit)`; a false `commit` leaves the committed file unchanged.
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
        if commit || reaped {
            self.commit_reclaiming(&mut life, objects)
                .map_err(LedgerUpdateError::Cache)?;
        }
        Ok((value, life))
    }

    /// Takes the proof-of-life lock for `lease`. Call it while the publish section is still open, so the file
    /// is locked before any other section can look for it.
    /// # Errors
    /// Refuses a lock file that cannot be created or locked.
    pub fn hold_lease(&self, lease: LeaseId) -> Result<LeaseHold, CacheError> {
        let path = self.root.join(lease_lock_name(lease));
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(&path).map_err(|_| CacheError::LedgerIo {
            reason: "lease lock",
        })?;
        file.try_lock().map_err(|_| CacheError::LedgerIo {
            reason: "lease lock",
        })?;
        Ok(LeaseHold { _lock: file, path })
    }

    /// Marks the leases whose reader process is gone. A lease without a lock file predates this check and is kept.
    fn reap_dead_leases(&self, life: &mut Lifecycle) -> bool {
        let mut reaped = false;
        for id in life.open_leases() {
            if matches!(probe(&self.root, id), Owner::Dead) && life.reader_owner_dead(id).is_ok() {
                let _ = fs::remove_file(self.root.join(lease_lock_name(id)));
                reaped = true;
            }
        }
        reaped
    }

    fn commit_reclaiming(
        &self,
        life: &mut Lifecycle,
        objects: &FsObjectStore,
    ) -> Result<(), CacheError> {
        if life.begin_reclaim() > 0 {
            self.store_locked(life)?;
        }
        let prior = life.clone();
        let ids = life.finish_reclaim();
        for id in ids {
            if objects.unlink_object(id).is_err() {
                life.restore_pending_charge(&prior, id)?;
            }
        }
        self.store_locked(life)
    }
}
