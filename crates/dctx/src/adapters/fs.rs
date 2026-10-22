//! Filesystem source rooted at a caller-supplied directory.
//! Implements contracts/API-PORTS-v1.md `Source`: confined paths, metadata before content.
//! Depends on [`crate::ports::Source`] and the standard library. The session does not open a path itself.
//! Symlinks are not followed. A symlink read is [`dctx_core::DctxError::PolicyDenied`].

use std::io::Read;
use std::path::{Path, PathBuf};

use dctx_core::datatypes::NodeType;
use dctx_core::error::DctxError as CoreError;
use dctx_core::facts::{EntryMeta, ReadGrant, SourceEntry};
use dctx_core::ids::{RelativePath, Timestamp};
use dctx_ports::Source;

use crate::domain::DctxError;

/// A directory tree the caller opened by path. Nothing outside `root` is reachable.
#[derive(Debug, Clone)]
pub struct FsSource {
    root: PathBuf,
}

impl FsSource {
    /// Canonicalizes `path` and requires a directory. An empty path is [`DctxError::InvalidRequest`].
    /// # Errors
    /// Refuses an empty/non-directory source root or canonicalization/metadata I/O failures.
    pub fn open(path: &Path) -> Result<Self, DctxError> {
        if path.as_os_str().is_empty() {
            return Err(DctxError::invalid("source path is empty"));
        }
        let root = std::fs::canonicalize(path).map_err(|error| DctxError::from_core(map_io(error)))?;
        let meta = std::fs::metadata(&root).map_err(|error| DctxError::from_core(map_io(error)))?;
        if !meta.is_dir() {
            return Err(DctxError::invalid("source root must be a directory"));
        }
        Ok(Self { root })
    }

    fn confined(&self, path: &RelativePath) -> Result<PathBuf, CoreError> {
        let mut full = self.root.clone();
        if !path.is_root() {
            for segment in path.as_str().split('/') {
                if segment.is_empty() || segment == "." || segment == ".." {
                    return Err(CoreError::InvalidRequest { reason: "path escapes the source root" });
                }
                full.push(segment);
            }
        }
        Ok(full)
    }
}

impl Source for FsSource {
    fn root_kind(&self) -> Result<NodeType, CoreError> {
        let meta = std::fs::symlink_metadata(&self.root).map_err(map_io)?;
        if meta.is_dir() { Ok(NodeType::Directory) } else { Err(CoreError::SourceUnavailable) }
    }

    fn entries(&self, dir: &RelativePath) -> Result<Vec<SourceEntry>, CoreError> {
        let full = self.confined(dir)?;
        let read = std::fs::read_dir(&full).map_err(map_io)?;
        let mut entries = Vec::new();
        for item in read {
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

    fn metadata(&self, path: &RelativePath) -> Result<EntryMeta, CoreError> {
        let full = self.confined(path)?;
        let meta = std::fs::symlink_metadata(&full).map_err(map_io)?;
        Ok(EntryMeta { size: meta.len(), modified: Timestamp(modified_millis(&meta)) })
    }

    fn read(&self, grant: &ReadGrant, max_bytes: u64) -> Result<Vec<u8>, CoreError> {
        let full = self.confined(grant.path())?;
        let meta = std::fs::symlink_metadata(&full).map_err(map_io)?;
        if meta.file_type().is_symlink() {
            return Err(CoreError::PolicyDenied);
        }
        if meta.len() > max_bytes {
            return Err(CoreError::ResourceLimit { resource: dctx_core::error::Resource::FileBytes, limit: max_bytes });
        }
        // Stop one byte past the cap so a file that grows after the stat cannot buffer an unbounded body.
        let file = std::fs::File::open(&full).map_err(map_io)?;
        let mut bytes = Vec::new();
        file.take(max_bytes.saturating_add(1)).read_to_end(&mut bytes).map_err(map_io)?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > max_bytes {
            return Err(CoreError::ResourceLimit { resource: dctx_core::error::Resource::FileBytes, limit: max_bytes });
        }
        Ok(bytes)
    }
}

fn modified_millis(meta: &std::fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
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
        assert_eq!(source.entries(&RelativePath::root()).expect("entries"), [] as [dctx_core::facts::SourceEntry; 0]);
        let path = RelativePath::parse("missing.txt").expect("path");
        let grant = ReadGrant::authorize(&path, FileHandlingDecision::Include, true).expect("grant");
        let error = source.read(&grant, 32).expect_err("read");
        assert_eq!(error, CoreError::SourceUnavailable);
    }

    #[test]
    fn read_stops_at_the_byte_cap_and_a_fitting_file_is_returned() {
        let dir = tempfile::tempdir().expect("temp");
        let path = dir.path().join("note.txt");
        let mut note = std::fs::File::options().write(true).create_new(true).open(&path).expect("note");
        std::io::Write::write_all(&mut note, b"01234567").expect("body");
        drop(note);
        let source = FsSource::open(dir.path()).expect("open");
        let rel = RelativePath::parse("note.txt").expect("path");
        let grant = ReadGrant::authorize(&rel, FileHandlingDecision::Include, true).expect("grant");
        let denied = source.read(&grant, 4).expect_err("over cap");
        assert!(matches!(
            denied,
            CoreError::ResourceLimit { resource: dctx_core::error::Resource::FileBytes, limit: 4 }
        ));
        assert_eq!(source.read(&grant, 8).expect("within cap"), b"01234567");
    }
}
