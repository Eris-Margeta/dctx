//! `Source` port: confined, metadata-first access to one source root.
//! Implements contracts/API-PORTS-v1.md `Source.inventory/open/read`: listing returns names and
//! kinds only, metadata is a separate call, and content needs a core [`ReadGrant`] (P1).
//! Depends on dctx-core facts and ids. Implemented by adapters (fs later, memory here); used by dctx-pack.

use dctx_core::datatypes::NodeType;
use dctx_core::error::DctxError;
use dctx_core::facts::{EntryMeta, ReadGrant, SourceEntry};
use dctx_core::ids::RelativePath;

/// Read access to one source root. Paths are root-relative and can never escape the root.
pub trait Source {
    /// Kind of the root itself; `SourceUnavailable` if it does not exist.
    fn root_kind(&self) -> Result<NodeType, DctxError>;

    /// Children of directory `dir`, names and kinds only, in any order.
    fn entries(&self, dir: &RelativePath) -> Result<Vec<SourceEntry>, DctxError>;

    /// Size and modification time of `path`.
    fn metadata(&self, path: &RelativePath) -> Result<EntryMeta, DctxError>;

    /// At most `max_bytes` of the granted file's content. An adapter returns
    /// `ResourceLimit` rather than a silently truncated prefix.
    fn read(&self, grant: &ReadGrant, max_bytes: u64) -> Result<Vec<u8>, DctxError>;
}

impl<S: Source + ?Sized> Source for &S {
    fn root_kind(&self) -> Result<NodeType, DctxError> {
        (**self).root_kind()
    }

    fn entries(&self, dir: &RelativePath) -> Result<Vec<SourceEntry>, DctxError> {
        (**self).entries(dir)
    }

    fn metadata(&self, path: &RelativePath) -> Result<EntryMeta, DctxError> {
        (**self).metadata(path)
    }

    fn read(&self, grant: &ReadGrant, max_bytes: u64) -> Result<Vec<u8>, DctxError> {
        (**self).read(grant, max_bytes)
    }
}
