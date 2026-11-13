// SPDX-License-Identifier: Apache-2.0
//! `std::fs` adapter that does not follow symlinks.
//! Implements DX-B3 confinement for the strict filesystem profile.
//! Depends on [`crate::ports`]. [`super::source::FsSource::open`] uses it.
//! Inode is recorded only where `std` exposes a file index.

use crate::ports::{DirEntry, EntryMeta, FsKind, SourceFs};
use std::io::Read;
#[cfg(not(unix))]
use std::time::UNIX_EPOCH;
use std::{fs, io, path::Path};

/// Process filesystem. Reads refuse symlinks in every path component.
/// Content reads accept up to 8 MiB; UTF-8 policy reads accept up to 1 MiB.
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
        #[cfg(unix)]
        {
            nofollow::metadata(path)
        }
        #[cfg(not(unix))]
        {
            #[cfg(windows)]
            let _guards = pinned::ancestors_of(path)?;
            let meta = fs::symlink_metadata(path)?;
            Ok(EntryMeta {
                kind: kind_of(&meta),
                size: meta.len(),
                modified_ns: modified_ns(&meta)?,
                inode: None,
            })
        }
    }

    fn read_dir(&self, path: &Path) -> io::Result<Vec<DirEntry>> {
        #[cfg(unix)]
        {
            nofollow::read_dir(path)
        }
        #[cfg(not(unix))]
        {
            #[cfg(windows)]
            let _guards = pinned::directory(path)?;
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
    }

    fn read_link(&self, path: &Path) -> io::Result<std::path::PathBuf> {
        #[cfg(unix)]
        {
            nofollow::read_link(path)
        }
        #[cfg(not(unix))]
        {
            #[cfg(windows)]
            let _guards = pinned::ancestors_of(path)?;
            fs::read_link(path)
        }
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

#[cfg(unix)]
fn unsafe_path() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "unsafe file path; use normal components without symlinks",
    )
}

/// Handle-relative lookups: every directory is opened from its parent handle with
/// `O_NOFOLLOW`, so a component swapped for a symlink fails instead of redirecting.
#[cfg(unix)]
mod nofollow {
    use super::{DirEntry, EntryMeta, FsKind, fs, unsafe_path};
    use rustix::fs::{AtFlags, CWD, Dir, FileType, Mode, OFlags, Stat, fstat, openat, statat};
    use std::ffi::{OsStr, OsString};
    use std::io;
    use std::os::fd::OwnedFd;
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    use std::path::{Component, Path, PathBuf};

    const DIRECTORY: OFlags = OFlags::RDONLY
        .union(OFlags::DIRECTORY)
        .union(OFlags::NOFOLLOW)
        .union(OFlags::CLOEXEC);

    /// Opens `path` as a directory one component at a time; no component may be a symlink.
    pub(super) fn open_directory(path: &Path) -> io::Result<OwnedFd> {
        let start = if path.is_absolute() { "/" } else { "." };
        let mut directory = openat(CWD, start, DIRECTORY, Mode::empty())?;
        for component in path.components() {
            match component {
                Component::RootDir | Component::CurDir => {}
                Component::Normal(name) => {
                    directory = openat(&directory, name, DIRECTORY, Mode::empty())?;
                }
                _ => return Err(unsafe_path()),
            }
        }
        Ok(directory)
    }

    /// The no-follow parent handle and final name of `path`, or `None` for a path without a final name.
    fn parent(path: &Path) -> io::Result<Option<(OwnedFd, &OsStr)>> {
        match path.components().next_back() {
            Some(Component::Normal(name)) => {
                let parent = path.parent().unwrap_or_else(|| Path::new(""));
                Ok(Some((open_directory(parent)?, name)))
            }
            Some(Component::RootDir | Component::CurDir) | None => Ok(None),
            Some(_) => Err(unsafe_path()),
        }
    }

    /// Opens the final component no-follow and non-blocking; the caller checks the file type.
    pub(super) fn open_file(path: &Path) -> io::Result<fs::File> {
        let (directory, name) = parent(path)?.ok_or_else(unsafe_path)?;
        let flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK;
        Ok(fs::File::from(openat(
            &directory,
            name,
            flags,
            Mode::empty(),
        )?))
    }

    pub(super) fn metadata(path: &Path) -> io::Result<EntryMeta> {
        let stat = match parent(path)? {
            Some((directory, name)) => statat(&directory, name, AtFlags::SYMLINK_NOFOLLOW)?,
            None => fstat(open_directory(path)?)?,
        };
        entry_meta(&stat)
    }

    pub(super) fn read_dir(path: &Path) -> io::Result<Vec<DirEntry>> {
        let directory = open_directory(path)?;
        let mut reader = Dir::read_from(&directory)?;
        let mut entries = Vec::new();
        while let Some(entry) = reader.read() {
            let entry = entry?;
            let name = entry.file_name();
            if matches!(name.to_bytes(), b"." | b"..") {
                continue;
            }
            let file_type = match entry.file_type() {
                FileType::Unknown => FileType::from_raw_mode(
                    statat(&directory, name, AtFlags::SYMLINK_NOFOLLOW)?.st_mode,
                ),
                known => known,
            };
            entries.push(DirEntry {
                name: OsStr::from_bytes(name.to_bytes()).to_os_string(),
                kind: kind_of(file_type),
            });
        }
        Ok(entries)
    }

