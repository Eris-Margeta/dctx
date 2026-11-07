// SPDX-License-Identifier: Apache-2.0
//! Attribute-only, no-follow stat with the NTFS file index from the same handle.

use std::fs::{File, Metadata};
use std::io;
use std::os::windows::{fs::OpenOptionsExt, io::AsRawHandle};
use std::path::Path;

use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_READ_ATTRIBUTES, GetFileInformationByHandle,
};

pub(super) fn stat(path: &Path) -> io::Result<(Metadata, Option<u64>)> {
    // READ_ATTRIBUTES does not read content. BACKUP_SEMANTICS also permits a
    // directory; OPEN_REPARSE_POINT inspects a link rather than its target.
    let file = File::options()
        .access_mode(FILE_READ_ATTRIBUTES)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    let meta = file.metadata()?;
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: the file owns a valid handle throughout the call and `info` is a
    // properly aligned, writable structure of the required size. No pointer escapes.
    let found = unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) };
    // Filesystems without a usable identity fall back to hashing, never a fake inode.
    let inode = (found != 0)
        .then(|| (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow))
        .filter(|index| *index != 0);
    Ok((meta, inode))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_index_tracks_the_file_across_names_and_changes_on_replacement() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source");
        let other = dir.path().join("other");
        std::fs::write(&path, b"abc").unwrap();
        let (_, first) = stat(&path).unwrap();
        assert!(first.is_some());
        std::fs::hard_link(&path, &other).unwrap();
        assert_eq!(stat(&other).unwrap().1, first);
        std::fs::remove_file(&path).unwrap();
        std::fs::write(&path, b"abc").unwrap();
        assert_ne!(stat(&path).unwrap().1, first);
        assert!(stat(dir.path()).unwrap().0.is_dir());
    }
}
