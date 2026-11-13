// SPDX-License-Identifier: Apache-2.0
//! Filesystem source rooted at a caller-supplied directory.
//! Implements contracts/API-PORTS-v1.md `Source`: confined paths, metadata before content.
//! Depends on [`crate::ports::Source`] and the standard library. The session does not open a path itself.
//! Symlinks are not followed. A symlink read is [`dctx_core::DctxError::PolicyDenied`].
//! Unix operations resolve one component at a time relative to an open directory handle,
//! with kernel no-follow guards. Other platforms use path checks; see SECURITY-MODEL.md.

#[cfg(not(unix))]
use std::io::Read;
use std::path::Path;
#[cfg(not(unix))]
use std::path::PathBuf;

#[cfg(any(not(unix), test))]
use dctx_core::datatypes::NodeType;
use dctx_core::error::DctxError as CoreError;
#[cfg(any(not(unix), test))]
use dctx_core::facts::EntryStamp;
#[cfg(not(unix))]
use dctx_core::facts::{EntryMeta, ReadGrant, SourceEntry};
#[cfg(not(unix))]
use dctx_core::ids::{RelativePath, Timestamp};
#[cfg(any(not(unix), test))]
use dctx_ports::Source;

use crate::domain::DctxError;

#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "Windows identity and ACL queries require private FFI wrappers borrowing owned file and token handles."
)]
mod windows;

#[cfg(any(windows, test))]
mod windows_retry;

#[cfg(unix)]
mod unix;

/// A read-only directory source. Link and race defenses depend on the platform (see `docs/SECURITY-MODEL.md`).
#[derive(Debug, Clone)]
pub struct FsSource {
    #[cfg(not(unix))]
    root: PathBuf,
    #[cfg(windows)]
    _root_guards: std::sync::Arc<Vec<std::fs::File>>,
    #[cfg(unix)]
    directory: std::sync::Arc<std::fs::File>,
}

impl FsSource {
    /// Canonicalizes `path` and requires a directory. An empty path is [`DctxError::InvalidRequest`].
    /// # Errors
    /// Refuses an empty/non-directory source root or canonicalization/metadata I/O failures.
    pub fn open(path: &Path) -> Result<Self, DctxError> {
        if path.as_os_str().is_empty() {
            return Err(DctxError::invalid("source path is empty"));
        }
        let root =
            std::fs::canonicalize(path).map_err(|error| DctxError::from_core(map_io(error)))?;
        let meta = std::fs::metadata(&root).map_err(|error| DctxError::from_core(map_io(error)))?;
        if !meta.is_dir() {
            return Err(DctxError::invalid("source root must be a directory"));
        }
        #[cfg(unix)]
        {
            let directory = unix::open_root(&root).map_err(DctxError::from_core)?;
            Ok(Self {
                directory: std::sync::Arc::new(directory),
            })
        }
        #[cfg(windows)]
        {
            let guards = windows::pin_source_root(&root)
                .map_err(|error| DctxError::from_core(map_io(error)))?;
            Ok(Self {
                root,
                _root_guards: std::sync::Arc::new(guards),
            })
        }
        #[cfg(not(any(unix, windows)))]
        Ok(Self { root })
    }

    #[cfg(not(unix))]
    fn confined(&self, path: &RelativePath) -> Result<PathBuf, CoreError> {
        let mut full = self.root.clone();
        if !path.is_root() {
            for segment in path.as_str().split('/') {
                if segment.is_empty() || segment == "." || segment == ".." {
                    return Err(CoreError::InvalidRequest {
                        reason: "path escapes the source root",
                    });
                }
                full.push(segment);
            }
        }
        Ok(full)
    }

    /// Refuses `path` when an intermediate directory (and, with `last`, the final component) is a symlink.
    #[cfg(not(unix))]
    fn reject_links(
        &self,
        path: &RelativePath,
        last: bool,
    ) -> Result<Vec<std::fs::File>, CoreError> {
        #[cfg(windows)]
        {
            let full = self.confined(path)?;
            let directory = if last {
                full.as_path()
            } else {
                full.parent().unwrap_or(&self.root)
            };
            windows::pin_directories(directory).map_err(map_io)
        }
        #[cfg(not(windows))]
        {
            if path.is_root() {
                return Ok(Vec::new());
            }
            let segments: Vec<&str> = path.as_str().split('/').collect();
            let checked = if last {
                segments.len()
            } else {
                segments.len().saturating_sub(1)
            };
            let mut full = self.root.clone();
            for segment in segments.iter().take(checked) {
                full.push(segment);
                if std::fs::symlink_metadata(&full)
                    .map_err(map_io)?
                    .file_type()
                    .is_symlink()
                {
                    return Err(CoreError::PolicyDenied);
                }
            }
            Ok(Vec::new())
        }
    }
}