    pub(super) fn read_link(path: &Path) -> io::Result<PathBuf> {
        let (directory, name) = parent(path)?.ok_or_else(unsafe_path)?;
        let target = rustix::fs::readlinkat(&directory, name, Vec::new())?;
        Ok(PathBuf::from(OsString::from_vec(target.into_bytes())))
    }

    fn kind_of(file_type: FileType) -> FsKind {
        match file_type {
            FileType::Symlink => FsKind::Symlink,
            FileType::Directory => FsKind::Directory,
            FileType::RegularFile => FsKind::File,
            _ => FsKind::Other,
        }
    }

    /// Same values as `std::fs::symlink_metadata`: logical size, mtime in ns since the epoch, inode.
    #[allow(
        clippy::unnecessary_fallible_conversions,
        clippy::useless_conversion,
        reason = "Unix stat fields have different signedness and widths across targets."
    )]
    fn entry_meta(stat: &Stat) -> io::Result<EntryMeta> {
        let before_epoch = || {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "mtime before the unix epoch; correct the file timestamp and retry",
            )
        };
        let seconds = u128::try_from(stat.st_mtime).map_err(|_| before_epoch())?;
        let nanos = u128::try_from(stat.st_mtime_nsec).map_err(|_| before_epoch())?;
        Ok(EntryMeta {
            kind: kind_of(FileType::from_raw_mode(stat.st_mode)),
            size: u64::try_from(stat.st_size).map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "negative file size; repair or replace the source file and retry",
                )
            })?,
            modified_ns: seconds * 1_000_000_000 + nanos,
            inode: Some(u64::try_from(stat.st_ino).unwrap_or(0)),
        })
    }
}

/// Windows ancestor guards. A guard needs `FILE_LIST_DIRECTORY`: Windows does not apply
/// share-mode checks to attribute-only opens, so `FILE_READ_ATTRIBUTES` alone pins nothing.
/// Denying write and delete sharing keeps each directory from being renamed, replaced, or
/// turned into a junction while the guard is held. Mirrors the engine's `pin_directories`.
#[cfg(windows)]
mod pinned {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use std::path::Path;
    use std::{fs, io};

    pub(super) const FILE_LIST_DIRECTORY: u32 = 0x0000_0001;
    pub(super) const FILE_READ_ATTRIBUTES: u32 = 0x0000_0080;
    pub(super) const FILE_SHARE_READ: u32 = 0x0000_0001;
    pub(super) const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    pub(super) const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    pub(super) const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;

    /// Pins `directory` and each of its ancestors, outermost first.
    pub(super) fn directory(directory: &Path) -> io::Result<Vec<fs::File>> {
        let mut guards = Vec::new();
        for directory in directory
            .ancestors()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .filter(|p| !p.as_os_str().is_empty())
        {
            let guard = fs::File::options()
                .access_mode(FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES)
                .share_mode(FILE_SHARE_READ)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
                .open(directory)?;
            let meta = guard.metadata()?;
            if !meta.is_dir() || meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "unsafe rule directory; choose a real directory without reparse points",
                ));
            }
            guards.push(guard);
        }
        Ok(guards)
    }

    /// Pins every ancestor directory of `path`, not `path` itself.
    pub(super) fn ancestors_of(path: &Path) -> io::Result<Vec<fs::File>> {
        path.parent().map_or_else(|| Ok(Vec::new()), directory)
    }
}

fn read_bounded(path: &Path, cap: u64) -> io::Result<Vec<u8>> {
    #[cfg(unix)]
    let file = nofollow::open_file(path)?;
    #[cfg(windows)]
    let (_guards, file) = {
        use pinned::{FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT};
        use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
        let guards = pinned::ancestors_of(path)?;
        let file = fs::File::options()
            .read(true)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)?;
        if file.metadata()?.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "reparse point file; choose a regular source file without reparse points",
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
            "not a regular file; choose a regular source file",
        ));
    }
    if before.len() > cap {
        return Err(io::Error::new(
            io::ErrorKind::FileTooLarge,
            "file exceeds byte limit; reduce the file size or choose a larger explicit byte cap",
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
            "file exceeds byte limit; reduce the file size or choose a larger explicit byte cap",
        ));
    }
    if before.len() != after.len()
        || before.modified()? != after.modified()?
        || bytes.len() as u64 != after.len()
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "file changed during read; retry after the file stops changing",
        ));
    }
    Ok(bytes)
}

#[cfg(not(unix))]
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

#[cfg(not(unix))]
fn modified_ns(meta: &fs::Metadata) -> io::Result<u128> {
    let modified = meta.modified()?;
    modified
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "mtime before the unix epoch; correct the file timestamp and retry",
            )
        })
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

