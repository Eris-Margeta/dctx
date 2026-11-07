// SPDX-License-Identifier: Apache-2.0
//! Scoped query admission: the ordinary walk and verifier, without cache publication.

use super::{Session, SnapshotHandle};
use crate::domain::{DctxError, MapRequest, SnapshotRequest, Verb};
use crate::index::{Generation, IdIndex, WalkArgs, walk::walk_scope};

impl Session {
    /// Prepares only a path-bearing request's source scope. Other requests use reusable snapshots.
    /// Requests with `meta` retain whole-snapshot identity through the reusable path.
    /// Scoped handles accept only file-local queries over their original scope.
    /// Scoped generations stay in memory and never require store or cache admission.
    /// The same handle/query API and source verification apply to both paths.
    /// # Errors
    /// Propagates request, policy, source, cancellation and scan-limit errors.
    pub fn snapshot_for_query(
        &mut self,
        snapshot: &SnapshotRequest,
        request: &MapRequest,
        source_key: &str,
    ) -> Result<SnapshotHandle, DctxError> {
        self.reject_transport(request)?;
        request.validate()?;
        let Some(scope) = request.source_scope() else {
            return self.snapshot_reusable_for_profile(snapshot, source_key);
        };
        let (limits, parser, operation) = self.reject_snapshot(snapshot)?;
        self.hooks_before_index()?;
        let started = self.clock.now().millis();
        let elapsed = std::time::Instant::now();
        let walked = walk_scope(
            &WalkArgs {
                source: self.source.as_ref(),
                cancel: self.cancel.as_ref(),
                policy: self.policy_view(),
                limits: &limits,
                freshness: snapshot.freshness,
                parser,
                deadline_ms: operation.deadline_ms,
                started_ms: started,
                now_ms: started,
                elapsed: Some(elapsed),
            },
            Some((scope, !matches!(request.verb, Verb::Find | Verb::Grep))),
        )?;
        self.content_reads += walked.reads;
        let generation = Generation {
            id: self.next_generation,
            digest: String::new(),
            encoded_len: 0,
            files: walked.files,
            nodes: walked.nodes,
            edges: walked.edges,
            text: walked.text,
            freshness: snapshot.freshness,
            taken_at: started,
            epoch: self.epoch,
            coverage: walked.coverage,
            completion: walked.completion,
            parser: walked.parser,
            ids: IdIndex::default(),
        };
        let mut generation = crate::index::scope::project(&generation, scope)?;
        self.cancel.checkpoint().map_err(DctxError::from_core)?;
        let millis = u64::try_from(elapsed.elapsed().as_millis()).unwrap_or(u64::MAX);
        crate::domain::check_deadline(
            started,
            started.saturating_add(millis),
            operation.deadline_ms,
        )?;
        generation.id = self.next_generation;
        let mut handle = self.pin_generation(generation, None, false);
        handle.scope = Some(scope.to_string());
        self.hooks_after_index()?;
        Ok(handle)
    }
}
