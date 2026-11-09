// SPDX-License-Identifier: Apache-2.0
//! Std filesystem adapter for durable segment names.
//! Implements SEGMENTS-v1 sync and rename on a caller-supplied directory.
//! Depends on [`crate::ports::StoreFs`]. Crash cuts and Windows sharing live in [`super::memory`].
//! Unix directories are mode 0700 and files 0600. Symlinks and over-ceiling reads are refused.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::domain::{Boundary, StoreError, check_root};
use crate::ports::{self, FsCapabilities, StoreFs, read_ceiling};

#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "MoveFileExW has no write-through equivalent in std; the private wrapper validates and owns both UTF-16 paths."
)]
mod windows;

/// A real directory of immutable segments and generation pointers.
pub struct StdFs {
    root: PathBuf,
    locks: Mutex<BTreeMap<String, File>>,
}

impl StdFs {
    /// Creates `root` if needed and confines later names under its canonical path.
    ///
    /// A missing root is created with mode 0700 on Unix. A symlinked or non-directory root is
    /// refused. On Unix an existing root must also be owned by the effective user and not
    /// writable by group or others, and its ancestors must be owned by root or that user and not
    /// writable by others (root-owned sticky directories and read-only mounts excepted).
    ///
    /// # Errors
    /// Returns [`StoreError::InvalidRequest`] for an unsafe root, or directory-creation and
    /// canonicalization IO failures.
    pub fn open(root: &Path) -> Result<Self, StoreError> {
        let root = ports::prepare_root(root)?;
        Ok(Self {
            root,
            locks: Mutex::new(BTreeMap::new()),
        })
    }

    fn guard(&self) -> MutexGuard<'_, BTreeMap<String, File>> {
        self.locks.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn confine(&self, rel: &str) -> Result<PathBuf, StoreError> {
        ports::confine_under(&self.root, rel)
    }

    fn make_dir(&self, dir: &Path) -> Result<(), StoreError> {
        ports::ensure_dir(&self.root, dir)
    }

    fn parent_of(&self, path: &Path) -> Result<(), StoreError> {
        if let Some(parent) = path.parent() {
            self.make_dir(parent)?;
        }
        Ok(())
    }

    fn open_file(&self, rel: &str, write: bool, truncate: bool) -> Result<File, StoreError> {
        ports::open_under(&self.root, rel, write, truncate)
    }
}

impl StoreFs for StdFs {
    fn capabilities(&self) -> FsCapabilities {
        FsCapabilities {
            // Windows pairs file flushes with synchronous write-through renames;
            // it does not try to fsync a POSIX directory handle.
            sync: cfg!(any(unix, windows)),
            atomic_replace: cfg!(any(unix, windows)),
            os_lock: true,
        }
    }

    fn checkpoint(&self, _boundary: Boundary) -> Result<(), StoreError> {
        Ok(())
    }

    fn try_lock(&self, root: &str) -> Result<(), StoreError> {
        check_root(root)?;
        let mut locks = self.guard();
        if locks.contains_key(root) {
            return Err(StoreError::Busy);
        }
        let rel = format!("roots/{root}/writer.lock");
        let path = self.confine(&rel)?;
        self.parent_of(&path)?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        ports::set_nofollow(&mut options);
        let file = options.open(&path).map_err(ports::io_err)?;
        match file.try_lock() {
            Ok(()) => {
                locks.insert(root.to_string(), file);
                Ok(())
            }
            Err(std::fs::TryLockError::WouldBlock) => Err(StoreError::Busy),
            Err(std::fs::TryLockError::Error(err)) => Err(ports::io_err(err)),
        }
    }

    fn unlock(&self, root: &str) -> Result<(), StoreError> {
        let mut locks = self.guard();
        if let Some(file) = locks.remove(root) {
            file.unlock().map_err(ports::io_err)?;
        }
        Ok(())
    }

    fn lock_held(&self, root: &str) -> bool {
        self.guard().contains_key(root)
    }

    fn held_roots(&self) -> Vec<String> {
        self.guard().keys().cloned().collect()
    }

    fn create(&self, path: &str) -> Result<(), StoreError> {
        let path = self.confine(path)?;
        self.parent_of(&path)?;
        if fs::symlink_metadata(&path).is_ok() {
            return Err(StoreError::io("exists"));
        }
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        ports::set_nofollow(&mut options);
        options.open(&path).map_err(ports::io_err)?;
        Ok(())
    }

