// SPDX-License-Identifier: Apache-2.0
//! `pack --output` and `template build --output` files. The page is written to a fresh sibling file, then renamed over the target.
//! A link, directory, FIFO or device at the target is refused: it is never followed or truncated.
//! A failed write leaves the target as it was and removes the sibling.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Writes `path` through `write`, replacing a regular file atomically and keeping its permissions.
/// # Errors
/// Refuses a non-regular target or a path without a file name; returns create, write or rename errors.
pub(crate) fn write_with<T>(
    path: &Path,
    write: impl FnOnce(&mut dyn Write) -> io::Result<T>,
) -> io::Result<T> {
    if path.as_os_str().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "--output path is empty",
        ));
    }
    let permissions = match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_file() => Some(meta.permissions()),
        Ok(_) => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "refusing to replace a link or a non-regular file",
            ))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    let mut staged = Staged::create(path)?;
    let file = staged
        .file
        .take()
        .ok_or_else(|| io::Error::other("staged file missing"))?;
    let mut buffered = io::BufWriter::with_capacity(64 * 1024, file);
    let value = write(&mut buffered)?;
    let file = buffered
        .into_inner()
        .map_err(io::IntoInnerError::into_error)?;
    if let Some(permissions) = permissions {
        file.set_permissions(permissions)?;
    }
    drop(file);
    // rename replaces a link or file that appeared at `path` since the check; it never follows it.
    match fs::rename(&staged.path, path) {
        Ok(()) => staged.committed = true,
        // ERROR_SHARING_VIOLATION: an open dctx session (here or in another process, such as an
        // persistent caller) pins its root and every ancestor against renames. Copy in place instead.
        #[cfg(windows)]
        Err(error) if error.raw_os_error() == Some(32) => copy_in_place(&staged.path, path)?,
        Err(error) => return Err(error),
    }
    Ok(value)
}

/// Overwrites `target` with the staged bytes through a handle that never follows a link, for a
/// directory that refuses renames. A link or non-regular file at `target` is refused before it
/// is truncated.
#[cfg(windows)]
fn copy_in_place(staged: &Path, target: &Path) -> io::Result<()> {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(target)?;
    let meta = file.metadata()?;
    if !meta.is_file() || meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "refusing to replace a link or a non-regular file",
        ));
    }
    file.set_len(0)?;
    io::copy(&mut fs::File::open(staged)?, &mut file)?;
    Ok(())
}

/// A sibling of the target, created exclusively (no existing entry or link is ever opened).
struct Staged {
    path: PathBuf,
    file: Option<fs::File>,
    committed: bool,
}

impl Staged {
    fn create(target: &Path) -> io::Result<Self> {
        static SERIAL: AtomicU64 = AtomicU64::new(0);
        if target.file_name().is_none() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "--output must name a file",
            ));
        }
        let parent = match target.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => parent,
            _ => Path::new("."),
        };
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.subsec_nanos());
        let mut last = None;
        for _ in 0..16 {
            let serial = SERIAL.fetch_add(1, Ordering::Relaxed);
            let path = parent.join(format!(
                ".dctx-output-{}-{nanos}-{serial}.tmp",
                std::process::id()
            ));
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(file) => {
                    return Ok(Self {
                        path,
                        file: Some(file),
                        committed: false,
                    })
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => last = Some(error),
                Err(error) => return Err(error),
            }
        }
        Err(last.unwrap_or_else(|| io::Error::other("no free staging name")))
    }
}

impl Drop for Staged {
    fn drop(&mut self) {
        drop(self.file.take());
        if !self.committed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_page(path: &Path, page: &[u8]) -> io::Result<()> {
        write_with(path, |writer| writer.write_all(page))
    }

    #[test]
    fn a_new_or_regular_target_gets_exactly_the_page_and_no_staging_debris() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("context.txt");
        write_page(&target, b"first\n").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"first\n");
        write_page(&target, b"second\n").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"second\n");
        let names: Vec<_> = fs::read_dir(temp.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, [std::ffi::OsString::from("context.txt")]);
    }

    #[test]
    fn a_directory_target_is_refused_and_a_failed_write_leaves_no_debris() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("dir");
        fs::create_dir(&target).unwrap();
        assert_eq!(
            write_page(&target, b"page").unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        let regular = temp.path().join("kept.txt");
        fs::write(&regular, b"old").unwrap();
        let failed = write_with(&regular, |writer| {
            writer.write_all(b"partial")?;
            Err::<(), _>(io::Error::other("render failed"))
        });
        assert!(failed.is_err());
        assert_eq!(fs::read(&regular).unwrap(), b"old");
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn a_link_or_fifo_target_is_refused_and_never_written_through() {
        let temp = tempfile::tempdir().unwrap();
        let victim = temp.path().join("victim");
        fs::write(&victim, b"keep").unwrap();
        let link = temp.path().join("context.txt");
        std::os::unix::fs::symlink(&victim, &link).unwrap();
        assert!(write_page(&link, b"page").is_err());
        assert_eq!(fs::read(&victim).unwrap(), b"keep");
        assert!(fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        let dangling = temp.path().join("dangling");
        std::os::unix::fs::symlink(temp.path().join("absent"), &dangling).unwrap();
        assert!(write_page(&dangling, b"page").is_err());
        assert!(!temp.path().join("absent").exists());
        let fifo = temp.path().join("fifo");
        let made = std::process::Command::new("mkfifo").arg(&fifo).status();
        if made.is_ok_and(|status| status.success()) {
            assert!(write_page(&fifo, b"page").is_err());
        }
    }

    /// Windows directory guards (an open session's root and ancestors) deny write sharing, so
    /// the rename fails with a sharing violation; the page is still written, with no debris.
    #[cfg(windows)]
    #[test]
    fn a_target_in_a_rename_pinned_directory_is_still_replaced() {
        use std::os::windows::fs::OpenOptionsExt;
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("context.txt");
        fs::write(&target, b"old page that is longer").unwrap();
        let guard = fs::File::options()
            .access_mode(0x0001 | 0x0080) // FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES
            .share_mode(0x0001) // FILE_SHARE_READ
            .custom_flags(0x0200_0000) // FILE_FLAG_BACKUP_SEMANTICS
            .open(temp.path())
            .unwrap();
        write_page(&target, b"new").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"new");
        write_page(&temp.path().join("fresh.txt"), b"fresh").unwrap();
        drop(guard);
        assert_eq!(fs::read(temp.path().join("fresh.txt")).unwrap(), b"fresh");
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn replacing_a_file_keeps_its_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("context.txt");
        fs::write(&target, b"old").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o640)).unwrap();
        write_page(&target, b"new").unwrap();
        let mode = fs::metadata(&target).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o640);
    }
}
