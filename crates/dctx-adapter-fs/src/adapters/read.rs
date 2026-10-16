//! Authorized content reads with a before/after stamp.
//! Implements DX-B3: a race discards the digest instead of reporting it as current.
//! Depends on [`crate::ports`] and `dctx-hash`. Inventory and `validate` share it.

use super::error::SourceError;
use crate::domain::{FileStamp, StaleReason};
use crate::ports::{EntryMeta, FsKind, SourceFs};
use crate::ContentDigest;
use dctx_hash::Sha256;
use std::path::Path;

pub(crate) struct StableRead {
    pub stamp: FileStamp,
    pub digest: ContentDigest,
    pub bytes: Vec<u8>,
}

pub(crate) enum ReadAttempt {
    Stable(StableRead),
    Stale(StaleReason),
}

pub(crate) fn file_stamp<F: SourceFs>(fs: &F, path: &Path) -> Result<FileStamp, SourceError> {
    let meta = fs
        .metadata(path)
        .map_err(|source| SourceError::io(path, source))?;
    if meta.kind != FsKind::File && meta.kind != FsKind::Symlink {
        return Err(SourceError::InvalidPath {
            path: path.to_path_buf(),
        });
    }
    Ok(stamp_of(&meta))
}

pub(crate) fn read_authorized<F: SourceFs>(
    fs: &F,
    path: &Path,
) -> Result<ReadAttempt, SourceError> {
    let before = fs
        .metadata(path)
        .map_err(|source| SourceError::io(path, source))?;
    if before.kind != FsKind::File {
        return Err(SourceError::InvalidPath {
            path: path.to_path_buf(),
        });
    }
    let bytes = fs
        .read_file(path)
        .map_err(|source| SourceError::io(path, source))?;
    let after = match fs.metadata(path) {
        Ok(meta) => meta,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ReadAttempt::Stale(StaleReason::Missing));
        }
        Err(source) => return Err(SourceError::io(path, source)),
    };
    let before_stamp = stamp_of(&before);
    let after_stamp = stamp_of(&after);
    let size_matches = u64::try_from(bytes.len()).ok() == Some(after.size);
    if after.kind != FsKind::File || before_stamp != after_stamp || !size_matches {
        return Ok(ReadAttempt::Stale(StaleReason::Race));
    }
    let digest = Sha256::digest(&bytes)?;
    Ok(ReadAttempt::Stable(StableRead {
        stamp: after_stamp,
        digest,
        bytes,
    }))
}

fn stamp_of(meta: &EntryMeta) -> FileStamp {
    FileStamp::new(meta.size, meta.modified_ns, meta.inode)
}
