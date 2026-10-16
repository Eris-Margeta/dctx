//! Std filesystem adapter for durable segment names.
//! Implements SEGMENTS-v1 sync and rename on a caller-supplied directory.
//! Depends on [`crate::ports::StoreFs`]. Crash cuts and Windows sharing live in [`super::memory`].
//! Unix directories are mode 0700 and files 0600. A path that canonicalizes outside the root is refused.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::domain::{check_rel, check_root, Boundary, StoreError};
use crate::ports::{FsCapabilities, StoreFs};

fn io_err(err: std::io::Error) -> StoreError {
    if err.kind() == std::io::ErrorKind::StorageFull || err.raw_os_error() == Some(28) {
        return StoreError::DiskFull;
    }
    if err.raw_os_error() == Some(32) {
        return StoreError::SharingViolation;
    }
    StoreError::io(err)
}

/// A real directory of immutable segments and generation pointers.
pub struct StdFs {
    root: PathBuf,
    locks: Mutex<BTreeMap<String, File>>,
}

impl StdFs {
    /// Creates `root` if needed and confines later names under its canonical path.
    pub fn open(root: &Path) -> Result<Self, StoreError> {
        fs::create_dir_all(root).map_err(io_err)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(root, fs::Permissions::from_mode(0o700));
        }
        let root = fs::canonicalize(root).map_err(io_err)?;
        Ok(Self {
            root,
            locks: Mutex::new(BTreeMap::new()),
        })
    }

    fn guard(&self) -> MutexGuard<'_, BTreeMap<String, File>> {
        self.locks.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn abs(&self, rel: &str) -> Result<PathBuf, StoreError> {
        check_rel(rel)?;
        Ok(self.root.join(rel))
    }

    fn make_dir(&self, dir: &Path) -> Result<(), StoreError> {
        fs::create_dir_all(dir).map_err(io_err)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o700));
        }
        let got = fs::canonicalize(dir).map_err(io_err)?;
        if !got.starts_with(&self.root) {
            return Err(StoreError::InvalidRequest { reason: "path" });
        }
        Ok(())
    }

    fn parent_of(&self, path: &Path) -> Result<(), StoreError> {
        if let Some(parent) = path.parent() {
            self.make_dir(parent)?;
        }
        Ok(())
    }
}

impl StoreFs for StdFs {
    fn capabilities(&self) -> FsCapabilities {
        FsCapabilities {
            sync: true,
            atomic_replace: true,
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
        let path = self.abs(&format!("roots/{root}/writer.lock"))?;
        self.parent_of(&path)?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(&path).map_err(io_err)?;
        match file.try_lock() {
            Ok(()) => {
                locks.insert(root.to_string(), file);
                Ok(())
            }
            Err(std::fs::TryLockError::WouldBlock) => Err(StoreError::Busy),
            Err(std::fs::TryLockError::Error(err)) => Err(io_err(err)),
        }
    }

    fn unlock(&self, root: &str) -> Result<(), StoreError> {
        let mut locks = self.guard();
        if let Some(file) = locks.remove(root) {
            file.unlock().map_err(io_err)?;
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
        let path = self.abs(path)?;
        self.parent_of(&path)?;
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        options.open(&path).map_err(io_err)?;
        Ok(())
    }

    fn write(&self, path: &str, bytes: &[u8]) -> Result<(), StoreError> {
        let path = self.abs(path)?;
        let mut file = OpenOptions::new().write(true).truncate(true).open(&path).map_err(io_err)?;
        file.write_all(bytes).map_err(io_err)?;
        Ok(())
    }

    fn flush(&self, path: &str) -> Result<(), StoreError> {
        let path = self.abs(path)?;
        let mut file = OpenOptions::new().write(true).open(&path).map_err(io_err)?;
        file.flush().map_err(io_err)?;
        Ok(())
    }

    fn sync_file(&self, path: &str) -> Result<(), StoreError> {
        let path = self.abs(path)?;
        let file = OpenOptions::new().write(true).open(&path).map_err(io_err)?;
        file.sync_all().map_err(io_err)?;
        Ok(())
    }

    fn sync_dir(&self) -> Result<(), StoreError> {
        let dir = File::open(&self.root).map_err(io_err)?;
        dir.sync_all().map_err(io_err)?;
        Ok(())
    }

    fn rename_new(&self, from: &str, to: &str) -> Result<(), StoreError> {
        let from = self.abs(from)?;
        let to = self.abs(to)?;
        self.parent_of(&to)?;
        if to.exists() {
            return Err(StoreError::io("exists"));
        }
        fs::rename(from, to).map_err(io_err)
    }

    fn rename_replace(&self, from: &str, to: &str) -> Result<(), StoreError> {
        let from = self.abs(from)?;
        let to = self.abs(to)?;
        self.parent_of(&to)?;
        #[cfg(windows)]
        if to.exists() {
            return Err(StoreError::UnsupportedDurability { capability: "atomic-replace" });
        }
        fs::rename(from, to).map_err(io_err)
    }

    fn read(&self, path: &str) -> Result<Vec<u8>, StoreError> {
        let path = self.abs(path)?;
        fs::read(&path).map_err(io_err)
    }

    fn exists(&self, path: &str) -> Result<bool, StoreError> {
        let path = self.abs(path)?;
        path.try_exists().map_err(io_err)
    }

    fn len(&self, path: &str) -> Result<u64, StoreError> {
        let path = self.abs(path)?;
        Ok(fs::metadata(path).map_err(io_err)?.len())
    }

    fn unlink(&self, path: &str) -> Result<(), StoreError> {
        let path = self.abs(path)?;
        fs::remove_file(path).map_err(io_err)
    }

    fn list_prefix(&self, prefix: &str) -> Result<Vec<String>, StoreError> {
        let mut out = Vec::new();
        walk(&self.root, &self.root, prefix, &mut out)?;
        out.sort();
        Ok(out)
    }
}

fn walk(root: &Path, dir: &Path, prefix: &str, out: &mut Vec<String>) -> Result<(), StoreError> {
    if !dir.exists() {
        return Ok(());
    }
    let entries = match fs::read_dir(dir) {
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
        let rel = path.strip_prefix(root).map_err(|_| StoreError::InvalidRequest { reason: "path" })?;
        let text = rel.to_str().ok_or(StoreError::InvalidRequest { reason: "path" })?;
        let text = text.replace('\\', "/");
        if text.starts_with(prefix) {
            out.push(text);
        }
    }
    Ok(())
}
