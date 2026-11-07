// SPDX-License-Identifier: Apache-2.0
//! `Source` port: confined, metadata-first access to one source root.
//! Implements contracts/API-PORTS-v1.md `Source.inventory/open/read`: listing returns names and
//! kinds only, metadata is a separate call, and content needs a core [`ReadGrant`] (P1).
//! Depends on dctx-core facts and ids. Implemented by adapters (fs later, memory here); used by dctx-pack.

use dctx_core::datatypes::NodeType;
use dctx_core::error::DctxError;
use dctx_core::facts::{EntryMeta, EntryStamp, ReadGrant, SourceEntry};
use dctx_core::ids::RelativePath;

/// Read access to one source root. Paths are root-relative and can never escape the root.
pub trait Source {
    /// Kind of the root itself; `SourceUnavailable` if it does not exist.
    ///
    /// # Errors
    /// Returns `SourceUnavailable` for a missing root, or an adapter error
    /// when the root cannot be inspected.
    fn root_kind(&self) -> Result<NodeType, DctxError>;

    /// Children of directory `dir`, names and kinds only, in any order.
    ///
    /// # Errors
    /// Returns an adapter error if the directory is unavailable, cannot be
    /// listed, or violates the source confinement policy.
    fn entries(&self, dir: &RelativePath) -> Result<Vec<SourceEntry>, DctxError>;

    /// Kind of one address without reading its content. Filesystem adapters should
    /// override this with a confined stat; the default inspects the parent's names.
    /// # Errors
    /// Returns source/confinement failures or `SourceUnavailable` for a missing address.
    fn kind(&self, path: &RelativePath) -> Result<NodeType, DctxError> {
        if path.is_root() {
            return self.root_kind();
        }
        let parent = path
            .as_str()
            .rsplit_once('/')
            .map_or("", |(parent, _)| parent);
        let parent = RelativePath::parse(parent)?;
        self.entries(&parent)?
            .into_iter()
            .find(|entry| entry.name == path.name())
            .map(|entry| entry.kind)
            .ok_or(DctxError::SourceUnavailable)
    }

    /// Size and modification time of `path`.
    ///
    /// # Errors
    /// Returns an adapter error if the path is unavailable, cannot be
    /// inspected, or violates the source confinement policy.
    fn metadata(&self, path: &RelativePath) -> Result<EntryMeta, DctxError>;

    /// Size and modification time of `path`, plus the stat identity a caller may use to skip hashing
    /// an unchanged file. The stamp is `None` when the adapter cannot vouch for it: no inode or
    /// nanosecond clock, or a file modified so recently (racy window) that an edit could keep the
    /// same stamp. A caller that gets `None` must hash. The default says `None`.
    ///
    /// # Errors
    /// Same as [`Source::metadata`].
    fn stat(&self, path: &RelativePath) -> Result<(EntryMeta, Option<EntryStamp>), DctxError> {
        Ok((self.metadata(path)?, None))
    }

    /// At most `max_bytes` of the granted file's content. An adapter returns
    /// `ResourceLimit` rather than a silently truncated prefix.
    ///
    /// # Errors
    /// Returns `ResourceLimit` if the content exceeds `max_bytes`, or an
    /// adapter error if the granted file is unavailable or cannot be read.
    fn read(&self, grant: &ReadGrant, max_bytes: u64) -> Result<Vec<u8>, DctxError>;
}

impl<S: Source + ?Sized> Source for &S {
    fn root_kind(&self) -> Result<NodeType, DctxError> {
        (**self).root_kind()
    }

    fn entries(&self, dir: &RelativePath) -> Result<Vec<SourceEntry>, DctxError> {
        (**self).entries(dir)
    }

    fn kind(&self, path: &RelativePath) -> Result<NodeType, DctxError> {
        (**self).kind(path)
    }

    fn metadata(&self, path: &RelativePath) -> Result<EntryMeta, DctxError> {
        (**self).metadata(path)
    }

    fn stat(&self, path: &RelativePath) -> Result<(EntryMeta, Option<EntryStamp>), DctxError> {
        (**self).stat(path)
    }

    fn read(&self, grant: &ReadGrant, max_bytes: u64) -> Result<Vec<u8>, DctxError> {
        (**self).read(grant, max_bytes)
    }
}
