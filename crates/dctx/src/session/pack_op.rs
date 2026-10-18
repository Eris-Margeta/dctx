//! Pack as a library operation. The engine is `dctx-pack`; this module only builds the report.
//! Implements contracts/API-PORTS-v1.md `Session.pack`. One publish is the commit point.
//! Depends on [`super::Session`] and `dctx-pack`. A git revision is refused before the scan.
//! Hook methods run before the engine and after the page is written. The report counts diagnostics.

use dctx_core::budget::PackLimits;
use dctx_core::datatypes::{DirContextTreeNode, NodeType};
use dctx_core::diff::compare_trees;
use dctx_core::ids::ViewKey;
use dctx_pack::{pack, scan, PackPorts, PackRequest as EngineRequest, ScanOptions};
use dctx_template::{render_legacy_diff, render_legacy_snapshot, LegacyLayout, MarkerNode};

const MAX_DIFF_FILES: u32 = 100_000;

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
        let effective_limits = self.limits.intersect(request.limits);
        if effective_limits.encoded_bytes.is_some_and(|bytes| bytes < 64) {
            return Err(DctxError::BudgetTooSmall { minimum_bytes: 64 });
        }
        self.cancel.checkpoint().map_err(DctxError::from_core)?;
        self.hooks_before_pack()?;
        self.touch_cache_root()?;
        let view_name = if request.view.is_empty() { self.view.clone() } else { request.view.clone() };
        let view = ViewKey::parse(&view_name).map_err(DctxError::from_core)?;
        let scan = crate::domain::resolve_scan(&effective_limits)?;
        let limits = PackLimits {
            max_entries: scan.entries,
            max_depth: scan.depth,
            max_file_bytes: scan.file_bytes,
            max_source_bytes: scan.source_bytes,
        };
        self.refuse_exhausted_revision(&view, request, limits)?;
        let engine = pack(
            PackPorts {
                source: self.source.as_ref(),
                store: self.store.as_mut(),
                cache: &mut self.cache,
                clock: self.clock.as_ref(),
                cancel: self.cancel.as_ref(),
            },
            &EngineRequest {
                view,
                policy: &self.selection,
                debug_ignored: request.legacy_output.as_ref().is_some_and(|output| output.debug_ignored),
                max_diff_files: MAX_DIFF_FILES,
                limits,
            },
        )
        .map_err(DctxError::from_core)?;
        let changes = engine.diff.stats.files_added + engine.diff.stats.files_removed + engine.diff.stats.files_modified;
        let mut public = PackReport {
            version: engine.snapshot.version().to_string(),
            previous: engine.previous_version.as_ref().map(|version| version.to_string()),
            published: engine.published,
            major_reset: engine.major_reset,
            digest: engine.snapshot.digest().to_string(),
            files: file_count(engine.snapshot.tree()),
            changes: u64::try_from(changes).unwrap_or(u64::MAX),
            diagnostics: u64::try_from(engine.diagnostics.len()).unwrap_or(u64::MAX),
            legacy_layout: None,
        };
        let encoded = encode_output(request, &engine, &mut public, effective_limits.encoded_bytes)?;
        if let Some(limit) = effective_limits.encoded_bytes {
            let len = u64::try_from(encoded.len()).unwrap_or(u64::MAX);
            if len > limit {
                return Err(DctxError::BudgetTooSmall { minimum_bytes: len });
            }
        }
        sink.write_page(&encoded)?;
        self.hooks_after_pack()?;
        Ok(public)
    }

    /// A stored `u32::MAX` major cannot survive another major reset. Refuse before `pack` publishes.
    fn refuse_exhausted_revision(
        &self,
        view: &ViewKey,
        request: &PackRequest,
        limits: PackLimits,
    ) -> Result<(), DctxError> {
        let Some(previous) = self.store.load(view).map_err(DctxError::from_core)? else {
            return Ok(());
        };
        if previous.version().major != u32::MAX {
            return Ok(());
        }
        let scanned = scan(
            self.source.as_ref(),
            self.cancel.as_ref(),
            ScanOptions {
                policy: &self.selection,
                debug_ignored: request.legacy_output.as_ref().is_some_and(|output| output.debug_ignored),
                limits,
            },
        )
        .map_err(DctxError::from_core)?;
        let report = compare_trees(Some(previous.tree()), &scanned.tree);
        if report.has_changes
            && previous
                .version()
                .try_calculate_next(report.stats.files_unchanged, report.stats.total_files_in_new_state, MAX_DIFF_FILES)
                .is_err()
        {
            return Err(DctxError::invalid("version exhausted"));
        }
        Ok(())
    }
}

fn encode_output(
    request: &PackRequest,
    engine: &dctx_pack::PackReport,
    report: &mut PackReport,
    limit: Option<u64>,
) -> Result<Vec<u8>, DctxError> {
    let Some(output) = &request.legacy_output else {
        return Ok(encode_pack(request.output_format, report));
    };
    let tree = marker_tree(engine.snapshot.tree());
    let version = engine.snapshot.version();
    if output.layout == LegacyLayout::Diff && !engine.major_reset && let Some(previous) = &engine.previous_version {
        report.legacy_layout = Some(LegacyLayout::Diff);
        let text = match limit {
            Some(limit) => output
                .template
                .legacy_diff_within(&tree, &engine.diff, previous, version, output.omit_svg_content, limit)
                .map_err(DctxError::from_template)?,
            None => render_legacy_diff(&output.template, &tree, &engine.diff, previous, version, output.omit_svg_content),
        };
        return Ok(text.into_bytes());
    }
    let tree_only = output.layout == LegacyLayout::TreeOnly;
    report.legacy_layout = Some(if tree_only { LegacyLayout::TreeOnly } else { LegacyLayout::Snapshot });
    let text = match limit {
        Some(limit) => output
            .template
            .legacy_snapshot_within(&tree, version, tree_only, output.omit_svg_content, limit)
            .map_err(DctxError::from_template)?,
        None => render_legacy_snapshot(&output.template, &tree, version, tree_only, output.omit_svg_content),
    };
    Ok(text.into_bytes())
}

fn marker_tree(node: &DirContextTreeNode) -> MarkerNode {
    let omitted = node.handling_decision == dctx_core::FileHandlingDecision::OmitContent;
    let ignored = node.is_ignored || !node.handling_decision.should_read_content() && !omitted;
    let binary_label = if node.is_binary_file {
        Some(node.binary_file_type.clone().unwrap_or_else(|| "Unknown".to_string()))
    } else {
        node.binary_file_type.clone().filter(|label| label == "SVG file")
    };
    MarkerNode {
        relative_path: node.relative_path.clone(),
        is_dir: node.node_type == NodeType::Directory,
        is_ignored: ignored,
        omit_content: omitted,
        binary_label,
        content: if ignored || omitted || node.is_binary_file { None } else { node.file_content.clone() },
        content_size: node.content_size,
        modified: node.last_modified_timestamp,
        children: node.children.iter().map(marker_tree).collect(),
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
