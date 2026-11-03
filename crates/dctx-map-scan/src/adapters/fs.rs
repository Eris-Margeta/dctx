// SPDX-License-Identifier: Apache-2.0
//! Standard filesystem traversal adapter.
//! Implements SPEC-MAP-MODE section 13a filesystem boundary.
//! Supplies entry kinds without following links to the walker.

use crate::ports::{
    ContentSource,
    fs::{Entry, FileKind, FileSystem},
};
use std::{fs, io, path::Path};

/// Read-only standard filesystem adapter.
pub struct StdFileSystem;
fn kind(t: fs::FileType) -> FileKind {
    if t.is_dir() {
        FileKind::Directory
    } else if t.is_file() {
        FileKind::File
    } else {
        FileKind::Other
    }
}
impl ContentSource for StdFileSystem {
    fn read_content(&self, path: &Path) -> io::Result<Vec<u8>> {
        fs::read(path)
    }
}
impl FileSystem for StdFileSystem {
    fn kind(&self, path: &Path) -> io::Result<FileKind> {
        Ok(kind(fs::symlink_metadata(path)?.file_type()))
    }
    fn entries(&self, path: &Path) -> io::Result<Vec<Entry>> {
        fs::read_dir(path)?
            .map(|entry| {
                let entry = entry?;
                Ok(Entry {
                    name: entry.file_name(),
                    kind: kind(entry.file_type()?),
                })
            })
            .collect()
    }
    fn size(&self, path: &Path) -> io::Result<u64> {
        Ok(fs::symlink_metadata(path)?.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn file_and_missing_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file");
        assert!(StdFileSystem.kind(&path).is_err());
        fs::write(&path, b"abc").unwrap();
        assert_eq!(StdFileSystem.kind(&path).unwrap(), FileKind::File);
        assert_eq!(StdFileSystem.size(&path).unwrap(), 3);
        assert_eq!(StdFileSystem.read_content(&path).unwrap(), b"abc");
        assert_eq!(StdFileSystem.entries(dir.path()).unwrap().len(), 1);
    }
    #[cfg(unix)]
    #[test]
    fn symlinks_are_not_followed() {
        let dir = tempfile::tempdir().unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(dir.path(), &link).unwrap();
        assert_eq!(StdFileSystem.kind(&link).unwrap(), FileKind::Other);
        assert_eq!(
            StdFileSystem.entries(dir.path()).unwrap()[0].kind,
            FileKind::Other
        );
    }
}
