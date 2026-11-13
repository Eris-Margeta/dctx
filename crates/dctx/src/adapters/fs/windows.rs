// SPDX-License-Identifier: Apache-2.0
//! Attribute-only, no-follow stat with the NTFS file index from the same handle.

use std::fs::{File, Metadata, OpenOptions};
use std::io;
use std::os::windows::{fs::OpenOptionsExt, io::AsRawHandle};
use std::path::Path;

use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    GetFileInformationByHandle, READ_CONTROL,
};

mod inventory;
mod security;
pub(super) use inventory::Walk;

/// Retry transient Windows sharing conflicts without changing access or sharing flags.
fn open_with_retry(path: &Path, options: &OpenOptions) -> io::Result<File> {
    super::windows_retry::retry(|| options.open(path), std::thread::sleep)
}

fn open_attributes(path: &Path) -> io::Result<File> {
    // Attribute queries must coexist with writers and renames. BACKUP_SEMANTICS
    // permits directories; OPEN_REPARSE_POINT keeps the query on the named object.
    let file = open_with_retry(
        path,
        File::options()
            .access_mode(FILE_READ_ATTRIBUTES | READ_CONTROL)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT),
    )?;
    // A successful CreateFile can inherit READ_ATTRIBUTES from parent listing
    // permission. Check this object's DACL before trusting any enumeration data.
    security::check_read_attributes(&file)?;
    Ok(file)
}

pub(super) fn stat(path: &Path) -> io::Result<(Metadata, Option<u64>)> {
    let file = open_attributes(path)?;
    let meta = file.metadata()?;
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: the file owns a valid handle throughout the call and `info` is a
    // properly aligned, writable structure of the required size. No pointer escapes.
    let found = unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) };
    if found == 0 {
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::PermissionDenied {
            return Err(error);
        }
    }
    // Filesystems without a usable identity fall back to hashing, never a fake inode.
    let inode = (found != 0)
        .then(|| (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow))
        .filter(|index| *index != 0);
    Ok((meta, inode))
}

