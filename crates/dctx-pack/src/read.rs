// SPDX-License-Identifier: Apache-2.0
//! Verified, on-demand reads of files in a persisted pack manifest.
//! History contains identities and metadata; the source owns every content byte.

use dctx_core::domain::FileHandlingDecision;
use dctx_core::error::DctxError;
use dctx_core::facts::{PackSnapshot, ReadGrant};
use dctx_core::filter::{has_git_segment, is_secret_shaped_name};
use dctx_core::ids::RelativePath;
use dctx_hash::Sha256;
use dctx_ports::{Cancellation, Source};

/// Reads one manifest file from the current source and verifies its complete content identity.
/// Files without a recorded digest, ignored files and sensitive paths cannot be read.
/// No cached body is used, including when the caller supplies a freshly scanned snapshot.
///
/// # Errors
/// Returns `PolicyDenied` for unauthorized paths, `SourceChanged` for hash or size drift,
/// and propagates source, cancellation and byte-limit errors. Missing files remain source errors.
pub fn read_snapshot_file(
    snapshot: &PackSnapshot,
    path: &RelativePath,
    source: &dyn Source,
    cancel: &dyn Cancellation,
    max_bytes: u64,
) -> Result<Vec<u8>, DctxError> {
    cancel.checkpoint()?;
    if has_git_segment(path.as_str()) || is_secret_shaped_name(path.name()) {
        return Err(DctxError::PolicyDenied);
    }
    let expected = snapshot
        .digests()
        .get(path.as_str())
        .ok_or(DctxError::PolicyDenied)?;
    let mut stack = vec![(snapshot.tree(), true)];
    let mut authorized = false;
    while let Some((node, ancestors)) = stack.pop() {
        let readable =
            ancestors && !node.is_ignored && node.handling_decision.should_read_content();
        if node.relative_path == path.as_str() {
            authorized = readable && node.node_type == dctx_core::datatypes::NodeType::File;
            break;
        }
        stack.extend(node.children.iter().map(|child| (child, readable)));
    }
    if !authorized {
        return Err(DctxError::PolicyDenied);
    }
    let grant = ReadGrant::authorize(path, FileHandlingDecision::Include, true)
        .ok_or(DctxError::PolicyDenied)?;
    let bytes = source.read(&grant, max_bytes)?;
    cancel.checkpoint()?;
    let actual = Sha256::digest(&bytes).map_err(|_| DctxError::SourceChanged)?;
    if actual != *expected {
        return Err(DctxError::SourceChanged);
    }
    Ok(bytes)
}
