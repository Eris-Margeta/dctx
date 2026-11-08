// SPDX-License-Identifier: Apache-2.0
//! Filesystem ledger and the in-memory `Cache` port.
//! Implements CACHE-v1 publication: a unique temp file, sync, then rename, under a revision check.
//! Depends on [`crate::Lifecycle`] and std filesystem calls. Load reports orphans and does not delete them.
//! Object bytes live in [`FsObjectStore`]. A symlink is never a reason to touch a path outside the cache root.

mod cache;
mod coordinate;
mod lock;
mod orphans;
mod publish;
mod reclaim;

use crate::domain::{LEDGER_MAX_BYTES, Lifecycle, ObjectId, Quota};
use crate::error::CacheError;
use crate::ports::{LedgerStore, LoadedLedger};
#[cfg(unix)]
use std::fs::File;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

pub use cache::FsObjectStore;
pub use coordinate::auto_clean;
pub use dctx_ports::Cache;
pub use reclaim::LeaseHold;

/// Committed ledger name inside the cache root.
pub const LEDGER_FILE: &str = "ledger.v1";
/// Historical fixed temp name. Stages now use a unique name so one store cannot commit another's bytes.
pub const LEDGER_TEMP: &str = "ledger.v1.tmp";

/// Revision this store last observed on the committed ledger.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Observed {
    /// `load` or `store` has not looked at the committed file yet.
    Unknown,
    /// The committed file was absent.
    Absent,
    /// The committed file was corrupt, an unknown version, or not a regular file.
    Unreadable,
    /// Logical revision of the committed image.
    Revision(u64),
}

#[derive(Clone, Debug)]
struct Staged {
    path: PathBuf,
    revision: u64,
}

static TEMP_SEQ: AtomicU64 = AtomicU64::new(1);

/// Failure of [`FsLedgerStore::update`].
/// `Body` is the edit's own error. The committed ledger is unchanged in that case.
#[derive(Debug)]
pub enum LedgerUpdateError<E> {
    /// The lock, the load, a corrupt image, or the store refused the write.
    Cache(CacheError),
    /// `body` refused the edit.
    Body(E),
}

impl<E: std::fmt::Display> std::fmt::Display for LedgerUpdateError<E> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cache(error) => write!(formatter, "{error}"),
            Self::Body(error) => write!(formatter, "{error}"),
        }
    }
}

impl<E: std::fmt::Debug + std::fmt::Display> std::error::Error for LedgerUpdateError<E> {}

/// Versioned ledger file for one cache root.
#[derive(Debug)]
pub struct FsLedgerStore {
    root: PathBuf,
    observed: Mutex<Observed>,
    staged: Mutex<Option<Staged>>,
}

impl FsLedgerStore {
    /// `root` must already be a real directory. A symlink root is refused.
    /// # Errors
    /// Refuses an unsafe/non-directory cache root or filesystem metadata failures.
    pub fn open(root: &Path) -> Result<Self, CacheError> {
        Ok(Self {
            root: real_root(root)?,
            observed: Mutex::new(Observed::Unknown),
            staged: Mutex::new(None),
        })
    }

    /// Holds the cache lock for `body`. `store` and `load` take the same lock.
    pub(super) fn exclusive<T>(
        &self,
        body: impl FnOnce() -> Result<T, CacheError>,
    ) -> Result<T, CacheError> {
        let _guard = lock::LockGuard::acquire(&self.root)?;
        body()
    }

    /// Holds the shared cache lock for `body`. Other readers run beside it; a publish waits for it to end.
    pub(super) fn shared<T>(
        &self,
        body: impl FnOnce() -> Result<T, CacheError>,
    ) -> Result<T, CacheError> {
        let _guard = lock::LockGuard::acquire_shared(&self.root)?;
        body()
    }

    /// Loads the ledger and runs `body` on it while the shared lock is held, so no publish can reclaim
    /// an object file `body` reads. Readers share the lock; only a publish or commit excludes them.
    /// # Errors
    /// Returns `LedgerUpdateError::Cache` for locking/load faults or a corrupt image, or `LedgerUpdateError::Body`.
    pub fn read<T, E>(
        &self,
        empty_quota: Quota,
        body: impl FnOnce(&Lifecycle) -> Result<T, E>,
    ) -> Result<T, LedgerUpdateError<E>> {
        let _guard =
            lock::LockGuard::acquire_shared(&self.root).map_err(LedgerUpdateError::Cache)?;
        let loaded = self
            .load_locked(empty_quota)
            .map_err(LedgerUpdateError::Cache)?;
        if let Some(error) = loaded.fault {
            return Err(LedgerUpdateError::Cache(error));
        }
        body(&loaded.lifecycle).map_err(LedgerUpdateError::Body)
    }

