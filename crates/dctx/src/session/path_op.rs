// SPDX-License-Identifier: Apache-2.0
//! The path verb: the shortest edge path between two nodes, written as one bounded page.
//! Implements the `dctx map path` read over the pinned generation; it has no cursor.
//! Depends on the index path search and the same limit resolution as the five verbs.
//! A path that does not fit the budget is refused, never cut, because half a path says nothing.

use super::Session;
use crate::domain::{BoundedSink, DctxError, MapRequest, within_budget};
use crate::index::hubs::PageHead;
use crate::index::path::{encode, shortest_path};

impl Session {
    /// Writes one path page for `handle`: the hops from node `from` to node `to`, or `no path` within 8 edges.
    /// The caps come from `request.limits` and the format from `request.output_format` (1 text, 2 JSON).
    /// The request verb only picks the caps: pass [`crate::domain::Verb::Neighbors`]. Its target is ignored.
    /// # Errors
    /// Refuses invalid/stale handles or requests, an id that names no node, a budget too small for the page, or sink failures.
    pub fn shortest_path(
        &mut self,
        handle: &super::SnapshotHandle,
        from: &str,
        to: &str,
        request: &MapRequest,
        sink: &mut dyn BoundedSink,
    ) -> Result<(), DctxError> {
        self.reject_transport(request)?;
        let resolved = self.query_limits(request)?;
        let generation = self.generation(handle)?;
        let hops = shortest_path(&generation, from, to)
            .map_err(|id| DctxError::invalid_detail("unknown id", &id))?;
        let age_s = self
            .clock
            .now()
            .millis()
            .saturating_sub(generation.taken_at)
            / 1000;
        let state = if generation.freshness == crate::domain::FreshnessMode::Metadata {
            "metadata"
        } else {
            "verified"
        };
        let head = PageHead {
            place: &self.place,
            snapshot: &generation.digest,
            generation: generation.id,
            mode: generation.freshness.as_str(),
            state,
            age_s,
            coverage: &generation.coverage,
            completion: &generation.completion,
            parser: &generation.parser,
            meta: request.meta,
        };
        let encoded = encode(&head, hops.as_deref(), request.output_format != 1).into_bytes();
        if !within_budget(&encoded, &resolved) {
            let minimum_bytes = u64::try_from(encoded.len()).unwrap_or(u64::MAX).max(1);
            return Err(DctxError::BudgetTooSmall { minimum_bytes });
        }
        sink.write_page(&encoded)
    }
}
