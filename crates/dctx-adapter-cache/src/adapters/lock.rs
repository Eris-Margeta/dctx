//! Advisory cross-process lock for one cache root.
//! Implements review F05: reload, publish and commit take the same lock.
//! Depends on std `File` locks. [`super::FsLedgerStore`] holds the guard for the critical section.
//! Released when the guard drops or the process dies. A writer that never locks can still race.

use crate::error::CacheError;
use std::fs::{File, OpenOptions};
use std::path::Path;

/// Lock file inside the cache root. Load skips this name when it lists orphans.
pub(super) const LOCK_FILE: &str = "ledger.lock";

/// Id mixed into unique temp names so two processes do not share a stage file.
pub(super) fn owner_id() -> u32 {
    std::process::id()
}

/// Holds an exclusive advisory lock on [`LOCK_FILE`].
pub(super) struct LockGuard {
    file: File,
}

impl LockGuard {
    /// Creates the lock file if needed and takes a non-blocking exclusive lock.
    pub(super) fn acquire(root: &Path) -> Result<Self, CacheError> {
        let path = root.join(LOCK_FILE);
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(&path).map_err(|_| CacheError::LedgerIo { reason: "lock" })?;
        match file.try_lock() {
            Ok(()) => Ok(Self { file }),
            Err(std::fs::TryLockError::WouldBlock) => Err(CacheError::Busy),
            Err(std::fs::TryLockError::Error(_)) => Err(CacheError::LedgerIo { reason: "lock" }),
        }
    }
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}