    fn write(&self, path: &str, bytes: &[u8]) -> Result<(), StoreError> {
        let mut file = self.open_file(path, true, true)?;
        file.write_all(bytes).map_err(ports::io_err)?;
        Ok(())
    }

    fn flush(&self, path: &str) -> Result<(), StoreError> {
        let mut file = self.open_file(path, true, false)?;
        file.flush().map_err(ports::io_err)?;
        Ok(())
    }

    fn sync_file(&self, path: &str) -> Result<(), StoreError> {
        let file = self.open_file(path, true, false)?;
        // std calls FlushFileBuffers on Windows, with a writable file handle.
        file.sync_all().map_err(ports::io_err)?;
        Ok(())
    }

    fn sync_dir(&self) -> Result<(), StoreError> {
        #[cfg(unix)]
        {
            let dir = File::open(&self.root).map_err(ports::io_err)?;
            dir.sync_all().map_err(ports::io_err)
        }
        #[cfg(windows)]
        {
            // Every publication rename already used MOVEFILE_WRITE_THROUGH.
            // There is no separate POSIX-style directory flush on Windows.
            Ok(())
        }
        #[cfg(not(any(unix, windows)))]
        {
            Err(StoreError::UnsupportedDurability { capability: "sync" })
        }
    }

    fn rename_new(&self, from: &str, to: &str) -> Result<(), StoreError> {
        let from = self.confine(from)?;
        let to = self.confine(to)?;
        self.parent_of(&to)?;
        if fs::symlink_metadata(&to).is_ok() {
            return Err(StoreError::io("exists"));
        }
        #[cfg(windows)]
        return windows::rename(&from, &to, false).map_err(ports::io_err);
        #[cfg(not(windows))]
        fs::rename(from, to).map_err(ports::io_err)
    }

    fn rename_replace(&self, from: &str, to: &str) -> Result<(), StoreError> {
        let from = self.confine(from)?;
        let to = self.confine(to)?;
        self.parent_of(&to)?;
        #[cfg(windows)]
        return windows::rename(&from, &to, true).map_err(ports::io_err);
        #[cfg(not(windows))]
        fs::rename(from, to).map_err(ports::io_err)
    }

    fn read(&self, path: &str) -> Result<Vec<u8>, StoreError> {
        let max = read_ceiling(path);
        let file = self.open_file(path, false, false)?;
        let got = file.metadata().map_err(ports::io_err)?.len();
        if got > max {
            return Err(StoreError::CorruptIndex { reason: "limit" });
        }
        let mut buf = Vec::new();
        file.take(max.saturating_add(1))
            .read_to_end(&mut buf)
            .map_err(ports::io_err)?;
        if u64::try_from(buf.len()).unwrap_or(u64::MAX) > max {
            return Err(StoreError::CorruptIndex { reason: "limit" });
        }
        Ok(buf)
    }

    fn exists(&self, path: &str) -> Result<bool, StoreError> {
        let path = self.confine(path)?;
        match fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_symlink() => {
                Err(StoreError::InvalidRequest { reason: "path" })
            }
            Ok(_) => Ok(true),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(err) => Err(ports::io_err(err)),
        }
    }

    fn len(&self, path: &str) -> Result<u64, StoreError> {
        let path = self.confine(path)?;
        let meta = fs::symlink_metadata(&path).map_err(ports::io_err)?;
        if meta.file_type().is_symlink() || !meta.is_file() {
            return Err(StoreError::InvalidRequest { reason: "path" });
        }
        Ok(meta.len())
    }

    fn unlink(&self, path: &str) -> Result<(), StoreError> {
        let path = self.confine(path)?;
        let meta = fs::symlink_metadata(&path).map_err(ports::io_err)?;
        if meta.file_type().is_symlink() {
            return Err(StoreError::InvalidRequest { reason: "path" });
        }
        fs::remove_file(path).map_err(ports::io_err)
    }

    fn list_prefix(&self, prefix: &str) -> Result<Vec<String>, StoreError> {
        ports::list_under(&self.root, prefix)
    }
}

#[cfg(all(test, any(unix, windows)))]
mod tests {
    use super::*;

