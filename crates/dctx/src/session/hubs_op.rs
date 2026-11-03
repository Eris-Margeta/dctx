// SPDX-License-Identifier: Apache-2.0
//! The hubs verb: most-called symbols and per-directory counts, written as one bounded page.
//! Implements the `dctx map hubs` read over the pinned generation; it needs no target and has no cursor.
//! Depends on the index hub counts and the same limit resolution as the five verbs.
//! Over a budget the page drops hub lines and directory lines until it fits, or is refused when nothing fits.

use super::Session;
use crate::domain::{within_budget, BoundedSink, DctxError, MapRequest};
use crate::index::hubs::{collect_hubs, encode, PageHead};

impl Session {
    /// Writes one hubs page for `handle`. The caps come from `request.limits` (records are hub lines, at most 60) and the format from `request.output_format` (1 text, 2 JSON).
    /// The request verb only picks the caps: pass [`crate::domain::Verb::Neighbors`]. Its target is ignored.
    /// # Errors
    /// Refuses invalid/stale handles or requests, unsupported formats, a budget too small for one line, or sink failures.
    pub fn hubs(&mut self, handle: &super::SnapshotHandle, request: &MapRequest, sink: &mut dyn BoundedSink) -> Result<(), DctxError> {
        self.reject_transport(request)?;
        let resolved = self.query_limits(request)?;
        let generation = self.generation(handle)?;
        let age_s = self.clock.now().millis().saturating_sub(generation.taken_at) / 1000;
        let state = if generation.freshness == crate::domain::FreshnessMode::Metadata { "metadata" } else { "verified" };
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
        let (mut hubs, mut dirs) = collect_hubs(&generation);
        hubs.truncate(usize::try_from(resolved.records).unwrap_or(usize::MAX));
        let json = request.output_format != 1;
        loop {
            let encoded = encode(&head, &hubs, &dirs, json).into_bytes();
            if within_budget(&encoded, &resolved) {
                sink.write_page(&encoded)?;
                return Ok(());
            }
            if dirs.len() > hubs.len() {
                dirs.pop();
            } else if hubs.pop().is_none() {
                let minimum_bytes = u64::try_from(encoded.len()).unwrap_or(u64::MAX).max(1);
                return Err(DctxError::BudgetTooSmall { minimum_bytes });
            }
        }
    }
}