/// Reuse directory-enumeration metadata, but confirm its identity and access
/// through the same attribute handle as `stat`. A listing alone cannot certify
/// either a file index or a per-file permission failure.
fn stat_entry(entry: &std::fs::DirEntry) -> io::Result<(Metadata, Option<u64>)> {
    use std::os::windows::fs::MetadataExt;

    let meta = entry.metadata()?;
    let path = entry.path();
    let file = open_attributes(&path)?;
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: `file` owns the borrowed handle; `info` is writable and correctly sized.
    let found = unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) };
    if found == 0 {
        let error = io::Error::last_os_error();
        // An ACL denial is authoritative; do not substitute cached enumeration
        // metadata or retry through another metadata API that can bypass it.
        if error.kind() == io::ErrorKind::PermissionDenied {
            return Err(error);
        }
    }
    let modified = (u64::from(info.ftLastWriteTime.dwHighDateTime) << 32)
        | u64::from(info.ftLastWriteTime.dwLowDateTime);
    let size = (u64::from(info.nFileSizeHigh) << 32) | u64::from(info.nFileSizeLow);
    if found == 0
        || modified != meta.last_write_time()
        || size != meta.file_size()
        || info.dwFileAttributes != meta.file_attributes()
    {
        // A racing replacement/edit or a filesystem without this query uses
        // the original live stat, including its missing-identity hash fallback.
        return stat(&path);
    }
    let inode = (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow);
    Ok((meta, (inode != 0).then_some(inode)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_open_preserves_exclusive_handle_failure_and_recovers_after_release() {
        use std::io::Read;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source");
        std::fs::write(&path, b"source bytes").unwrap();
        let held = File::options()
            .read(true)
            .share_mode(0)
            .open(&path)
            .unwrap();
        let original = File::open(&path).unwrap_err();
        let error = open_content(&path).unwrap_err();
        assert_eq!(error.raw_os_error(), Some(32));
        assert_eq!(error.kind(), original.kind());
        assert_eq!(error.to_string(), original.to_string());
        drop(held);
        let mut bytes = Vec::new();
        open_content(&path)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        assert_eq!(bytes, b"source bytes");
    }

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

    #[test]
    fn enumeration_stat_matches_live_stat_and_rechecks_racing_metadata() {
        use std::os::windows::fs::MetadataExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source");
        std::fs::write(&path, b"abc").unwrap();
        let entry = std::fs::read_dir(dir.path())
            .unwrap()
            .next()
            .unwrap()
            .unwrap();
        let (listed, listed_id) = stat_entry(&entry).unwrap();
        let (live, live_id) = stat(&path).unwrap();
        assert_eq!(listed_id, live_id);
        assert_eq!(listed.file_size(), live.file_size());
        assert_eq!(listed.last_write_time(), live.last_write_time());
        // DirEntry retains its original metadata. A changed handle query must
        // cause a fresh stat, rather than pairing the old size with a live ID.
        std::fs::write(&path, b"longer replacement").unwrap();
        let (changed, changed_id) = stat_entry(&entry).unwrap();
        let (live, live_id) = stat(&path).unwrap();
        assert_eq!(changed_id, live_id);
        assert_eq!(changed.file_size(), live.file_size());
        assert_eq!(changed.last_write_time(), live.last_write_time());
    }
}

/// Keep every directory name stable until the operation completes.
/// Write and delete sharing are denied to prevent reparse changes and replacement.
pub(super) fn pin_directories(path: &Path) -> io::Result<Vec<File>> {
    let mut guards = Vec::new();
    for directory in path.ancestors().collect::<Vec<_>>().into_iter().rev() {
        guards.push(pin_directory(directory)?);
    }
    Ok(guards)
}

/// Source-lifetime guards keep names stable but let released children be renamed.
pub(super) fn pin_source_root(path: &Path) -> io::Result<Vec<File>> {
    let strict = pin_directories(path)?;
    // Acquire the entire chain before relaxing write sharing: each pinned child
    // keeps its parent nonempty, so Windows cannot turn that parent into a junction.
    // The root itself may be empty, so every operation must strictly repin/check it.
    let guards = path
        .ancestors()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|directory| pin_directory_shared(directory, FILE_SHARE_READ | FILE_SHARE_WRITE))
        .collect();
    drop(strict);
    guards
}

/// The caller must already hold guards for every ancestor of `path`.
fn pin_directory(path: &Path) -> io::Result<File> {
    pin_directory_shared(path, FILE_SHARE_READ)
}

fn pin_directory_shared(path: &Path, sharing: u32) -> io::Result<File> {
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_LIST_DIRECTORY,
    };
    // Windows does not apply share-mode checks to attribute-only opens, so a guard with
    // FILE_READ_ATTRIBUTES alone would pin nothing. FILE_LIST_DIRECTORY reads names only,
    // never file content, and makes the denied write/delete sharing enforceable.
    let file = open_with_retry(
        path,
        File::options()
            .access_mode(FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES)
            .share_mode(sharing)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT),
    )?;
    let meta = file.metadata()?;
    if !meta.is_dir() || meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "source directory is a reparse point or not a directory",
        ));
    }
    Ok(file)
}

pub(super) fn open_content(path: &Path) -> io::Result<File> {
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
    let file = open_with_retry(
        path,
        File::options()
            .read(true)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT),
    )?;
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
    fn source_guards_allow_child_mutations_but_keep_the_root_name_pinned() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("root");
        std::fs::create_dir_all(root.join("child")).unwrap();
        let guards = pin_source_root(&root).unwrap();
        std::fs::rename(root.join("child"), root.join("moved")).unwrap();
        std::fs::remove_dir(root.join("moved")).unwrap();
        assert!(std::fs::rename(&root, temp.path().join("replaced")).is_err());
        let writer = || {
            File::options()
                .write(true)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
                .open(&root)
        };
        drop(writer().unwrap());
        let operation = pin_directory(&root).unwrap();
        assert_eq!(writer().unwrap_err().raw_os_error(), Some(32));
        drop(operation);
        drop(writer().unwrap());
        drop(guards);
        std::fs::rename(&root, temp.path().join("released")).unwrap();
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
