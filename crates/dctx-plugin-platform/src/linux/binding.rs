//! Handle-relative Linux resource binding, implementing PL-R bindings and S-FILE.
//! openat2 refuses symlinks, magic links and mount transitions in one resolution.
//! Object metadata detects namespace substitution; only a checked sealed digest binds bytes.

use crate::{PlatformError, RelativePath, Unsupported};
use dctx_hash::ContentDigest;
use rustix::fs::{FileType, Mode, OFlags, ResolveFlags, Stat, fstat, openat2};
use std::fs::File;

use super::SealedSnapshot;

/// Metadata precondition, deliberately not described as an immutable content version.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObjectIdentity {
    device: u64,
    inode: u64,
    size: u64,
    changed: (i64, i64),
    modified: (i64, i64),
    mode: u32,
}

impl ObjectIdentity {
    #[allow(clippy::cast_possible_wrap, reason = "Linux stat nanosecond fields are in 0..1_000_000_000, which fits i64.")]
    fn checked(stat: &Stat) -> Result<Self, PlatformError> {
        if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile || stat.st_nlink != 1 {
            return Err(PlatformError::Conflict);
        }
        Ok(Self {
            device: stat.st_dev,
            inode: stat.st_ino,
            size: stat
                .st_size
                .try_into()
                .map_err(|_| PlatformError::Conflict)?,
            changed: (stat.st_ctime, stat.st_ctime_nsec as i64),
            modified: (stat.st_mtime, stat.st_mtime_nsec as i64),
            mode: stat.st_mode,
        })
    }
}

/// Host-opened project/install root retained by descriptor, never by prefix comparison.
#[derive(Debug)]
pub struct RootHandle {
    directory: File,
}

impl RootHandle {
    /// Binds the already authorized directory object. Caller establishes its project identity.
    /// No path lookup or ambient root discovery takes place here.
    ///
    /// # Errors
    /// Returns an IO error if descriptor metadata cannot be read, or a conflict if the descriptor is not a directory.
    pub fn new(directory: File) -> Result<Self, PlatformError> {
        let stat = fstat(&directory).map_err(|_| PlatformError::Io)?;
        if FileType::from_raw_mode(stat.st_mode) != FileType::Directory {
            return Err(PlatformError::Conflict);
        }
        Ok(Self { directory })
    }

    fn open(&self, path: &RelativePath, flags: OFlags) -> Result<File, PlatformError> {
        let resolve = ResolveFlags::BENEATH
            | ResolveFlags::NO_SYMLINKS
            | ResolveFlags::NO_MAGICLINKS
            | ResolveFlags::NO_XDEV;
        openat2(
            &self.directory,
            path.as_str(),
            flags | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::empty(),
            resolve,
        )
        .map(File::from)
        .map_err(|error| match error {
            rustix::io::Errno::NOSYS | rustix::io::Errno::INVAL => {
                PlatformError::Unsupported(Unsupported::PathResolution)
            }
            rustix::io::Errno::LOOP | rustix::io::Errno::XDEV | rustix::io::Errno::AGAIN => {
                PlatformError::Conflict
            }
            _ => PlatformError::Unavailable,
        })
    }

    /// Inspects an authorized name without opening a device for IO or reading source bytes.
    /// Rejects directories, symlinks, special objects and multiply linked files.
    ///
    /// # Errors
    /// Refuses unavailable or unsupported path resolution, unsafe object kinds and conflicting links; propagates metadata IO failures.
    pub fn identity(&self, path: &RelativePath) -> Result<ObjectIdentity, PlatformError> {
        let object = self.open(path, OFlags::PATH)?;
        ObjectIdentity::checked(&fstat(&object).map_err(|_| PlatformError::Io)?)
    }

    /// Copies only an authorized object, checks the authorized digest, then seals the copy.
    /// The host must check selection/consent before this call and again before delivery.
    /// Metadata alone never authorizes an in-place edit; digest mismatch delivers no bytes.
    ///
    /// # Errors
    /// Refuses changed identities, digest mismatches, oversized snapshots, unsupported resolution and IO or sealing failures.
    pub fn snapshot(
        &self,
        path: &RelativePath,
        identity: ObjectIdentity,
        digest: ContentDigest,
    ) -> Result<SealedSnapshot, PlatformError> {
        if digest.byte_length() != identity.size {
            return Err(PlatformError::Conflict);
        }
        if self.identity(path)? != identity {
            return Err(PlatformError::Conflict);
        }
        let object = self.open(path, OFlags::RDONLY | OFlags::NONBLOCK)?;
        if ObjectIdentity::checked(&fstat(&object).map_err(|_| PlatformError::Io)?)? != identity {
            return Err(PlatformError::Conflict);
        }
        let snapshot = SealedSnapshot::copy_checked(&object, digest)?;
        if ObjectIdentity::checked(&fstat(&object).map_err(|_| PlatformError::Io)?)? != identity
            || self.identity(path)? != identity
        {
            return Err(PlatformError::Conflict);
        }
        Ok(snapshot)
    }

    /// Refuses existing-file compare-and-replace before any write.
    /// renameat2 and advisory locks do not implement this contract against hostile writers.
    ///
    /// # Errors
    /// Always returns the unsupported conditional-replace error before writing any bytes.
    pub fn replace(
        &self,
        _path: &RelativePath,
        _identity: ObjectIdentity,
        _bytes: &[u8],
    ) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported(Unsupported::ConditionalReplace))
    }
}
