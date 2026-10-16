//! Read spy around an injected filesystem.
//! Implements the DX-B3 evidence that ignored and omitted paths are never opened.
//! Depends on [`crate::ports`]. Tests and callers inspect [`SpyFs::read_files`].

use crate::ports::{DirEntry, EntryMeta, FsOp, SourceFs};
use std::{cell::RefCell, io, path::PathBuf};

/// Records every port call, then delegates.
#[derive(Debug)]
pub struct SpyFs<F> {
    inner: F,
    operations: RefCell<Vec<FsOp>>,
}

impl<F> SpyFs<F> {
    /// Wraps `inner` with an empty log.
    pub fn new(inner: F) -> Self {
        Self {
            inner,
            operations: RefCell::new(Vec::new()),
        }
    }

    /// Returns calls in order, including unsuccessful attempts.
    pub fn operations(&self) -> Vec<FsOp> {
        self.operations.borrow().clone()
    }

    /// Content paths passed to [`SourceFs::read_file`].
    pub fn read_files(&self) -> Vec<PathBuf> {
        self.operations()
            .into_iter()
            .filter_map(|operation| match operation {
                FsOp::ReadFile(path) => Some(path),
                _ => None,
            })
            .collect()
    }

    /// Clears the log without touching the inner filesystem.
    pub fn clear(&self) {
        self.operations.borrow_mut().clear();
    }

    fn record(&self, operation: FsOp) {
        self.operations.borrow_mut().push(operation);
    }
}

impl<F: SourceFs> SourceFs for SpyFs<F> {
    fn metadata(&self, path: &std::path::Path) -> io::Result<EntryMeta> {
        self.record(FsOp::Metadata(path.to_path_buf()));
        self.inner.metadata(path)
    }

    fn read_dir(&self, path: &std::path::Path) -> io::Result<Vec<DirEntry>> {
        self.record(FsOp::ReadDir(path.to_path_buf()));
        self.inner.read_dir(path)
    }

    fn read_link(&self, path: &std::path::Path) -> io::Result<PathBuf> {
        self.record(FsOp::ReadLink(path.to_path_buf()));
        self.inner.read_link(path)
    }

    fn read_file(&self, path: &std::path::Path) -> io::Result<Vec<u8>> {
        self.record(FsOp::ReadFile(path.to_path_buf()));
        self.inner.read_file(path)
    }

    fn read_policy(&self, path: &std::path::Path) -> io::Result<Option<String>> {
        self.record(FsOp::ReadPolicy(path.to_path_buf()));
        self.inner.read_policy(path)
    }
}
