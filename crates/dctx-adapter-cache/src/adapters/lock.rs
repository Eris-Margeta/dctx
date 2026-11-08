// SPDX-License-Identifier: Apache-2.0
//! Advisory cross-process lock for one cache root.
//! Implements review F05 and P-C12: reload, publish and commit take the same lock; readers share it.
//! Depends on std `File` locks. [`super::FsLedgerStore`] holds the guard for the critical section.
//! Released when the guard drops or the process dies. A writer that never locks can still race.
//! A refused try is retried for a bounded wait, so a short critical section never surfaces as `busy`.

use crate::error::CacheError;
use std::fs::{File, OpenOptions};
use std::path::Path;
use std::time::{Duration, Instant};

/// Lock file inside the cache root. Load skips this name when it lists orphans.
pub(super) const LOCK_FILE: &str = "ledger.lock";
/// Serializes acquisition only: a waiting writer stops new readers from overtaking it.
pub(super) const ENTRY_FILE: &str = "ledger.entry.lock";

/// How long a try is repeated before the cache reports `busy`. A publish holds the lock for well under a second.
pub(super) const LOCK_WAIT: Duration = Duration::from_secs(20);

/// Id mixed into unique temp names so two processes do not share a stage file.
pub(super) fn owner_id() -> u32 {
    std::process::id()
}

/// Holds an advisory lock on [`LOCK_FILE`]: shared for readers, exclusive for the publish and commit path.
pub(super) struct LockGuard {
    file: File,
}

impl LockGuard {
    /// Creates the lock file if needed and takes the exclusive lock, waiting up to [`LOCK_WAIT`].
    pub(super) fn acquire(root: &Path) -> Result<Self, CacheError> {
        Self::acquire_within(root, false, LOCK_WAIT)
    }

    /// Takes a lock that other readers may hold at the same time, waiting up to [`LOCK_WAIT`].
    pub(super) fn acquire_shared(root: &Path) -> Result<Self, CacheError> {
        Self::acquire_within(root, true, LOCK_WAIT)
    }

    pub(super) fn acquire_within(
        root: &Path,
        shared: bool,
        wait: Duration,
    ) -> Result<Self, CacheError> {
        let started = Instant::now();
        // Readers release the entry lock once they hold their shared ledger lock,
        // so their bodies still run together. A writer keeps the entry lock while
        // existing readers finish, preventing a stream of readers from starving it.
        let entry = Self::lock_file(&root.join(ENTRY_FILE), false, started, wait)?;
        let ledger = Self::lock_file(&root.join(LOCK_FILE), shared, started, wait);
        drop(entry);
        ledger
    }

    fn lock_file(
        path: &Path,
        shared: bool,
        started: Instant,
        wait: Duration,
    ) -> Result<Self, CacheError> {
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(super::no_follow());
        }
        let file = options
            .open(path)
            .map_err(|_| CacheError::LedgerIo { reason: "lock" })?;
        let metadata = file.metadata().map_err(|_| CacheError::LedgerIo {
            reason: "lock metadata",
        })?;
        if !metadata.is_file() {
            return Err(CacheError::OutsideCacheRoot);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if metadata.uid() != rustix::process::geteuid().as_raw() || metadata.mode() & 0o022 != 0
            {
                return Err(CacheError::OutsideCacheRoot);
            }
        }
        let mut pause = Duration::from_millis(1);
        loop {
            let tried = if shared {
                file.try_lock_shared()
            } else {
                file.try_lock()
            };
            match tried {
                Ok(()) => return Ok(Self { file }),
                Err(std::fs::TryLockError::WouldBlock) => {
                    if started.elapsed() >= wait {
                        return Err(CacheError::Busy);
                    }
                    std::thread::sleep(pause);
                    pause = (pause * 2).min(Duration::from_millis(25));
                }
                Err(std::fs::TryLockError::Error(_)) => {
                    return Err(CacheError::LedgerIo { reason: "lock" });
                }
            }
        }
    }
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn shared_locks_coexist_and_an_exclusive_one_waits_then_reports_busy() {
        let dir = tempfile::tempdir().expect("temp");
        let a = LockGuard::acquire_within(dir.path(), true, Duration::from_millis(20))
            .expect("first reader");
        let b = LockGuard::acquire_within(dir.path(), true, Duration::from_millis(20))
            .expect("second reader");
        let started = Instant::now();
        let refused = LockGuard::acquire_within(dir.path(), false, Duration::from_millis(60));
        assert!(matches!(refused, Err(CacheError::Busy)));
        assert!(
            started.elapsed() >= Duration::from_millis(60),
            "the writer waited before giving up"
        );
        drop((a, b));
        let writer = LockGuard::acquire_within(dir.path(), false, Duration::from_millis(20))
            .expect("writer");
        let reader = LockGuard::acquire_within(dir.path(), true, Duration::from_millis(40));
        assert!(matches!(reader, Err(CacheError::Busy)));
        drop(writer);
        LockGuard::acquire_within(dir.path(), true, Duration::from_millis(20))
            .expect("reader again");
    }
}