/// Opens `path` for reading without following a final symlink. A symlink there is [`CoreError::PolicyDenied`].
#[cfg(not(unix))]
fn open_no_follow(path: &Path) -> Result<std::fs::File, CoreError> {
    #[cfg(not(unix))]
    let opened = {
        #[cfg(windows)]
        {
            windows::open_content(path)
        }
        #[cfg(not(windows))]
        {
            std::fs::File::open(path)
        }
    };
    let file = match opened {
        Ok(file) => file,
        Err(error) => {
            if std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink()) {
                return Err(CoreError::PolicyDenied);
            }
            return Err(map_io(error));
        }
    };
    // The handle, not the path, decides: a directory or device swapped in after the stat is refused.
    if !file.metadata().map_err(map_io)?.is_file() {
        return Err(CoreError::PolicyDenied);
    }
    Ok(file)
}

#[cfg(not(unix))]
impl Source for FsSource {
    #[cfg(windows)]
    fn inventory(&self) -> Option<Box<dyn dctx_ports::source::Inventory + '_>> {
        Some(Box::new(windows::Walk::new(self)))
    }

    fn root_kind(&self) -> Result<NodeType, CoreError> {
        #[cfg(windows)]
        let _guards = self.reject_links(&RelativePath::root(), true)?;
        let meta = std::fs::symlink_metadata(&self.root).map_err(map_io)?;
        if meta.is_dir() {
            Ok(NodeType::Directory)
        } else {
            Err(CoreError::SourceUnavailable)
        }
    }

    fn entries(&self, dir: &RelativePath) -> Result<Vec<SourceEntry>, CoreError> {
        let full = self.confined(dir)?;
        let _guards = self.reject_links(dir, true)?;
        let read = std::fs::read_dir(&full).map_err(map_io)?;
        let mut entries = Vec::new();
        let cap = crate::domain::entries_ceiling().map_err(|_| CoreError::InvalidRequest {
            reason: "invalid DCTX_MAX_ENTRIES ceiling",
        })?;
        for (visited, item) in read.enumerate() {
            if visited as u64 >= cap {
                return Err(CoreError::ResourceLimit {
                    resource: dctx_core::error::Resource::Entries,
                    limit: cap,
                });
            }
            let item = item.map_err(map_io)?;
            let Some(name) = item.file_name().to_str().map(str::to_string) else {
                continue;
            };
            if name.is_empty() || name == "." || name == ".." {
                continue;
            }
            let kind = item.file_type().map_err(map_io)?;
            if kind.is_symlink() || !kind.is_dir() {
                entries.push(SourceEntry::file(&name));
            } else {
                entries.push(SourceEntry::directory(&name));
            }
        }
        Ok(entries)
    }

    fn kind(&self, path: &RelativePath) -> Result<NodeType, CoreError> {
        let _guards = self.reject_links(path, false)?;
        let meta = std::fs::symlink_metadata(self.confined(path)?).map_err(map_io)?;
        Ok(if meta.is_dir() {
            NodeType::Directory
        } else {
            NodeType::File
        })
    }

    fn metadata(&self, path: &RelativePath) -> Result<EntryMeta, CoreError> {
        let full = self.confined(path)?;
        let _guards = self.reject_links(path, false)?;
        let meta = std::fs::symlink_metadata(&full).map_err(map_io)?;
        Ok(EntryMeta {
            size: meta.len(),
            modified: Timestamp(modified_millis(&meta)),
        })
    }

    fn stat(&self, path: &RelativePath) -> Result<(EntryMeta, Option<EntryStamp>), CoreError> {
        let full = self.confined(path)?;
        let _guards = self.reject_links(path, false)?;
        // The clock is read before the stat: an edit after this stat gets a time no earlier than `now`,
        // so a stamp older than `now - RACY_WINDOW_NS` can never be matched by a later edit.
        let now = now_ns();
        #[cfg(windows)]
        let (meta, inode) = windows::stat(&full).map_err(map_io)?;
        #[cfg(not(windows))]
        let meta = std::fs::symlink_metadata(&full).map_err(map_io)?;
        #[cfg(not(windows))]
        let inode = inode_of(&meta);
        let entry = EntryMeta {
            size: meta.len(),
            modified: Timestamp(modified_millis(&meta)),
        };
        Ok((entry, settled_stamp(&meta, now, inode)))
    }

    fn read(&self, grant: &ReadGrant, max_bytes: u64) -> Result<Vec<u8>, CoreError> {
        let full = self.confined(grant.path())?;
        let _guards = self.reject_links(grant.path(), false)?;
        let meta = std::fs::symlink_metadata(&full).map_err(map_io)?;
        if meta.file_type().is_symlink() {
            return Err(CoreError::PolicyDenied);
        }
        if meta.len() > max_bytes {
            return Err(CoreError::ResourceLimit {
                resource: dctx_core::error::Resource::FileBytes,
                limit: max_bytes,
            });
        }
        // Stop one byte past the cap so a file that grows after the stat cannot buffer an unbounded body.
        let file = open_no_follow(&full)?;
        let mut bytes = Vec::new();
        file.take(max_bytes.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(map_io)?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > max_bytes {
            return Err(CoreError::ResourceLimit {
                resource: dctx_core::error::Resource::FileBytes,
                limit: max_bytes,
            });
        }
        Ok(bytes)
    }
}

