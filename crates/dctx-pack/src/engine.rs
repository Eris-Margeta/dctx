// SPDX-License-Identifier: Apache-2.0
//! Pack engine: scan → load previous → diff/version → reserve → publish, all through ports.
//! Implements the pack operation of contracts/API-PORTS-v1.md (`Session.pack`): one publish is the
//! commit point; cancellation or a refusal before it leaves the previous snapshot and cache unchanged.
//! Depends on [`crate::snapshot`], [`crate::diff`], dctx-core and every dctx-ports trait. Used by hosts.

use crate::diff::diff_against;
use crate::snapshot::{scan, ScanOptions};
use dctx_core::budget::PackLimits;
use dctx_core::diff::DiffReport;
use dctx_core::error::DctxError;
use dctx_core::facts::{Diagnostic, PackSnapshot};
use dctx_core::ids::ViewKey;
use dctx_core::selection::SelectionPolicy;
use dctx_core::version::Version;
use dctx_ports::{Cache, Cancellation, Clock, FactStore, Source};

/// The ports one pack run uses. Nothing else is reachable from the engine.
pub struct PackPorts<'a> {
    /// Where files come from.
    pub source: &'a dyn Source,
    /// Where snapshots are kept.
    pub store: &'a mut dyn FactStore,
    /// Quota for published bytes.
    pub cache: &'a mut dyn Cache,
    /// Snapshot time stamps.
    pub clock: &'a dyn Clock,
    /// Cooperative cancellation.
    pub cancel: &'a dyn Cancellation,
}

/// One pack request.
#[derive(Debug, Clone)]
pub struct PackRequest<'a> {
    /// The view whose previous snapshot is diffed and replaced.
    pub view: ViewKey,
    /// Selection policy (caller-loaded; the engine never reads rule files).
    pub policy: &'a SelectionPolicy,
    /// Show ignored items as `DebugVisible`.
    pub debug_ignored: bool,
    /// Minor versions before a forced major bump.
    pub max_diff_files: u32,
    /// Scan bounds.
    pub limits: PackLimits,
}

/// Result of a pack run.
#[derive(Debug)]
pub struct PackReport {
    /// The new snapshot (published or identical to the stored one).
    pub snapshot: PackSnapshot,
    /// Version of the previous snapshot, if there was one.
    pub previous_version: Option<Version>,
    /// Changes against the previous snapshot.
    pub diff: DiffReport,
    /// True when the version is a major reset.
    pub major_reset: bool,
    /// True when the snapshot was published (first run or changes).
    pub published: bool,
    /// Non-fatal scan observations.
    pub diagnostics: Vec<Diagnostic>,
}

/// Runs one pack: the only way this crate touches the outside world is through `ports`.
///
/// # Errors
/// Propagates source, cancellation, snapshot-validation, store and cache errors. Failed publication aborts its cache reservation.
#[allow(
    clippy::needless_pass_by_value,
    reason = "Keep the existing public pack signature; the port bundle is passed as the capability set for one operation."
)]
pub fn pack(ports: PackPorts<'_>, request: &PackRequest<'_>) -> Result<PackReport, DctxError> {
    let options = ScanOptions {
        policy: request.policy,
        debug_ignored: request.debug_ignored,
        limits: request.limits,
    };
    let scanned = scan(ports.source, ports.cancel, options)?;
    let previous = ports.store.load(&request.view)?;
    let diff = diff_against(previous.as_ref(), &scanned.tree, request.max_diff_files);
    let snapshot = PackSnapshot::new(
        scanned.tree,
        diff.version,
        ports.clock.now(),
        scanned.digests,
    )?;
    let must_publish = previous.is_none() || diff.report.has_changes;

    if must_publish {
        let reservation = ports.cache.reserve(scanned.content_bytes)?;
        if let Err(error) = ports.cancel.checkpoint() {
            ports.cache.abort(reservation);
            return Err(error);
        }
        if let Err(error) = ports.store.publish(&request.view, &snapshot) {
            ports.cache.abort(reservation);
            return Err(error);
        }
        ports.cache.commit(reservation, scanned.content_bytes)?;
    }

    Ok(PackReport {
        snapshot,
        previous_version: previous.map(|old| old.version().clone()),
        diff: diff.report,
        major_reset: diff.major_reset,
        published: must_publish,
        diagnostics: scanned.diagnostics,
    })
}
