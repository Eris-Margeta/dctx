//! Filesystem port the publication adapter drives.
//! Implements SEGMENTS-v1 temp write, sync, immutable rename, and atomic pointer replace.
//! Depends on [`crate::domain::Boundary`] and [`crate::domain::StoreError`]. [`crate::adapters`] implement it.
//! `checkpoint` is the crash cut. A real disk treats it as a no-op.

use crate::domain::{Boundary, StoreError};

/// What a durable publish is allowed to assume about this adapter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FsCapabilities {
    /// File and directory sync are available.
    pub sync: bool,
    /// Replace of an existing name is atomic.
    pub atomic_replace: bool,
    /// The writer lock is released by the operating system when the process dies.
    pub os_lock: bool,
}

/// Byte storage with explicit durability steps.
pub trait StoreFs: Send + Sync {
    /// Durability features of this adapter.
    fn capabilities(&self) -> FsCapabilities;

    /// Records a publication cut. Fault injection may crash here.
    fn checkpoint(&self, boundary: Boundary) -> Result<(), StoreError>;

    /// Takes the per-root writer lock, or [`StoreError::Busy`].
    fn try_lock(&self, root: &str) -> Result<(), StoreError>;

    /// Drops a lock this process holds. A stuck mutation stays stuck.
    fn unlock(&self, root: &str) -> Result<(), StoreError>;

    /// True when this root's lock is held or stuck.
    fn lock_held(&self, root: &str) -> bool;

    /// Roots whose writer lock is held or stuck.
    fn held_roots(&self) -> Vec<String>;

    /// Creates an empty file. Fails when the name exists.
    fn create(&self, path: &str) -> Result<(), StoreError>;

    /// Replaces the file's bytes without syncing them.
    fn write(&self, path: &str, bytes: &[u8]) -> Result<(), StoreError>;

    /// Flushes a userspace buffer. This is not durability.
    fn flush(&self, path: &str) -> Result<(), StoreError>;

    /// Makes file bytes durable.
    fn sync_file(&self, path: &str) -> Result<(), StoreError>;

    /// Makes directory entries durable.
    fn sync_dir(&self) -> Result<(), StoreError>;

    /// Renames onto a name that must not exist.
    fn rename_new(&self, from: &str, to: &str) -> Result<(), StoreError>;

    /// Atomically replaces `to` when the profile allows it.
    fn rename_replace(&self, from: &str, to: &str) -> Result<(), StoreError>;

    /// Reads the visible bytes of a file.
    fn read(&self, path: &str) -> Result<Vec<u8>, StoreError>;

    /// Whether the visible directory contains `path`.
    fn exists(&self, path: &str) -> Result<bool, StoreError>;

    /// Visible byte length.
    fn len(&self, path: &str) -> Result<u64, StoreError>;

    /// Removes a name. Does not truncate a live inode.
    fn unlink(&self, path: &str) -> Result<(), StoreError>;

    /// Visible file names with this prefix, sorted.
    fn list_prefix(&self, prefix: &str) -> Result<Vec<String>, StoreError>;
}