#[cfg(not(unix))]
fn modified_millis(meta: &std::fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |duration| {
            u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
        })
}

/// A stamp is trusted only when the file's mtime is at least this far behind the clock of the stat (git's racy-entry rule).
const RACY_WINDOW_NS: u128 = 2_000_000_000;

fn now_ns() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos())
}

/// The stat identity of a regular file whose mtime is settled: at least [`RACY_WINDOW_NS`] before `now_ns`.
/// `None` for anything else (a symlink, a recent or future mtime, no inode on this platform), and the caller hashes.
#[cfg(any(not(unix), test))]
fn settled_stamp(meta: &std::fs::Metadata, now_ns: u128, inode: Option<u64>) -> Option<EntryStamp> {
    if !meta.file_type().is_file() {
        return None;
    }
    let modified_ns = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_nanos();
    if modified_ns == 0 || modified_ns.saturating_add(RACY_WINDOW_NS) > now_ns {
        return None;
    }
    Some(EntryStamp {
        size: meta.len(),
        modified_ns,
        inode: inode?,
    })
}

#[cfg(not(any(unix, windows)))]
fn inode_of(_meta: &std::fs::Metadata) -> Option<u64> {
    None
}

fn map_io(error: std::io::Error) -> CoreError {
    if error.kind() == std::io::ErrorKind::NotFound {
        CoreError::SourceUnavailable
    } else {
        CoreError::io(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dctx_core::FileHandlingDecision;
    use dctx_core::facts::ReadGrant;
    use dctx_core::ids::RelativePath;
    use std::fs;

    #[test]
    fn settled_stamp_requires_identity_and_the_full_racy_window() {
        let dir = tempfile::tempdir().expect("temp");
        let path = dir.path().join("stamp");
        fs::write(&path, b"abc").expect("write");
        let meta = fs::symlink_metadata(&path).expect("metadata");
        let mtime = meta
            .modified()
            .unwrap()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let settled_at = mtime + RACY_WINDOW_NS;
        assert_eq!(settled_stamp(&meta, settled_at - 1, Some(42)), None);
        assert_eq!(settled_stamp(&meta, mtime - 1, Some(42)), None);
        assert_eq!(settled_stamp(&meta, settled_at, None), None);
        assert_eq!(
            settled_stamp(&meta, settled_at, Some(42)),
            Some(EntryStamp {
                size: 3,
                modified_ns: mtime,
                inode: 42
            })
        );
        let directory = fs::metadata(dir.path()).expect("directory");
        assert_eq!(settled_stamp(&directory, u128::MAX, Some(42)), None);
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(std::time::UNIX_EPOCH)
            .unwrap();
        assert_eq!(
            settled_stamp(&fs::metadata(&path).unwrap(), settled_at, Some(42)),
            None
        );
    }

    #[test]
    fn stat_stamps_only_a_settled_regular_file() {
        let dir = tempfile::tempdir().expect("temp");
        let path = dir.path().join("a.txt");
        fs::write(&path, b"abc").expect("write");
        let source = FsSource::open(dir.path()).expect("open");
        let rel = RelativePath::parse("a.txt").expect("path");
        let (meta, fresh) = source.stat(&rel).expect("stat");
        assert_eq!(meta.size, 3);
        assert_eq!(
            fresh, None,
            "a file written just now is inside the racy window"
        );
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        fs::File::options()
            .write(true)
            .open(&path)
            .expect("open")
            .set_modified(old)
            .expect("age");
        let (_, settled) = source.stat(&rel).expect("stat");
        let stamp = settled.expect("settled stamp");
        assert_eq!(stamp.size, 3);
        assert_ne!(stamp.inode, 0);
        assert_eq!(source.stat(&rel).expect("again").1, Some(stamp));
        let future = std::time::SystemTime::now() + std::time::Duration::from_secs(3600);
        fs::File::options()
            .write(true)
            .open(&path)
            .expect("open")
            .set_modified(future)
            .expect("future");
        assert_eq!(
            source.stat(&rel).expect("future").1,
            None,
            "a future mtime is never settled"
        );
    }

    #[test]
    fn empty_path_and_missing_root_are_refused() {
        let error = FsSource::open(Path::new("")).expect_err("empty");
        assert_eq!(error.code().as_str(), "InvalidRequest");
        let missing = FsSource::open(Path::new("this-dctx-root-is-not-real")).expect_err("missing");
        assert_eq!(missing.code().as_str(), "SourceUnavailable");
    }

    #[test]
    fn a_directory_lists_and_a_missing_file_is_unavailable() {
        let dir = tempfile::tempdir().expect("temp");
        let source = FsSource::open(dir.path()).expect("open");
        assert_eq!(source.root_kind().expect("root"), NodeType::Directory);
        assert_eq!(
            source.entries(&RelativePath::root()).expect("entries"),
            [] as [dctx_core::facts::SourceEntry; 0]
        );
        let path = RelativePath::parse("missing.txt").expect("path");
        let grant =
            ReadGrant::authorize(&path, FileHandlingDecision::Include, true).expect("grant");
        let error = source.read(&grant, 32).expect_err("read");
        assert_eq!(error, CoreError::SourceUnavailable);
    }

    #[test]
    fn read_stops_at_the_byte_cap_and_a_fitting_file_is_returned() {
        let dir = tempfile::tempdir().expect("temp");
        let path = dir.path().join("note.txt");
        let mut note = std::fs::File::options()
            .write(true)
            .create_new(true)
            .open(&path)
            .expect("note");
        std::io::Write::write_all(&mut note, b"01234567").expect("body");
        drop(note);
        let source = FsSource::open(dir.path()).expect("open");
        let rel = RelativePath::parse("note.txt").expect("path");
        let grant = ReadGrant::authorize(&rel, FileHandlingDecision::Include, true).expect("grant");
        let denied = source.read(&grant, 4).expect_err("over cap");
        assert!(matches!(
            denied,
            CoreError::ResourceLimit {
                resource: dctx_core::error::Resource::FileBytes,
                limit: 4
            }
        ));
        assert_eq!(source.read(&grant, 8).expect("within cap"), b"01234567");
    }

    /// A symlink to a file outside the root, or a symlinked directory, never yields outside bytes.
    #[cfg(unix)]
    #[test]
    fn symlinks_out_of_the_root_are_refused() {
        let outside = tempfile::tempdir().expect("outside");
        fs::write(outside.path().join("secret.txt"), b"OUTSIDE_SENTINEL").expect("secret");
        let dir = tempfile::tempdir().expect("temp");
        fs::write(dir.path().join("kept.txt"), b"kept").expect("kept");
        std::os::unix::fs::symlink(
            outside.path().join("secret.txt"),
            dir.path().join("link.txt"),
        )
        .expect("file link");
        std::os::unix::fs::symlink(outside.path(), dir.path().join("linkdir")).expect("dir link");
        let source = FsSource::open(dir.path()).expect("open");
        let mut names: Vec<_> = source
            .entries(&RelativePath::root())
            .expect("entries")
            .into_iter()
            .map(|entry| (entry.name.clone(), entry.is_dir()))
            .collect();
        names.sort();
        assert_eq!(
            names,
            [
                ("kept.txt".to_string(), false),
                ("link.txt".to_string(), false),
                ("linkdir".to_string(), false)
            ]
        );
        for target in ["link.txt", "linkdir/secret.txt"] {
            let path = RelativePath::parse(target).expect("path");
            let grant =
                ReadGrant::authorize(&path, FileHandlingDecision::Include, true).expect("grant");
            assert_eq!(
                source.read(&grant, 64).expect_err(target),
                CoreError::PolicyDenied,
                "{target}"
            );
        }
        let linked = RelativePath::parse("linkdir").expect("dir");
        assert_eq!(
            source.entries(&linked).expect_err("dir link"),
            CoreError::PolicyDenied
        );
        let kept = RelativePath::parse("kept.txt").expect("kept");
        let grant =
            ReadGrant::authorize(&kept, FileHandlingDecision::Include, true).expect("grant");
        assert_eq!(source.read(&grant, 64).expect("kept"), b"kept");
    }
}
