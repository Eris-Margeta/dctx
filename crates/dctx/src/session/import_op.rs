//! Explicit legacy import through FactStore (D3-LEGACY-REMOVAL §5 D3-02).
//! Depends on dctx-pack's bounded codec and the session's injected store/cache/clock.
//! Hosts supply immutable bytes and an empty destination view. No discovery or cleanup runs.

#![forbid(unsafe_code)]

use std::collections::BTreeSet;

use dctx_core::facts::{ContentDigests, PackSnapshot};
use dctx_core::ids::{RelativePath, ViewKey};
use dctx_hash::Sha256;
use dctx_pack::{LegacyBodies, LegacyLimits, LegacyNode, decode_legacy};
use dctx_ports::Cache;

use super::Session;
use crate::domain::{DctxError, Resource, check_deadline, resolve_scan};

impl Session {
    /// Import caller-loaded legacy bytes into an explicitly named, empty pack view.
    /// Existing views are refused; input, other views and exports are never changed.
    /// Selection rules are not reapplied to historical facts and no source is read.
    /// `MetadataOnly` keeps bodies and digests absent; `Include` treats even legacy
    /// zero padding as bytes, since the wire format has no omission marker.
    /// One FactStore publish is the commit point, as in `pack`.
    pub fn import_legacy(
        &mut self,
        bytes: &[u8],
        destination: &str,
        bodies: LegacyBodies,
        mut limits: LegacyLimits,
    ) -> Result<PackSnapshot, DctxError> {
        if !cfg!(feature = "pack") {
            return Err(DctxError::unsupported("pack"));
        }
        let view = ViewKey::parse(destination).map_err(DctxError::from_core)?;
        self.cancel.checkpoint().map_err(DctxError::from_core)?;
        let started = self.clock.now().0;
        check_deadline(started, started, self.limits.deadline_ms)?;
        if self.store.load(&view).map_err(DctxError::from_core)?.is_some() {
            return Err(DctxError::invalid("legacy import destination already exists"));
        }
        let caps = resolve_scan(&self.limits)?;
        limits.max_total_bytes = cap(limits.max_total_bytes, caps.source_bytes);
        limits.max_nodes = cap(limits.max_nodes, caps.entries);
        limits.max_depth = cap(limits.max_depth, caps.depth);
        if let Some(memory) = self.limits.memory_bytes {
            limits.max_total_bytes = cap(limits.max_total_bytes, memory);
        }
        let document = decode_legacy(bytes, bodies, limits)
            .map_err(|error| DctxError::invalid_detail("legacy import", &error.to_string()))?;
        let digests = import_digests(&document.root, caps.file_bytes)?;
        let snapshot = PackSnapshot::new(document.root, document.header.version, self.clock.now(), digests)
            .map_err(DctxError::from_core)?;
        let stored_bytes = u64::try_from(bytes.len()).map_err(|_| DctxError::invalid("legacy import size"))?;
        let reservation = self.cache.reserve(stored_bytes).map_err(DctxError::from_core)?;
        let ready = self
            .cancel
            .checkpoint()
            .map_err(DctxError::from_core)
            .and_then(|()| check_deadline(started, self.clock.now().0, self.limits.deadline_ms));
        if let Err(error) = ready {
            self.cache.abort(reservation);
            return Err(error);
        }
        if let Err(error) = self.store.publish(&view, &snapshot) {
            self.cache.abort(reservation);
            return Err(DctxError::from_core(error));
        }
        self.cache.commit(reservation, stored_bytes).map_err(DctxError::from_core)?;
        Ok(snapshot)
    }
}

fn cap(requested: usize, maximum: u64) -> usize {
    requested.min(usize::try_from(maximum).unwrap_or(usize::MAX))
}

fn import_digests(root: &LegacyNode, file_limit: u64) -> Result<ContentDigests, DctxError> {
    let mut digests = ContentDigests::new();
    let mut paths = BTreeSet::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        RelativePath::parse(&node.relative_path).map_err(DctxError::from_core)?;
        if !paths.insert(&node.relative_path) {
            return Err(DctxError::invalid("duplicate legacy path"));
        }
        if node.content_size > file_limit {
            return Err(DctxError::ResourceLimit { resource: Resource::FileBytes, limit: file_limit });
        }
        if let Some(bytes) = &node.file_content {
            let digest = Sha256::digest(bytes).map_err(|_| DctxError::invalid("legacy digest size"))?;
            digests.insert(node.relative_path.clone(), digest);
        }
        stack.extend(&node.children);
    }
    Ok(digests)
}
