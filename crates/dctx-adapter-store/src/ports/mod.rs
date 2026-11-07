// SPDX-License-Identifier: Apache-2.0
//! Filesystem port the publication adapter drives.
//! Implements SEGMENTS-v1 temp write, sync, immutable rename, and atomic pointer replace.
//! Depends on [`crate::domain::Boundary`] and [`crate::domain::StoreError`]. [`crate::adapters`] implement it.
//! `checkpoint` is the crash cut. A real disk treats it as a no-op. Std confinement lives below the trait.

use crate::domain::{Boundary, POINTER_LEN, StoreError};

/// Largest object a writer behind this store can leave on disk, in bytes.
/// The segment codec ceiling and the map index 512 MiB admission cap both apply; the greater one wins.
/// A read bound below either refuses a generation that writer can publish.
pub(crate) const OBJECT_MAX: u64 = {
    const ADMISSION: u64 = 512 * 1024 * 1024;
    const CODEC: u64 = dctx_map_segment::DecoderLimits::standard().max_segment_bytes;
    if ADMISSION > CODEC { ADMISSION } else { CODEC }
};

/// Largest buffer a read of `path` may store. Pointer names are fixed; other objects use [`OBJECT_MAX`].
pub(crate) fn read_ceiling(path: &str) -> u64 {
    let name = path.rsplit('/').next().unwrap_or(path);
    if matches!(
        name,
        "current" | "previous" | "pointer.partial" | "previous.partial" | "recover-pointer.partial"
    ) {
        POINTER_LEN
    } else {
        OBJECT_MAX
    }
}

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
    ///
    /// # Errors
    /// Returns an injected crash, cancellation or adapter failure at the requested boundary.
    fn checkpoint(&self, boundary: Boundary) -> Result<(), StoreError>;

    /// Takes the per-root writer lock, or [`StoreError::Busy`].
    ///
    /// # Errors
    /// Returns [`StoreError::Busy`] for a held root or an adapter/invalid-root failure.
    fn try_lock(&self, root: &str) -> Result<(), StoreError>;

    /// Drops a lock this process holds. A stuck mutation stays stuck.
    ///
    /// # Errors
    /// Returns an invalid-root or adapter unlock failure.
    fn unlock(&self, root: &str) -> Result<(), StoreError>;

    /// True when this root's lock is held or stuck.
    fn lock_held(&self, root: &str) -> bool;

    /// Roots whose writer lock is held or stuck.
    fn held_roots(&self) -> Vec<String>;

    /// Creates an empty file. Fails when the name exists.
    ///
    /// # Errors
    /// Refuses invalid paths or existing names and propagates creation failures.
    fn create(&self, path: &str) -> Result<(), StoreError>;

    /// Replaces the file's bytes without syncing them.
    ///
    /// # Errors
    /// Refuses invalid or missing paths and propagates write failures, including disk-full errors.
    fn write(&self, path: &str, bytes: &[u8]) -> Result<(), StoreError>;

    /// Flushes a userspace buffer. This is not durability.
    ///
    /// # Errors
    /// Returns invalid-path, missing-file or flush failures.
    fn flush(&self, path: &str) -> Result<(), StoreError>;

    /// Makes file bytes durable.
    ///
    /// # Errors
    /// Returns invalid-path, missing-file, unsupported-durability or synchronization failures.
    fn sync_file(&self, path: &str) -> Result<(), StoreError>;

    /// Makes directory entries durable.
    ///
    /// # Errors
    /// Returns unsupported-durability or directory synchronization failures.
    fn sync_dir(&self) -> Result<(), StoreError>;

    /// Renames onto a name that must not exist.
    ///
    /// # Errors
    /// Refuses invalid paths or an occupied destination and propagates rename failures.
    fn rename_new(&self, from: &str, to: &str) -> Result<(), StoreError>;

    /// Atomically replaces `to` when the profile allows it.
    ///
    /// # Errors
    /// Refuses invalid paths, unsupported replacement or sharing violations and propagates rename failures.
    fn rename_replace(&self, from: &str, to: &str) -> Result<(), StoreError>;

    /// Reads the visible bytes of a file.
    /// An object above its pointer, manifest, or segment ceiling is refused before those bytes are stored,
    /// including when the file grows after its length was observed.
    ///
    /// # Errors
    /// Refuses invalid, missing, symlink or oversized objects and propagates read failures.
    fn read(&self, path: &str) -> Result<Vec<u8>, StoreError>;

    /// Whether the visible directory contains `path`.
    ///
    /// # Errors
    /// Returns path-validation or filesystem inspection failures.
    fn exists(&self, path: &str) -> Result<bool, StoreError>;

    /// Visible byte length.
    ///
    /// # Errors
    /// Returns path-validation, missing-file or metadata failures.
    fn len(&self, path: &str) -> Result<u64, StoreError>;

    /// Removes a name. Does not truncate a live inode.
    ///
    /// # Errors
    /// Returns invalid-path, missing-file, sharing or removal failures.
    fn unlink(&self, path: &str) -> Result<(), StoreError>;

    /// Visible file names with this prefix, sorted.
    ///
    /// # Errors
    /// Returns invalid-path, directory traversal or entry metadata failures.
    fn list_prefix(&self, prefix: &str) -> Result<Vec<String>, StoreError>;
}

