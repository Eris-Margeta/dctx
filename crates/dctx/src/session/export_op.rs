// SPDX-License-Identifier: Apache-2.0
//! The export verb: the whole graph of the pinned generation as one document.
//! Implements the `dctx map export` read; it has no cursor, no page header and no token cap, because it is an export.
//! Depends on the index export encoder only.

use super::Session;
use crate::domain::{DctxError, MapRequest};
use crate::index::export::{export_graph, ExportFormat};

impl Session {
    /// Returns every node and every edge of `handle`'s generation as `json` or `mermaid`, sorted so equal generations give equal bytes.
    /// The request only carries the transport checks (version, format, revision, cancellation); its limits are not applied.
    /// # Errors
    /// Refuses invalid/stale handles or requests and a format other than `json` or `mermaid`.
    pub fn export_graph(&mut self, handle: &super::SnapshotHandle, format: &str, request: &MapRequest) -> Result<String, DctxError> {
        self.reject_transport(request)?;
        let format = ExportFormat::parse(format).map_err(|_| DctxError::invalid_detail("unknown export format", format))?;
        let generation = self.generation(handle)?;
        Ok(export_graph(&generation, format))
    }
}
