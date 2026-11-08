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

/// Keep every directory name stable until the operation completes.
/// Write and delete sharing are denied to prevent reparse changes and replacement.
pub(super) fn pin_directories(path: &Path) -> io::Result<Vec<File>> {
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_REPARSE_POINT, FILE_SHARE_READ};
    let mut guards = Vec::new();
    for directory in path.ancestors().collect::<Vec<_>>().into_iter().rev() {
        let file = File::options()
            .access_mode(FILE_READ_ATTRIBUTES)
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(directory)?;
        let meta = file.metadata()?;
        if !meta.is_dir() || meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "source directory is a reparse point or not a directory",
            ));
        }
        guards.push(file);
    }
    Ok(guards)
}

pub(super) fn open_content(path: &Path) -> io::Result<File> {
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
    let file = File::options()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    if file.metadata()?.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "source file is a reparse point",
        ));
    }
    Ok(file)
}

#[cfg(test)]
mod security_tests {
    use super::*;

    #[test]
    fn directory_guards_prevent_replacement_until_dropped() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("root");
        let moved = temp.path().join("moved");
        std::fs::create_dir(&root).unwrap();
        let guards = pin_directories(&root).unwrap();
        assert!(std::fs::rename(&root, &moved).is_err());
        let writer = || {
            File::options()
                .write(true)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
                .open(&root)
        };
        assert_eq!(writer().unwrap_err().raw_os_error(), Some(32)); // ERROR_SHARING_VIOLATION
        drop(guards);
        drop(writer().unwrap());
        std::fs::rename(&root, &moved).unwrap();
    }

    #[test]
    fn junctions_are_refused_by_directory_guards() {
        let temp = tempfile::tempdir().unwrap();
        let outside = temp.path().join("outside");
        let link = temp.path().join("junction");
        std::fs::create_dir(&outside).unwrap();
        let output = std::process::Command::new("cmd")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(&link)
            .arg(&outside)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(pin_directories(&link).is_err());
        std::fs::remove_dir(&link).unwrap();
    }
}
