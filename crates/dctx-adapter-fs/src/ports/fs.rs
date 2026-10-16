//! Operations the Source adapter may ask of a filesystem.
//! Implements DX-B3: content reads are distinct from metadata and rule loads.
//! Depends on `std::path`. [`crate::adapters::StdFs`] and test spies implement it.

use std::{ffi::OsString, io, path::Path};

/// Kind of one directory entry, without following links.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsKind {
    /// Regular file.
    File,
    /// Directory.
    Directory,
    /// Symbolic link. The adapter resolves it lexically and does not follow escapes.
    Symlink,
    /// Socket, device, or other non-source node. Never read.
    Other,
}

/// Metadata available without opening file bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntryMeta {
    /// Kind from symlink-aware metadata.
    pub kind: FsKind,
    /// Byte length reported by the filesystem.
    pub size: u64,
    /// Modification time in nanoseconds since the Unix epoch.
    pub modified_ns: u128,
    /// Inode or file id when the platform provides one.
    pub inode: Option<u64>,
}

/// One child of a listed directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    /// Single path component.
    pub name: OsString,
    /// Kind without following a symlink.
    pub kind: FsKind,
}

/// A recorded port call. Spies use this as evidence of what was opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FsOp {
    /// Symlink-aware metadata.
    Metadata(std::path::PathBuf),
    /// Directory listing.
    ReadDir(std::path::PathBuf),
    /// Symlink target read.
    ReadLink(std::path::PathBuf),
    /// Content open.
    ReadFile(std::path::PathBuf),
    /// Policy text open (ignore or omit rules).
    ReadPolicy(std::path::PathBuf),
}

/// Injected filesystem. Content, policy text, and metadata are separate methods.
pub trait SourceFs {
    /// Reads symlink-aware metadata. Must not follow links.
    fn metadata(&self, path: &Path) -> io::Result<EntryMeta>;
    /// Lists immediate children without reading their bytes.
    fn read_dir(&self, path: &Path) -> io::Result<Vec<DirEntry>>;
    /// Returns the stored link target, not a canonicalized path.
    fn read_link(&self, path: &Path) -> io::Result<std::path::PathBuf>;
    /// Opens a regular file. Must refuse a symlink instead of following it.
    fn read_file(&self, path: &Path) -> io::Result<Vec<u8>>;
    /// Reads rule text. `Ok(None)` means the file is absent. Other errors stay errors.
    fn read_policy(&self, path: &Path) -> io::Result<Option<String>>;
}
