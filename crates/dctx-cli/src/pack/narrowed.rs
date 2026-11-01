//! Narrowed source: drops files the view filters reject before `dctx-pack` can read them.

use dctx::{PackRequest, Source};
use dctx_core::datatypes::NodeType;
use dctx_core::error::DctxError as CoreError;
use dctx_core::facts::{EntryMeta, ReadGrant, SourceEntry};
use dctx_core::ids::RelativePath;

/// Drops files the view filters reject before `dctx-pack` can open them.
/// `DCTX_READ_REPORT=1` prints each content read. The facade wrapper stays quiet.
pub(super) struct NarrowedSource<'a, S> {
    pub(super) inner: S,
    pub(super) request: &'a PackRequest,
    pub(super) report_reads: bool,
}

impl<S: Source> Source for NarrowedSource<'_, S> {
    fn root_kind(&self) -> Result<NodeType, CoreError> {
        self.inner.root_kind()
    }

    fn entries(&self, dir: &RelativePath) -> Result<Vec<SourceEntry>, CoreError> {
        let entries = self.inner.entries(dir)?;
        if !self.request.apply_filters {
            return Ok(entries);
        }
        let mut kept = Vec::with_capacity(entries.len());
        for entry in entries {
            let Ok(path) = dir.child(&entry.name) else {
                kept.push(entry);
                continue;
            };
            if entry.is_dir() {
                if self.request.admits_directory(path.as_str(), &entry.name) {
                    kept.push(entry);
                }
                continue;
            }
            let size = if self.request.filters_need_size() {
                match self.inner.metadata(&path) {
                    Ok(meta) => meta.size,
                    Err(CoreError::Cancelled) => return Err(CoreError::Cancelled),
                    Err(_) => {
                        kept.push(entry);
                        continue;
                    }
                }
            } else {
                0
            };
            if self.request.admits_file(path.as_str(), &entry.name, size) {
                kept.push(entry);
            }
        }
        Ok(kept)
    }

    fn metadata(&self, path: &RelativePath) -> Result<EntryMeta, CoreError> {
        self.inner.metadata(path)
    }

    fn read(&self, grant: &ReadGrant, max_bytes: u64) -> Result<Vec<u8>, CoreError> {
        if self.request.apply_filters {
            let path = grant.path();
            let size = if self.request.filters_need_size() {
                match self.inner.metadata(path) {
                    Ok(meta) => meta.size,
                    Err(CoreError::Cancelled) => return Err(CoreError::Cancelled),
                    Err(_) => {
                        if self.report_reads {
                            eprintln!("read {}", path.as_str());
                        }
                        return self.inner.read(grant, max_bytes);
                    }
                }
            } else {
                0
            };
            if !self.request.admits_file(path.as_str(), path.name(), size) {
                return Err(CoreError::PolicyDenied);
            }
        }
        if self.report_reads {
            eprintln!("read {}", grant.path().as_str());
        }
        self.inner.read(grant, max_bytes)
    }
}
