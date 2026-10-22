//! Filesystem boundary for map traversal.
//! Implements SPEC-MAP-MODE §13a without coupling scan to model ids.
//! Adapters supply entry kinds without opening file content.

use super::ContentSource;
use std::{ffi::OsString, io, path::Path};

/// Entry kind; links and special files are never followed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    /// Regular file.
    File,
    /// Directory.
    Directory,
    /// Link or unsupported special file.
    Other,
}
/// A direct child name and its kind.
#[derive(Debug, Clone)]
pub struct Entry {
    /// Single path component.
    pub name: OsString,
    /// Kind without following links.
    pub kind: FileKind,
}
/// Traversal IO, separate from domain matching.
pub trait FileSystem: ContentSource {
    /// Inspects a path without following links.
    ///
    /// # Errors
    /// Returns the underlying metadata failure.
    fn kind(&self, path: &Path) -> io::Result<FileKind>;
    /// Lists immediate children without reading content.
    ///
    /// # Errors
    /// Returns directory-open or entry-enumeration failures.
    fn entries(&self, path: &Path) -> io::Result<Vec<Entry>>;
    /// Returns file size without reading content.
    ///
    /// # Errors
    /// Returns the underlying metadata failure.
    fn size(&self, path: &Path) -> io::Result<u64>;
}
