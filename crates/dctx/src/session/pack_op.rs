//! Pack as a library operation. The engine is `dctx-pack`; this module only builds the report.
//! Implements contracts/API-PORTS-v1.md `Session.pack`. One publish is the commit point.
//! Depends on [`super::Session`] and `dctx-pack`. A git revision is refused before the scan.
//! Hook methods run before the engine and after the page is written. The report counts diagnostics.

use dctx_core::budget::PackLimits;
use dctx_core::datatypes::{DirContextTreeNode, NodeType};
use dctx_core::ids::ViewKey;
use dctx_pack::{pack, PackPorts, PackRequest as EngineRequest};

use super::Session;
use crate::domain::{BoundedSink, PackReport, PackRequest, DctxError, REQUEST_VERSION};

impl Session {
    /// Runs pack and writes one page. A budget under 64 bytes is refused before publish.
    pub fn pack(&mut self, request: &PackRequest, sink: &mut dyn BoundedSink) -> Result<PackReport, DctxError> {
        if !cfg!(feature = "pack") {
            return Err(DctxError::unsupported("pack"));
        }
        if request.request_version != REQUEST_VERSION {
            return Err(DctxError::UnsupportedVersion { found: request.request_version });
        }
        reject_format(request.output_format)?;
        reject_git(request.git_revision.as_deref())?;
        if request.limits.encoded_bytes.is_some_and(|bytes| bytes < 64) {
            return Err(DctxError::BudgetTooSmall { minimum_bytes: 64 });
        }
        self.cancel.checkpoint().map_err(DctxError::from_core)?;
        self.hooks_before_pack()?;
        let view_name = if request.view.is_empty() { self.view.clone() } else { request.view.clone() };
        let view = ViewKey::parse(&view_name).map_err(DctxError::from_core)?;
        let scan = crate::domain::resolve_scan(&self.limits.intersect(request.limits))?;
        let limits = PackLimits {
            max_entries: scan.entries,
            max_depth: scan.depth,
            max_file_bytes: scan.file_bytes,
            max_source_bytes: scan.source_bytes,
        };
        let engine = pack(
            PackPorts {
                source: self.source.as_ref(),
                store: self.store.as_mut(),
                cache: &mut self.cache,
                clock: self.clock.as_ref(),
                cancel: self.cancel.as_ref(),
            },
            &EngineRequest { view, policy: &self.selection, debug_ignored: false, max_diff_files: 100_000, limits },
        )
        .map_err(DctxError::from_core)?;
        let changes = engine.diff.stats.files_added + engine.diff.stats.files_removed + engine.diff.stats.files_modified;
        let public = PackReport {
            version: engine.snapshot.version().to_string(),
            previous: engine.previous_version.map(|version| version.to_string()),
            published: engine.published,
            major_reset: engine.major_reset,
            digest: engine.snapshot.digest().to_string(),
            files: file_count(engine.snapshot.tree()),
            changes: u64::try_from(changes).unwrap_or(u64::MAX),
            diagnostics: u64::try_from(engine.diagnostics.len()).unwrap_or(u64::MAX),
        };
        let encoded = encode_pack(request.output_format, &public);
        if let Some(limit) = request.limits.encoded_bytes {
            let len = u64::try_from(encoded.len()).unwrap_or(u64::MAX);
            if len > limit {
                return Err(DctxError::BudgetTooSmall { minimum_bytes: len });
            }
        }
        sink.write_page(&encoded)?;
        self.hooks_after_pack()?;
        Ok(public)
    }
}

pub(super) fn reject_format(format: u32) -> Result<(), DctxError> {
    match format {
        1 | 2 => Ok(()),
        other => Err(DctxError::UnsupportedVersion { found: other }),
    }
}

pub(super) fn reject_git(revision: Option<&str>) -> Result<(), DctxError> {
    if revision.is_some() {
        Err(DctxError::unsupported("git"))
    } else {
        Ok(())
    }
}

/// Files in the pack tree. The first engine diff is empty, so the count comes from the tree.
fn file_count(node: &DirContextTreeNode) -> u64 {
    let own = u64::from(node.node_type == NodeType::File);
    node.children.iter().fold(own, |sum, child| sum.saturating_add(file_count(child)))
}

fn encode_pack(format: u32, report: &PackReport) -> Vec<u8> {
    if format == 1 {
        return format!("# pack {} files={} changes={}\n", report.version, report.files, report.changes).into_bytes();
    }
    let previous = match &report.previous {
        Some(version) => format!("\"{version}\""),
        None => "null".to_string(),
    };
    format!(
        "{{\"v\":2,\"version\":\"{}\",\"previous\":{previous},\"published\":{},\"major_reset\":{},\"digest\":\"{}\",\"files\":{},\"changes\":{},\"diagnostics\":{}}}\n",
        report.version,
        report.published,
        report.major_reset,
        report.digest,
        report.files,
        report.changes,
        report.diagnostics
    )
    .into_bytes()
}