#[cfg(test)]
mod nofollow_tests {
    use super::*;

    fn canonical_temp() -> tempfile::TempDir {
        tempfile::tempdir_in(fs::canonicalize(std::env::temp_dir()).unwrap()).unwrap()
    }

    #[cfg(unix)]
    #[test]
    fn metadata_matches_std_symlink_metadata() {
        use std::os::unix::fs::MetadataExt;
        let temp = canonical_temp();
        let file = temp.path().join("file.txt");
        fs::write(&file, b"hello").unwrap();
        let link = temp.path().join("link");
        std::os::unix::fs::symlink("file.txt", &link).unwrap();
        for path in [file.as_path(), link.as_path(), temp.path(), Path::new("/")] {
            let ours = StdFs.metadata(path).unwrap();
            let std = fs::symlink_metadata(path).unwrap();
            let std_ns = std
                .modified()
                .unwrap()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            assert_eq!(ours.size, std.len(), "{path:?}");
            assert_eq!(ours.modified_ns, std_ns, "{path:?}");
            assert_eq!(ours.inode, Some(std.ino()), "{path:?}");
        }
        assert_eq!(StdFs.metadata(&file).unwrap().kind, FsKind::File);
        assert_eq!(StdFs.metadata(&link).unwrap().kind, FsKind::Symlink);
        assert_eq!(StdFs.metadata(temp.path()).unwrap().kind, FsKind::Directory);
        assert_eq!(
            StdFs
                .metadata(&temp.path().join("missing"))
                .unwrap_err()
                .kind(),
            io::ErrorKind::NotFound
        );
    }

    #[cfg(unix)]
    #[test]
    fn listing_reports_kinds_without_following_links() {
        let temp = canonical_temp();
        fs::write(temp.path().join("file"), b"x").unwrap();
        fs::create_dir(temp.path().join("dir")).unwrap();
        std::os::unix::fs::symlink("dir", temp.path().join("link")).unwrap();
        // rustix has no mkfifoat on Apple targets; the POSIX mkfifo utility exists on every Unix host.
        let made = std::process::Command::new("mkfifo")
            .arg(temp.path().join("fifo"))
            .status()
            .unwrap();
        assert!(made.success());
        let mut entries: Vec<_> = StdFs
            .read_dir(temp.path())
            .unwrap()
            .into_iter()
            .map(|entry| (entry.name.into_string().unwrap(), entry.kind))
            .collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(
            entries,
            vec![
                ("dir".to_string(), FsKind::Directory),
                ("fifo".to_string(), FsKind::Other),
                ("file".to_string(), FsKind::File),
                ("link".to_string(), FsKind::Symlink),
            ]
        );
        assert_eq!(
            StdFs.read_link(&temp.path().join("link")).unwrap(),
            Path::new("dir")
        );
    }

    #[cfg(unix)]
    #[test]
    fn stat_listing_and_readlink_refuse_a_linked_ancestor() {
        let temp = canonical_temp();
        let outside = canonical_temp();
        fs::create_dir(outside.path().join("secret-dir")).unwrap();
        fs::write(outside.path().join("secret.txt"), b"secret").unwrap();
        std::os::unix::fs::symlink("secret.txt", outside.path().join("secret-link")).unwrap();
        let swapped = temp.path().join("sub");
        std::os::unix::fs::symlink(outside.path(), &swapped).unwrap();
        assert!(StdFs.read_dir(&swapped).is_err());
        assert!(StdFs.metadata(&swapped.join("secret.txt")).is_err());
        assert!(StdFs.metadata(&swapped.join("secret-dir")).is_err());
        assert!(StdFs.read_link(&swapped.join("secret-link")).is_err());
        assert!(StdFs.read_dir(&swapped.join("secret-dir")).is_err());
        assert_eq!(StdFs.metadata(&swapped).unwrap().kind, FsKind::Symlink);
        assert!(StdFs.read_link(Path::new("/")).is_err());
        assert!(StdFs.metadata(&temp.path().join("sub/../sub")).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn ancestor_guards_block_replacement_until_dropped() {
        let temp = canonical_temp();
        let parent = temp.path().join("parent");
        let moved = temp.path().join("moved");
        fs::create_dir(&parent).unwrap();
        let guards = pinned::ancestors_of(&parent.join("rule")).unwrap();
        assert!(fs::rename(&parent, &moved).is_err());
        drop(guards);
        fs::rename(&parent, &moved).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn junction_ancestors_are_refused() {
        let temp = canonical_temp();
        let outside = temp.path().join("outside");
        let link = temp.path().join("junction");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("rules"), b"secret").unwrap();
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
        assert!(StdFs.read_policy(&link.join("rules")).is_err());
        assert!(StdFs.read_file(&link.join("rules")).is_err());
        assert!(StdFs.metadata(&link.join("rules")).is_err());
        assert!(StdFs.read_dir(&link).is_err());
    }
}
