// SPDX-License-Identifier: Apache-2.0
//! Observable filesystem decorator for privacy evidence.
//! Implements SPEC-MAP-MODE section 13a's spy adapter.
//! Records attempts before delegating to an injected filesystem.

use crate::ports::{
    ContentSource,
    fs::{Entry, FileKind, FileSystem},
};
use std::{
    cell::RefCell,
    io,
    path::{Path, PathBuf},
};

/// Attempted filesystem operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Operation {
    /// Content read, including unsuccessful attempts.
    Open(PathBuf),
    /// Directory enumeration.
    List(PathBuf),
    /// Kind inspection.
    Kind(PathBuf),
    /// Size inspection.
    Size(PathBuf),
}
/// Decorates a filesystem with an ordered operation log.
pub struct SpyFileSystem<F> {
    inner: F,
    operations: RefCell<Vec<Operation>>,
}
impl<F> SpyFileSystem<F> {
    /// Wraps an adapter with an empty log.
    pub fn new(inner: F) -> Self {
        Self {
            inner,
            operations: RefCell::new(Vec::new()),
        }
    }
    /// Returns attempted operations in order.
    pub fn operations(&self) -> Vec<Operation> {
        self.operations.borrow().clone()
    }
}
impl<F: ContentSource> ContentSource for SpyFileSystem<F> {
    fn read_content(&self, path: &Path) -> io::Result<Vec<u8>> {
        self.operations
            .borrow_mut()
            .push(Operation::Open(path.into()));
        self.inner.read_content(path)
    }
}
impl<F: FileSystem> FileSystem for SpyFileSystem<F> {
    fn kind(&self, path: &Path) -> io::Result<FileKind> {
        self.operations
            .borrow_mut()
            .push(Operation::Kind(path.into()));
        self.inner.kind(path)
    }
    fn entries(&self, path: &Path) -> io::Result<Vec<Entry>> {
        self.operations
            .borrow_mut()
            .push(Operation::List(path.into()));
        self.inner.entries(path)
    }
    fn size(&self, path: &Path) -> io::Result<u64> {
        self.operations
            .borrow_mut()
            .push(Operation::Size(path.into()));
        self.inner.size(path)
    }
}