pub(crate) fn io_err(err: std::io::Error) -> StoreError {
    if err.kind() == std::io::ErrorKind::StorageFull || err.raw_os_error() == Some(28) {
        return StoreError::DiskFull;
    }
    if err.raw_os_error() == Some(32) {
        return StoreError::SharingViolation;
    }
    StoreError::io(err)
}

/// Refuses a symlink component. A parent replaced by a symlink after this walk is a residual race.
pub(crate) fn confine_under(
    root: &std::path::Path,
    rel: &str,
) -> Result<std::path::PathBuf, StoreError> {
    crate::domain::check_rel(rel)?;
    let parts: Vec<&str> = rel.split('/').collect();
    let mut base = root.to_path_buf();
    for (index, part) in parts.iter().enumerate() {
        let next = base.join(part);
        let last = index + 1 == parts.len();
        match std::fs::symlink_metadata(&next) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(StoreError::InvalidRequest { reason: "path" });
            }
            Ok(meta) if meta.is_dir() => {
                base = path_inside(root, &next)?;
                if last {
                    return Ok(base);
                }
            }
            Ok(meta) if meta.is_file() && last => return path_inside(root, &next),
            Ok(_) => return Err(StoreError::InvalidRequest { reason: "path" }),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                let rest = if last {
                    next
                } else {
                    base.join(parts[index..].join("/"))
                };
                return Ok(rest);
            }
            Err(err) => return Err(io_err(err)),
        }
    }
    Ok(base)
}

pub(crate) fn ensure_dir(root: &std::path::Path, dir: &std::path::Path) -> Result<(), StoreError> {
    if !dir.starts_with(root) {
        return Err(StoreError::InvalidRequest { reason: "path" });
    }
    let rel = dir
        .strip_prefix(root)
        .map_err(|_| StoreError::InvalidRequest { reason: "path" })?;
    let mut cursor = root.to_path_buf();
    for part in rel.components() {
        cursor.push(part);
        match std::fs::symlink_metadata(&cursor) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(StoreError::InvalidRequest { reason: "path" });
            }
            Ok(meta) if meta.is_dir() => {}
            Ok(_) => return Err(StoreError::InvalidRequest { reason: "path" }),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => create_dir(&cursor)?,
            Err(err) => return Err(io_err(err)),
        }
    }
    path_inside(root, dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }
    Ok(())
}

pub(crate) fn open_under(
    root: &std::path::Path,
    rel: &str,
    write: bool,
    truncate: bool,
) -> Result<std::fs::File, StoreError> {
    let path = confine_under(root, rel)?;
    let meta = std::fs::symlink_metadata(&path).map_err(io_err)?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(StoreError::InvalidRequest { reason: "path" });
    }
    let mut options = std::fs::OpenOptions::new();
    if write {
        options.write(true).truncate(truncate);
    } else {
        options.read(true);
    }
    set_nofollow(&mut options);
    options.open(&path).map_err(io_err)
}

pub(crate) fn list_under(root: &std::path::Path, prefix: &str) -> Result<Vec<String>, StoreError> {
    let mut out = Vec::new();
    walk(root, root, prefix, &mut out)?;
    out.sort();
    Ok(out)
}

pub(crate) fn set_nofollow(options: &mut std::fs::OpenOptions) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(NOFOLLOW);
    }
    #[cfg(not(unix))]
    {
        let _ = options;
    }
}

fn create_dir(dir: &std::path::Path) -> Result<(), StoreError> {
    match std::fs::create_dir(dir) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
            let meta = std::fs::symlink_metadata(dir).map_err(io_err)?;
            if meta.file_type().is_symlink() || !meta.is_dir() {
                return Err(StoreError::InvalidRequest { reason: "path" });
            }
        }
        Err(err) => return Err(io_err(err)),
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }
    Ok(())
}

fn path_inside(
    root: &std::path::Path,
    path: &std::path::Path,
) -> Result<std::path::PathBuf, StoreError> {
    let got = std::fs::canonicalize(path).map_err(io_err)?;
    if got.starts_with(root) {
        Ok(got)
    } else {
        Err(StoreError::InvalidRequest { reason: "path" })
    }
}

fn walk(
    root: &std::path::Path,
    dir: &std::path::Path,
    prefix: &str,
    out: &mut Vec<String>,
) -> Result<(), StoreError> {
    if !dir.exists() {
        return Ok(());
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(io_err(err)),
    };
    for entry in entries {
        let entry = entry.map_err(io_err)?;
        let kind = entry.file_type().map_err(io_err)?;
        if kind.is_symlink() {
            return Err(StoreError::InvalidRequest { reason: "path" });
        }
        let path = entry.path();
        if kind.is_dir() {
            walk(root, &path, prefix, out)?;
            continue;
        }
        if !kind.is_file() {
            continue;
        }
        let rel = path
            .strip_prefix(root)
            .map_err(|_| StoreError::InvalidRequest { reason: "path" })?;
        let text = rel
            .to_str()
            .ok_or(StoreError::InvalidRequest { reason: "path" })?;
        if text.replace('\\', "/").starts_with(prefix) {
            out.push(text.replace('\\', "/"));
        }
    }
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "android"))]
const NOFOLLOW: i32 = 0x20000;
#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "tvos",
    target_os = "watchos"
))]
const NOFOLLOW: i32 = 0x100;
#[cfg(all(
    unix,
    not(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios",
        target_os = "tvos",
        target_os = "watchos"
    ))
))]
const NOFOLLOW: i32 = 0;