    #[test]
    fn publication_renames_preserve_bytes_and_never_clobber_immutable_names() {
        let dir = tempfile::tempdir().unwrap();
        let fs = StdFs::open(dir.path()).unwrap();
        for (path, body) in [("tmp/first", &b"first"[..]), ("tmp/second", &b"second"[..])] {
            fs.create(path).unwrap();
            fs.write(path, body).unwrap();
            fs.flush(path).unwrap();
            fs.sync_file(path).unwrap();
        }
        fs.rename_new("tmp/first", "objects/immutable").unwrap();
        fs.sync_dir().unwrap();
        assert_eq!(fs.read("objects/immutable").unwrap(), b"first");
        assert!(!fs.exists("tmp/first").unwrap());
        assert!(fs.rename_new("tmp/second", "objects/immutable").is_err());
        assert_eq!(fs.read("objects/immutable").unwrap(), b"first");
        assert_eq!(fs.read("tmp/second").unwrap(), b"second");
        fs.rename_replace("tmp/second", "objects/immutable")
            .unwrap();
        fs.sync_dir().unwrap();
        assert_eq!(fs.read("objects/immutable").unwrap(), b"second");
        assert!(!fs.exists("tmp/second").unwrap());
        assert!(
            fs.rename_replace("tmp/missing", "objects/immutable")
                .is_err()
        );
        assert_eq!(fs.read("objects/immutable").unwrap(), b"second");
    }
}

#[cfg(all(test, unix))]
mod root_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn mode(path: &Path) -> u32 {
        fs::symlink_metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn a_missing_root_and_its_directories_are_created_private() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("store");
        let fs = StdFs::open(&root).unwrap();
        assert_eq!(mode(&root), 0o700);
        fs.create("objects/deep/name").unwrap();
        assert_eq!(mode(&root.join("objects")), 0o700);
        assert_eq!(mode(&root.join("objects/deep")), 0o700);
    }

    #[test]
    fn a_symlinked_or_non_directory_root_is_refused_and_not_chmodded() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target");
        fs::create_dir(&target).unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o755)).unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(matches!(
            StdFs::open(&link),
            Err(StoreError::InvalidRequest { reason: "root" })
        ));
        assert_eq!(mode(&target), 0o755);
        let file = dir.path().join("file");
        fs::write(&file, b"x").unwrap();
        assert!(StdFs::open(&file).is_err());
        assert!(StdFs::open(&dir.path().join("a/../b")).is_err());
    }

    #[test]
    fn a_writable_root_is_refused_and_a_readable_one_is_tightened() {
        let dir = tempfile::tempdir().unwrap();
        let open = dir.path().join("open");
        fs::create_dir(&open).unwrap();
        fs::set_permissions(&open, fs::Permissions::from_mode(0o777)).unwrap();
        assert!(matches!(
            StdFs::open(&open),
            Err(StoreError::InvalidRequest { reason: "root" })
        ));
        let readable = dir.path().join("readable");
        fs::create_dir(&readable).unwrap();
        fs::set_permissions(&readable, fs::Permissions::from_mode(0o755)).unwrap();
        StdFs::open(&readable).unwrap();
        assert_eq!(mode(&readable), 0o700);
    }

    #[test]
    fn a_root_under_a_world_writable_ancestor_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let shared = dir.path().join("shared");
        fs::create_dir(&shared).unwrap();
        fs::set_permissions(&shared, fs::Permissions::from_mode(0o777)).unwrap();
        assert!(StdFs::open(&shared.join("store")).is_err());
    }

    #[test]
    fn a_root_under_a_linked_ancestor_resolves_to_the_real_directory() {
        // Mirrors macOS temporary directories under /var -> /private/var on any Unix.
        let dir = tempfile::tempdir().unwrap();
        let real = fs::canonicalize(dir.path()).unwrap().join("real");
        fs::create_dir(&real).unwrap();
        let linked = dir.path().join("linked");
        std::os::unix::fs::symlink(&real, &linked).unwrap();
        let fs = StdFs::open(&linked.join("store")).unwrap();
        assert_eq!(fs.root, real.join("store"));
        assert_eq!(mode(&real.join("store")), 0o700);
        fs.create("objects/name").unwrap();
        assert!(real.join("store/objects").is_dir());
    }
}
