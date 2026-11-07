// SPDX-License-Identifier: Apache-2.0
//! Immutable object publication inside the cache root.
//! Implements review F05: create a new inode and link it, never truncate an existing object.
//! Depends on [`super::write_exclusive`] and std hard links. [`super::FsObjectStore::put_object`] calls this.
//! Same bytes are success. Different bytes are a refusal. A symlink is never followed.

use crate::error::CacheError;
use std::fs::{self, OpenOptions};
use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

static PUBLISH_SEQ: AtomicU64 = AtomicU64::new(1);

/// Publishes `bytes` at `path`. An existing regular file is left unchanged.
pub(super) fn publish_immutable(path: &Path, bytes: &[u8]) -> Result<(), CacheError> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() || !meta.is_file() => {
            return Err(CacheError::OutsideCacheRoot);
        }
        Ok(meta) => return same_bytes(path, meta.len(), bytes),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(CacheError::LedgerIo { reason: "metadata" }),
    }
    let temp = temp_beside(path)?;
    super::write_exclusive(&temp, bytes)?;
    match fs::hard_link(&temp, path) {
        Ok(()) => {
            let _ = fs::remove_file(&temp);
            Ok(())
        }
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
            let _ = fs::remove_file(&temp);
            match fs::symlink_metadata(path) {
                Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() => {
                    same_bytes(path, meta.len(), bytes)
                }
                Ok(_) => Err(CacheError::OutsideCacheRoot),
                Err(_) => Err(CacheError::LedgerIo { reason: "metadata" }),
            }
        }
        Err(_) => {
            let _ = fs::remove_file(&temp);
            Err(CacheError::LedgerIo { reason: "create" })
        }
    }
}

fn temp_beside(path: &Path) -> Result<std::path::PathBuf, CacheError> {
    let parent = path.parent().ok_or(CacheError::OutsideCacheRoot)?;
    let seq = PUBLISH_SEQ.fetch_add(1, Ordering::Relaxed);
    Ok(parent.join(format!(".pub.{}.{seq}", super::lock::owner_id())))
}

fn same_bytes(path: &Path, len: u64, bytes: &[u8]) -> Result<(), CacheError> {
    let Ok(expected) = u64::try_from(bytes.len()) else {
        return Err(CacheError::InvalidRequest {
            reason: "object already published",
        });
    };
    if len != expected {
        return Err(CacheError::InvalidRequest {
            reason: "object already published",
        });
    }
    let mut file = open_nofollow(path)?;
    let mut buf = [0u8; 8192];
    let mut offset = 0usize;
    while offset < bytes.len() {
        let end = offset.saturating_add(buf.len()).min(bytes.len());
        let want = &bytes[offset..end];
        file.read_exact(&mut buf[..want.len()])
            .map_err(|_| CacheError::LedgerIo { reason: "read" })?;
        if &buf[..want.len()] != want {
            return Err(CacheError::InvalidRequest {
                reason: "object already published",
            });
        }
        offset = end;
    }
    Ok(())
}

fn open_nofollow(path: &Path) -> Result<std::fs::File, CacheError> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(super::no_follow());
    }
    options
        .open(path)
        .map_err(|_| CacheError::LedgerIo { reason: "read" })
}

#[cfg(test)]
mod tests {
    use super::super::FsObjectStore;
    use crate::error::CacheError;
    use std::fs;

    fn root() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("temp");
        let root = dir.path().join("cache");
        fs::create_dir(&root).expect("dir");
        (dir, root)
    }

    #[test]
    fn objects_are_published_immutably() {
        let (_dir, root) = root();
        let objects = FsObjectStore::open(&root).expect("open");
        objects.put_object(1, b"alpha").expect("first");
        let path = root.join("objects/1");
        assert_eq!(fs::read(&path).expect("read"), b"alpha");
        let refused = objects.put_object(1, b"beta!");
        assert!(matches!(
            refused,
            Err(CacheError::InvalidRequest {
                reason: "object already published"
            })
        ));
        assert_eq!(fs::read(&path).expect("kept"), b"alpha");
        objects.put_object(1, b"alpha").expect("same bytes");
        assert_eq!(fs::read(&path).expect("still"), b"alpha");
        objects.put_object(2, b"beta!").expect("other id");
        assert_eq!(fs::read(root.join("objects/2")).expect("second"), b"beta!");
        assert_eq!(fs::read(&path).expect("first stays"), b"alpha");
    }
}