    /// Writes and syncs a unique temp file. The committed name is unchanged until [`Self::commit`].
    /// # Errors
    /// Propagates lock acquisition and ledger encoding/staging failures.
    pub fn stage(&self, ledger: &Lifecycle) -> Result<(), CacheError> {
        self.exclusive(|| self.stage_locked(ledger))
    }

    /// Renames this store's temp file over the committed ledger when the revision still matches.
    /// # Errors
    /// Propagates lock acquisition and durable ledger commit failures.
    pub fn commit(&self) -> Result<(), CacheError> {
        self.exclusive(|| self.commit_locked())
    }

    fn stage_locked(&self, ledger: &Lifecycle) -> Result<(), CacheError> {
        let bytes = ledger.encode_ledger()?;
        let path = self.fresh_temp()?;
        write_exclusive(&path, &bytes)?;
        let previous = {
            let mut slot = self
                .staged
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            slot.replace(Staged {
                path,
                revision: ledger.logical_revision(),
            })
        };
        if let Some(previous) = previous {
            remove_regular_file(&previous.path);
        }
        Ok(())
    }

    fn commit_locked(&self) -> Result<(), CacheError> {
        let Some(staged) = self.staged_slot() else {
            return Err(CacheError::LedgerIo {
                reason: "missing temp",
            });
        };
        self.ensure_current()?;
        let to = self.root.join(LEDGER_FILE);
        match fs::symlink_metadata(&staged.path) {
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Err(CacheError::LedgerIo {
                    reason: "missing temp",
                });
            }
            Err(_) => return Err(CacheError::LedgerIo { reason: "metadata" }),
            Ok(meta) if meta.file_type().is_symlink() || !meta.is_file() => {
                return Err(CacheError::OutsideCacheRoot);
            }
            Ok(_) => {}
        }
        if let Ok(meta) = fs::symlink_metadata(&to)
            && meta.file_type().is_symlink()
        {
            return Err(CacheError::OutsideCacheRoot);
        }
        fs::rename(&staged.path, &to).map_err(|_| CacheError::LedgerIo { reason: "rename" })?;
        sync_dir(&self.root)?;
        self.set_observed(Observed::Revision(staged.revision));
        self.clear_staged();
        Ok(())
    }

    /// Stores `ledger` while the caller already holds [`Self::exclusive`].
    pub(super) fn store_locked(&self, ledger: &Lifecycle) -> Result<(), CacheError> {
        self.ensure_current()?;
        self.unlink_evicted(ledger)?;
        self.stage_locked(ledger)?;
        self.commit_locked()
    }

    /// Unlinks the files of objects admission evicted. They left the ledger, so their files go before it commits:
    /// a crash in between leaves a charge for a missing file, never a file without a charge.
    fn unlink_evicted(&self, ledger: &Lifecycle) -> Result<(), CacheError> {
        let ids = ledger.evicted_ids();
        if ids.is_empty() {
            return Ok(());
        }
        let objects = FsObjectStore::open(&self.root)?;
        for id in ids {
            crate::ports::ObjectStore::unlink_object(&objects, *id)?;
        }
        Ok(())
    }

    /// Loads, runs `body`, and stores when `body` commits, all under the cache lock.
    /// `body` returns `(value, commit)`. A false `commit` leaves the committed file unchanged.
    /// Public load and store each take this try-lock, and it is not reentrant, so a caller cannot span them.
    /// A different store that observed an older revision still fails with `stale ledger`.
    /// # Errors
    /// Returns `LedgerUpdateError::Cache` for locking/load/persistence faults, or `LedgerUpdateError::Body` for a rejected update.
    pub fn update<T, E>(
        &self,
        empty_quota: Quota,
        body: impl FnOnce(&mut Lifecycle) -> Result<(T, bool), E>,
    ) -> Result<T, LedgerUpdateError<E>> {
        let _guard = lock::LockGuard::acquire(&self.root).map_err(LedgerUpdateError::Cache)?;
        let loaded = self
            .load_locked(empty_quota)
            .map_err(LedgerUpdateError::Cache)?;
        if let Some(error) = loaded.fault {
            return Err(LedgerUpdateError::Cache(error));
        }
        let mut life = loaded.lifecycle;
        let (value, commit) = body(&mut life).map_err(LedgerUpdateError::Body)?;
        if commit {
            self.store_locked(&life).map_err(LedgerUpdateError::Cache)?;
        }
        Ok(value)
    }

    fn load_locked(&self, empty_quota: Quota) -> Result<LoadedLedger, CacheError> {
        let path = self.root.join(LEDGER_FILE);
        match read_committed(&path)? {
            LedgerRead::Absent => {
                self.set_observed(Observed::Absent);
                self.empty_load(empty_quota, None)
            }
            LedgerRead::Rejected(error) => {
                self.set_observed(Observed::Unreadable);
                self.empty_load(empty_quota, Some(error))
            }
            LedgerRead::Bytes(bytes) => match Lifecycle::decode_ledger(&bytes) {
                Ok(lifecycle) => {
                    self.set_observed(Observed::Revision(lifecycle.logical_revision()));
                    let known = lifecycle
                        .object_ids()
                        .into_iter()
                        .map(ObjectId::raw)
                        .collect();
                    Ok(LoadedLedger {
                        lifecycle,
                        fault: None,
                        orphans: orphans::list_orphans(&self.root, &known)?,
                    })
                }
                Err(
                    error @ (CacheError::CorruptLedger { .. }
                    | CacheError::UnknownLedgerVersion { .. }),
                ) => {
                    self.set_observed(Observed::Unreadable);
                    self.empty_load(empty_quota, Some(error))
                }
                Err(error) => Err(error),
            },
        }
    }

    fn empty_load(
        &self,
        empty_quota: Quota,
        fault: Option<CacheError>,
    ) -> Result<LoadedLedger, CacheError> {
        Ok(LoadedLedger {
            lifecycle: Lifecycle::new(empty_quota)?,
            fault,
            orphans: orphans::list_orphans(&self.root, &std::collections::BTreeSet::new())?,
        })
    }

    fn ensure_current(&self) -> Result<(), CacheError> {
        if revision_matches(self.observed(), self.disk_token()?) {
            Ok(())
        } else {
            Err(CacheError::InvalidRequest {
                reason: "stale ledger",
            })
        }
    }

    fn disk_token(&self) -> Result<Observed, CacheError> {
        match read_committed(&self.root.join(LEDGER_FILE))? {
            LedgerRead::Absent => Ok(Observed::Absent),
            LedgerRead::Rejected(_) => Ok(Observed::Unreadable),
            LedgerRead::Bytes(bytes) => match Lifecycle::decode_ledger(&bytes) {
                Ok(lifecycle) => Ok(Observed::Revision(lifecycle.logical_revision())),
                Err(CacheError::CorruptLedger { .. } | CacheError::UnknownLedgerVersion { .. }) => {
                    Ok(Observed::Unreadable)
                }
                Err(error) => Err(error),
            },
        }
    }

    fn fresh_temp(&self) -> Result<PathBuf, CacheError> {
        for _ in 0..8 {
            let path = self.root.join(format!(
                "ledger.{}.{}.tmp",
                lock::owner_id(),
                TEMP_SEQ.fetch_add(1, Ordering::Relaxed)
            ));
            if !path.starts_with(&self.root) {
                return Err(CacheError::OutsideCacheRoot);
            }
            match fs::symlink_metadata(&path) {
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(path),
                Err(_) => return Err(CacheError::LedgerIo { reason: "metadata" }),
                Ok(meta) if meta.file_type().is_symlink() || meta.is_dir() => {
                    return Err(CacheError::OutsideCacheRoot);
                }
                Ok(_) => {}
            }
        }
        Err(CacheError::LedgerIo { reason: "create" })
    }

    fn staged_slot(&self) -> Option<Staged> {
        self.staged
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    #[cfg(test)]
    fn staged_path(&self) -> Option<PathBuf> {
        self.staged_slot().map(|staged| staged.path)
    }

    fn clear_staged(&self) {
        *self
            .staged
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    }

    fn observed(&self) -> Observed {
        *self
            .observed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn set_observed(&self, observed: Observed) {
        *self
            .observed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = observed;
    }
}

fn revision_matches(observed: Observed, disk: Observed) -> bool {
    match observed {
        Observed::Unknown => matches!(disk, Observed::Absent),
        other => other == disk,
    }
}

impl LedgerStore for FsLedgerStore {
    fn load(&self, empty_quota: Quota) -> Result<LoadedLedger, CacheError> {
        self.shared(|| self.load_locked(empty_quota))
    }

    fn store(&self, ledger: &Lifecycle) -> Result<(), CacheError> {
        self.exclusive(|| self.store_locked(ledger))
    }
}

pub(super) fn real_root(root: &Path) -> Result<PathBuf, CacheError> {
    if !root.is_absolute() {
        return Err(CacheError::InvalidRequest {
            reason: "cache root must be absolute",
        });
    }
    if root
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(CacheError::OutsideCacheRoot);
    }
    let meta =
        fs::symlink_metadata(root).map_err(|_| CacheError::LedgerIo { reason: "metadata" })?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(CacheError::InvalidRequest {
            reason: "cache root is not a directory",
        });
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if meta.uid() != rustix::process::geteuid().as_raw() || meta.mode() & 0o022 != 0 {
            return Err(CacheError::InvalidRequest {
                reason: "cache root must be user-owned and not writable by other users",
            });
        }
        // An untrusted writable ancestor could replace the entire cache directory.
        // Root-owned sticky directories (for example the system temporary directory) protect owned children.
        // A symlinked ancestor (macOS: /var -> private/var, /tmp -> private/tmp) is accepted only when it is
        // owned by root or the current user, its own parent passes these checks, and every real directory
        // it resolves to passes them as well.
        let current = rustix::process::geteuid().as_raw();
        let Some(parent) = root.parent() else {
            return Ok(root.to_path_buf());
        };
        if check_ancestors(parent, current, true)? {
            let resolved = fs::canonicalize(parent).map_err(|_| CacheError::LedgerIo {
                reason: "cache ancestor metadata",
            })?;
            check_ancestors(&resolved, current, false)?;
        }
    }
    Ok(root.to_path_buf())
}

