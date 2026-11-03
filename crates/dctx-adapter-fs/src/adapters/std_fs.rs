// SPDX-License-Identifier: Apache-2.0
//! `std::fs` adapter that does not follow symlinks.
//! Implements DX-B3 confinement for the strict filesystem profile.
//! Depends on [`crate::ports`]. [`super::source::FsSource::open`] uses it.
//! Inode is recorded only where `std` exposes a file index.

use crate::ports::{DirEntry, EntryMeta, FsKind, SourceFs};
use std::{fs, io, path::Path, time::UNIX_EPOCH};

/// Process filesystem. Metadata and reads use `symlink_metadata` first.
#[derive(Debug, Default, Clone, Copy)]
pub struct StdFs;

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
        let meta = fs::symlink_metadata(path)?;
        if !meta.file_type().is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not a regular file",
            ));
        }
        fs::read(path)
    }

    fn read_policy(&self, path: &Path) -> io::Result<Option<String>> {
        match fs::symlink_metadata(path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "symlink rule file",
                ));
            }
            Ok(_) => {}
        }
        let bytes = fs::read(path)?;
        let text = String::from_utf8(bytes)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let after = fs::symlink_metadata(path)?;
        if after.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "rule file changed",
            ));
        }
        Ok(Some(text))
    }
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
    modified.duration_since(UNIX_EPOCH).map(|duration| duration.as_nanos()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "mtime before the unix epoch",
        )
    })
}

#[cfg(unix)]
#[allow(clippy::unnecessary_wraps, reason = "The cross-platform file-index hook returns None on platforms without Unix inode metadata.")]
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
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note.txt");
        fs::write(&path, b"abc").unwrap();
        let fs = StdFs;
        assert_eq!(fs.read_file(&path).unwrap(), b"abc");
        assert_eq!(fs.metadata(&path).unwrap().kind, FsKind::File);
        assert!(fs.read_policy(&dir.path().join("missing")).unwrap().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_content_and_rule_reads_fail_closed() {
        let dir = tempfile::tempdir().unwrap();
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
