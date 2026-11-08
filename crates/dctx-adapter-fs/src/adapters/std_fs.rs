// SPDX-License-Identifier: Apache-2.0
//! `std::fs` adapter that does not follow symlinks.
//! Implements DX-B3 confinement for the strict filesystem profile.
//! Depends on [`crate::ports`]. [`super::source::FsSource::open`] uses it.
//! Inode is recorded only where `std` exposes a file index.

use crate::ports::{DirEntry, EntryMeta, FsKind, SourceFs};
use std::io::Read;
use std::{fs, io, path::Path, time::UNIX_EPOCH};

/// Process filesystem. Reads refuse symlinks in every path component.
/// Callers must resolve trusted directory boundaries before appending child paths;
/// this adapter never canonicalizes a file or an in-tree directory.
#[derive(Debug, Default, Clone, Copy)]
pub struct StdFs;

impl StdFs {
    /// Reads a regular file with no-follow guards and a caller-supplied byte cap.
    /// # Errors
    /// Refuses unsafe/non-regular paths, changed or oversized files, and I/O failures.
    pub fn read_file_limited(&self, path: &Path, max_bytes: u64) -> io::Result<Vec<u8>> {
        read_bounded(path, max_bytes)
    }
}

impl SourceFs for StdFs {
    fn metadata(&self, path: &Path) -> io::Result<EntryMeta> {
        let meta = fs::symlink_metadata(path)?;
        Ok(EntryMeta {
            kind: kind_of(&meta),
            size: meta.len(),
            modified_ns: modified_ns(&meta)?,
            inode: file_index(&meta),
        })
    }

    fn read_dir(&self, path: &Path) -> io::Result<Vec<DirEntry>> {
        let mut entries = Vec::new();
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            let kind = if file_type.is_symlink() {
                FsKind::Symlink
            } else if file_type.is_dir() {
                FsKind::Directory
            } else if file_type.is_file() {
                FsKind::File
            } else {
                FsKind::Other
            };
            entries.push(DirEntry {
                name: entry.file_name(),
                kind,
            });
        }
        Ok(entries)
    }

    fn read_link(&self, path: &Path) -> io::Result<std::path::PathBuf> {
        fs::read_link(path)
    }

    fn read_file(&self, path: &Path) -> io::Result<Vec<u8>> {
        read_bounded(path, 8 * 1024 * 1024)
    }

    fn read_policy(&self, path: &Path) -> io::Result<Option<String>> {
        let bytes = match read_bounded(path, 1024 * 1024) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        String::from_utf8(bytes)
            .map(Some)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }
}

fn read_bounded(path: &Path, cap: u64) -> io::Result<Vec<u8>> {
    #[cfg(unix)]
    let file = {
        use rustix::fs::{CWD, Mode, OFlags, openat};
        use std::path::Component;
        let flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
        let mut directory = openat(
            CWD,
            if path.is_absolute() { "/" } else { "." },
            flags | OFlags::DIRECTORY,
            Mode::empty(),
        )?;
        let mut components = path.components().peekable();
        let mut opened = None;
        while let Some(component) = components.next() {
            match component {
                Component::RootDir | Component::CurDir => continue,
                Component::Normal(name) => {
                    let last = components.peek().is_none();
                    let next = openat(
                        &directory,
                        name,
                        flags
                            | if last {
                                OFlags::NONBLOCK
                            } else {
                                OFlags::DIRECTORY
                            },
                        Mode::empty(),
                    )?;
                    if last {
                        opened = Some(fs::File::from(next));
                    } else {
                        directory = next;
                    }
                }
                _ => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "unsafe file path",
                    ));
                }
            }
        }
        opened.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "not a file"))?
    };
    #[cfg(windows)]
    let (_guards, file) = {
        use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
        let mut guards = Vec::new();
        if let Some(parent) = path.parent() {
            for directory in parent
                .ancestors()
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .filter(|p| !p.as_os_str().is_empty())
            {
                let guard = fs::File::options()
                    .access_mode(0x80)
                    .share_mode(1)
                    .custom_flags(0x0220_0000)
                    .open(directory)?;
                let meta = guard.metadata()?;
                if !meta.is_dir() || meta.file_attributes() & 0x400 != 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "unsafe rule directory",
                    ));
                }
                guards.push(guard);
            }
        }
        let file = fs::File::options()
            .read(true)
            .custom_flags(0x0020_0000)
            .open(path)?;
        if file.metadata()?.file_attributes() & 0x400 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "reparse point file",
            ));
        }
        (guards, file)
    };
    #[cfg(not(any(unix, windows)))]
    let file = fs::File::open(path)?;
    let before = file.metadata()?;
    if !before.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a regular file",
        ));
    }
    if before.len() > cap {
        return Err(io::Error::new(
            io::ErrorKind::FileTooLarge,
            "file exceeds byte limit",
        ));
    }
    let mut bytes = Vec::new();
    (&file)
        .take(cap.saturating_add(1))
        .read_to_end(&mut bytes)?;
    let after = file.metadata()?;
    if bytes.len() as u64 > cap {
        return Err(io::Error::new(
            io::ErrorKind::FileTooLarge,
            "file exceeds byte limit",
        ));
    }
    if before.len() != after.len()
        || before.modified()? != after.modified()?
        || bytes.len() as u64 != after.len()
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "file changed during read",
        ));
    }
    Ok(bytes)
}