/// Checks `path` and its ancestors. Returns whether a (trusted-owner) symlink was seen.
#[cfg(unix)]
fn check_ancestors(path: &Path, current: u32, allow_links: bool) -> Result<bool, CacheError> {
    use std::os::unix::fs::MetadataExt;
    let mut linked = false;
    for ancestor in path.ancestors() {
        let meta = fs::symlink_metadata(ancestor).map_err(|_| CacheError::LedgerIo {
            reason: "cache ancestor metadata",
        })?;
        if meta.file_type().is_symlink() {
            if !allow_links || (meta.uid() != 0 && meta.uid() != current) {
                return Err(CacheError::OutsideCacheRoot);
            }
            linked = true;
            continue;
        }
        if !meta.is_dir() {
            return Err(CacheError::OutsideCacheRoot);
        }
        if !trusted_ancestor(meta.uid(), meta.mode(), current) {
            let read_only = rustix::fs::statvfs(ancestor)
                .map(|status| {
                    status
                        .f_flag
                        .contains(rustix::fs::StatVfsMountFlags::RDONLY)
                })
                .unwrap_or(false);
            if !read_only {
                return Err(CacheError::OutsideCacheRoot);
            }
        }
    }
    Ok(linked)
}

#[cfg(unix)]
fn trusted_ancestor(owner: u32, mode: u32, current: u32) -> bool {
    (owner == 0 || owner == current) && (mode & 0o022 == 0 || (owner == 0 && mode & 0o1000 != 0))
}

pub(super) fn locate(root: &Path, relative: &Path) -> Result<Option<PathBuf>, CacheError> {
    if relative.as_os_str().is_empty() || relative.is_absolute() {
        return Err(CacheError::OutsideCacheRoot);
    }
    let mut cursor = root.to_path_buf();
    let mut saw_name = false;
    for component in relative.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(name) => {
                // Apply the engine's portable name rules before the OS can normalize a component.
                if name
                    .to_str()
                    .is_none_or(|text| dctx_core::ids::RelativePath::parse(text).is_err())
                {
                    return Err(CacheError::OutsideCacheRoot);
                }
                match fs::symlink_metadata(&cursor) {
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                    Err(_) => return Err(CacheError::LedgerIo { reason: "metadata" }),
                    Ok(meta) if meta.file_type().is_symlink() || !meta.is_dir() => {
                        return Err(CacheError::OutsideCacheRoot);
                    }
                    Ok(_) => {}
                }
                cursor.push(name);
                if !cursor.starts_with(root) {
                    return Err(CacheError::OutsideCacheRoot);
                }
                saw_name = true;
            }
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(CacheError::OutsideCacheRoot);
            }
        }
    }
    if !saw_name || cursor == root {
        return Err(CacheError::OutsideCacheRoot);
    }
    Ok(Some(cursor))
}