fn kind_of(meta: &fs::Metadata) -> FsKind {
    let file_type = meta.file_type();
    if file_type.is_symlink() {
        FsKind::Symlink
    } else if file_type.is_dir() {
        FsKind::Directory
    } else if file_type.is_file() {
        FsKind::File
    } else {
        FsKind::Other
    }
}

fn modified_ns(meta: &fs::Metadata) -> io::Result<u128> {
    let modified = meta.modified()?;
    modified
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "mtime before the unix epoch"))
}

#[cfg(unix)]
#[allow(
    clippy::unnecessary_wraps,
    reason = "The cross-platform file-index hook returns None on platforms without Unix inode metadata."
)]
fn file_index(meta: &fs::Metadata) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    Some(meta.ino())
}

#[cfg(not(unix))]
fn file_index(_meta: &fs::Metadata) -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_regular_file_and_treats_a_missing_rule_as_absent() {
        let dir = tempfile::tempdir_in(fs::canonicalize(std::env::temp_dir()).unwrap()).unwrap();
        let path = dir.path().join("note.txt");
        fs::write(&path, b"abc").unwrap();
        let fs = StdFs;
        assert_eq!(fs.read_file(&path).unwrap(), b"abc");
        assert_eq!(fs.metadata(&path).unwrap().kind, FsKind::File);
        assert!(
            fs.read_policy(&dir.path().join("missing"))
                .unwrap()
                .is_none()
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlink_content_and_rule_reads_fail_closed() {
        let dir = tempfile::tempdir_in(fs::canonicalize(std::env::temp_dir()).unwrap()).unwrap();
        let outside = dir.path().join("outside.txt");
        fs::write(&outside, b"secret").unwrap();
        let link = dir.path().join("leak");
        std::os::unix::fs::symlink(&outside, &link).unwrap();
        let fs = StdFs;
        assert!(fs.read_file(&link).is_err());
        assert!(fs.read_policy(&link).is_err());
        assert_eq!(fs.metadata(&link).unwrap().kind, FsKind::Symlink);
    }
}

#[cfg(test)]
mod security_tests {
    use super::*;

    #[test]
    fn sparse_source_and_policy_files_are_refused_before_buffering() {
        let temp = tempfile::tempdir_in(fs::canonicalize(std::env::temp_dir()).unwrap()).unwrap();
        let path = temp.path().join("sparse");
        let size = if cfg!(target_os = "linux") {
            1 << 40
        } else {
            9 * 1024 * 1024
        };
        fs::File::create(&path).unwrap().set_len(size).unwrap();
        assert!(StdFs.read_file(&path).is_err());
        assert!(StdFs.read_policy(&path).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn content_and_policy_reads_refuse_a_linked_ancestor() {
        let temp = tempfile::tempdir_in(fs::canonicalize(std::env::temp_dir()).unwrap()).unwrap();
        let outside =
            tempfile::tempdir_in(fs::canonicalize(std::env::temp_dir()).unwrap()).unwrap();
        fs::write(outside.path().join("rules"), b"secret").unwrap();
        std::os::unix::fs::symlink(outside.path(), temp.path().join("link")).unwrap();
        assert!(StdFs.read_file(&temp.path().join("link/rules")).is_err());
        assert!(StdFs.read_policy(&temp.path().join("link/rules")).is_err());
    }
}