pub(super) fn ensure_dir(path: &Path) -> Result<(), CacheError> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => return Err(CacheError::OutsideCacheRoot),
        Ok(meta) if meta.is_dir() => {
            real_root(path)?;
            return Ok(());
        }
        Ok(_) => {
            return Err(CacheError::InvalidRequest {
                reason: "cache object path is not a directory",
            });
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(CacheError::LedgerIo { reason: "metadata" }),
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = fs::DirBuilder::new();
        builder.mode(0o700);
        builder.create(path).map_err(|_| CacheError::LedgerIo {
            reason: "create directory",
        })?;
    }
    #[cfg(not(unix))]
    {
        fs::create_dir(path).map_err(|_| CacheError::LedgerIo {
            reason: "create directory",
        })?;
    }
    Ok(())
}

pub(super) fn write_exclusive(path: &Path, bytes: &[u8]) -> Result<(), CacheError> {
    if let Ok(meta) = fs::symlink_metadata(path) {
        if meta.file_type().is_symlink() || meta.is_dir() {
            return Err(CacheError::OutsideCacheRoot);
        }
        return Err(CacheError::LedgerIo { reason: "create" });
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
        options.custom_flags(no_follow());
    }
    let mut file = options
        .open(path)
        .map_err(|_| CacheError::LedgerIo { reason: "create" })?;
    file.write_all(bytes)
        .map_err(|_| CacheError::LedgerIo { reason: "write" })?;
    file.sync_all()
        .map_err(|_| CacheError::LedgerIo { reason: "sync" })?;
    Ok(())
}

fn remove_regular_file(path: &Path) {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() => {
            let _ = fs::remove_file(path);
        }
        _ => {}
    }
}

pub(super) fn sync_dir(path: &Path) -> Result<(), CacheError> {
    #[cfg(unix)]
    {
        let dir = File::open(path).map_err(|_| CacheError::LedgerIo {
            reason: "directory sync",
        })?;
        dir.sync_all().map_err(|_| CacheError::LedgerIo {
            reason: "directory sync",
        })?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

enum LedgerRead {
    Absent,
    Bytes(Vec<u8>),
    Rejected(CacheError),
}

fn read_committed(path: &Path) -> Result<LedgerRead, CacheError> {
    let Ok(limit) = u64::try_from(LEDGER_MAX_BYTES) else {
        return Ok(LedgerRead::Rejected(CacheError::CorruptLedger {
            reason: "ledger is too large",
        }));
    };
    match fs::symlink_metadata(path) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(LedgerRead::Absent),
        Err(_) => return Err(CacheError::LedgerIo { reason: "metadata" }),
        Ok(meta) if meta.file_type().is_symlink() => {
            return Ok(LedgerRead::Rejected(CacheError::CorruptLedger {
                reason: "ledger symlink",
            }));
        }
        Ok(meta) if !meta.is_file() => {
            return Ok(LedgerRead::Rejected(CacheError::CorruptLedger {
                reason: "ledger magic",
            }));
        }
        Ok(meta) if meta.len() > limit => {
            return Ok(LedgerRead::Rejected(CacheError::CorruptLedger {
                reason: "ledger is too large",
            }));
        }
        Ok(_) => {}
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(no_follow());
    }
    let file = options
        .open(path)
        .map_err(|_| CacheError::LedgerIo { reason: "read" })?;
    let metadata = file.metadata().map_err(|_| CacheError::LedgerIo {
        reason: "ledger metadata",
    })?;
    if !metadata.is_file() {
        return Err(CacheError::OutsideCacheRoot);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.uid() != rustix::process::geteuid().as_raw() || metadata.mode() & 0o022 != 0 {
            return Ok(LedgerRead::Rejected(CacheError::CorruptLedger {
                reason: "unsafe ledger permissions",
            }));
        }
    }
    let mut limited = file.take(limit.saturating_add(1));
    let mut bytes = Vec::new();
    limited
        .read_to_end(&mut bytes)
        .map_err(|_| CacheError::LedgerIo { reason: "read" })?;
    let Ok(len) = u64::try_from(bytes.len()) else {
        return Ok(LedgerRead::Rejected(CacheError::CorruptLedger {
            reason: "ledger is too large",
        }));
    };
    if len > limit {
        return Ok(LedgerRead::Rejected(CacheError::CorruptLedger {
            reason: "ledger is too large",
        }));
    }
    Ok(LedgerRead::Bytes(bytes))
}

#[cfg(unix)]
pub(super) const fn no_follow() -> i32 {
    rustix::fs::OFlags::NOFOLLOW.bits() as i32
}

#[cfg(test)]
mod tests;
