// SPDX-License-Identifier: Apache-2.0
//! The five verbs. Convenience methods only fill a [`MapRequest`] and call [`Session::query`].
//! Implements contracts/API-PORTS-v1.md query, budgets and continuation.
//! Depends on the index and the wire encoder. A refusal does not write the sink.
//! Read hashes the source again when the digest matches. Plugins are not scheduled.
//! `line_start` is the absolute read line. `depth` is the hop count only.
//! An over-budget read keeps a marked prefix and a continuation; a signature that does not fit is refused.

mod read_page;
mod read_resume;

use super::Session;
use super::pack_op::{reject_format, reject_git};
use crate::domain::{
    Answer, AnswerRecord, BoundedSink, DctxError, Direction, MapRequest, OperationLimits,
    REQUEST_VERSION, ResolvedLimits, Verb, check_deadline, cursor, parse_cursor, within_budget,
};
use crate::index::{Collected, collect};

use read_page::{encode_page, write_answer};
use read_resume::*;

impl Session {
    /// Runs `request` against `handle` and writes one page when the answer fits.
    /// # Errors
    /// Refuses invalid/stale handles or requests, denied source access, unsupported capabilities, exceeded budgets/deadlines, or sink failures.
    pub fn query(
        &mut self,
        handle: &super::SnapshotHandle,
        request: &MapRequest,
        sink: &mut dyn BoundedSink,
    ) -> Result<Answer, DctxError> {
        let _timing = crate::timings::Span::begin(crate::timings::Phase::Query);
        if !cfg!(feature = "map") {
            return Err(DctxError::unsupported("map"));
        }
        self.reject_transport(request)?;
        request.validate()?;
        if handle
            .scope
            .as_deref()
            .is_some_and(|scope| request.source_scope() != Some(scope))
        {
            return Err(DctxError::invalid(
                "request exceeds the direct handle source scope",
            ));
        }
        let shared = self.generation_for_query(handle)?;
        let projected = request
            .source_scope()
            .map(|scope| {
                let mut projected = crate::index::scope::project(&shared, scope)?;
                // Answer.snapshot continues to identify the pinned handle; the cursor's
                // generation and precision refer to the selected source scope.
                projected.digest.clone_from(&shared.digest);
                Ok::<_, DctxError>(projected)
            })
            .transpose()?;
        let generation = projected.as_ref().unwrap_or(&shared);
        let cursor_id = match &request.continuation {
            Some(token) => Some(parse_cursor(token, generation.id, request.verb)?),
            None => None,
        };
        let resume = match (&cursor_id, request.verb) {
            (Some(id), Verb::Read) => Some(parse_resume(id)?),
            _ => None,
        };
        let resolved = self.query_limits(request)?;
        let now = self.clock.now().millis();
        check_deadline(now, now, request.limits.deadline_ms)?;
        let collected = if request.verb == Verb::Grep {
            Collected::Records(self.grep_hits(generation, request)?)
        } else {
            collect(generation, request, resolved.depth)?
        };
        let mut text_match = false;
        let mut records = match collected {
            Collected::Outcome(outcome) => {
                let answer = self.answer_of(
                    generation,
                    request.verb,
                    Vec::new(),
                    0,
                    Some("map".to_string()),
                    Some(outcome),
                );
                return write_answer(sink, request, &self.place, answer, &resolved);
            }
            Collected::Read(record) => {
                if let Some(resume) = &resume
                    && resume.id != record.id
                {
                    return Err(DctxError::StaleContinuation);
                }
                let (start, count) = match &resume {
                    Some(item) => (
                        Some(item.start),
                        item.end.saturating_sub(item.start).saturating_add(1),
                    ),
                    None => (request.line_start, resolved.lines),
                };
                let (mut record, sliced_cap) =
                    self.read_record(generation, *record, count, start)?;
                let resume_beyond = resume.as_ref().is_some_and(|item| item.beyond);
                if resume_beyond
                    && !sliced_cap
                    && let Some(source) = record.source_utf8.as_mut()
                {
                    source.push_str(OMITTED_LINE);
                }
                let origin = start.unwrap_or(record.lines[0].max(1));
                return self.finish_read(
                    sink,
                    request,
                    generation,
                    record,
                    &resolved,
                    origin,
                    sliced_cap || resume_beyond,
                );
            }
            Collected::Records(mut records) => {
                let explain_path = records.first().map(|record| record.path.clone());
                if matches!(request.verb, Verb::Explain | Verb::Skeleton)
                    && let Some(path) = explain_path
                {
                    self.authorize(&path)?;
                }
                // A find that matched no symbol falls back to the literal search of grep; a key also lists its read sites.
                if request.verb == Verb::Find {
                    (records, text_match) = self.find_with_text(generation, request, records)?;
                }
                if request.verb == Verb::Map {
                    self.order_by_exports(generation, request, &mut records);
                }
                records
            }
        };
        // Callers are ranked over the whole answer, before the cursor and the page cut, so every page sees one order.
        let ranked =
            request.verb == Verb::Neighbors && self.rank_sites(generation, request, &mut records);
        // A neighbors probe on a declaration-only symbol names its paired definition once, as its first record.
        if request.verb == Verb::Neighbors
            && cursor_id.is_none()
            && let Some(pair) = crate::index::pair_record(generation, &request.target)
        {
            records.insert(0, pair);
        }
        if request.verb != Verb::Read
            && let Some(last) = cursor_id
        {
            let position = records
                .iter()
                .position(|record| record.id == last)
                .ok_or(DctxError::StaleContinuation)?;
            records.drain(..=position);
        }
        let cap = usize::try_from(resolved.records).unwrap_or(usize::MAX);
        let mut more = 0u64;
        if records.len() > cap {
            more = u64::try_from(records.len() - cap).unwrap_or(u64::MAX);
            records.truncate(cap);
        }
        if request.verb == Verb::Neighbors {
            if !ranked {
                self.attach_sites(generation, request, &mut records);
            }
            self.attach_out_calls(generation, request, &mut records);
            let fits = |page: &[AnswerRecord], more: u64| {
                let next = if more > 0 {
                    page.last()
                        .map(|record| cursor(generation.id, request.verb, &record.id))
                } else {
                    None
                };
                within_budget(
                    &encode_page(
                        request,
                        &self.place,
                        &self.answer_of(generation, request.verb, page.to_vec(), more, next, None),
                    ),
                    &resolved,
                )
            };
            super::sites_op::fit_neighbors(&mut records, &mut more, fits);
        }
        let had_records = !records.is_empty();
        let mut last_fail = 1u64;
        loop {
            let next = if more > 0 {
                records
                    .last()
                    .map(|record| cursor(generation.id, request.verb, &record.id))
            } else {
                None
            };
            let mut answer = self.answer_of(
                generation,
                request.verb,
                records.clone(),
                more,
                next.clone(),
                None,
            );
            if text_match {
                answer.precision.coverage = "text".to_string();
            }
            let encoded = encode_page(request, &self.place, &answer);
            let len = u64::try_from(encoded.len()).unwrap_or(u64::MAX);
            if within_budget(&encoded, &resolved) {
                if records.is_empty() && had_records {
                    return Err(DctxError::BudgetTooSmall {
                        minimum_bytes: last_fail,
                    });
                }
                sink.write_page(&encoded)?;
                return Ok(answer);
            }
            last_fail = len.max(1);
            if records.is_empty() {
                return Err(DctxError::BudgetTooSmall {
                    minimum_bytes: last_fail,
                });
            }
            // A lone grep group that is too big loses half of its hits and says how many.
            if request.verb == Verb::Grep
                && let [only] = records.as_mut_slice()
                && let Some(shorter) = only.source_utf8.as_deref().and_then(crate::domain::shrink)
            {
                only.source_utf8 = Some(shorter);
                continue;
            }
            records.pop();
            more = more.saturating_add(1);
        }
    }

    /// Map of `target`. Same path as [`Self::query`].
    /// # Errors
    /// Refuses invalid/stale handles or requests, denied source access, unsupported capabilities, exceeded budgets/deadlines, or sink failures.
    pub fn map(
        &mut self,
        handle: &super::SnapshotHandle,
        target: &str,
        sink: &mut dyn BoundedSink,
    ) -> Result<Answer, DctxError> {
        let mut request = MapRequest::new(Verb::Map);
        request.target = target.to_string();
        self.query(handle, &request, sink)
    }

    /// Find `query`. Same path as [`Self::query`].
    /// # Errors
    /// Refuses invalid/stale handles or requests, denied source access, unsupported capabilities, exceeded budgets/deadlines, or sink failures.
    pub fn find(
        &mut self,
        handle: &super::SnapshotHandle,
        query: &str,
        sink: &mut dyn BoundedSink,
    ) -> Result<Answer, DctxError> {
        let mut request = MapRequest::new(Verb::Find);
        request.query = query.to_string();
        self.query(handle, &request, sink)
    }

    /// Neighbors of `target`. Same path as [`Self::query`].
    /// # Errors
    /// Refuses invalid/stale handles or requests, denied source access, unsupported capabilities, exceeded budgets/deadlines, or sink failures.
    pub fn neighbors(
        &mut self,
        handle: &super::SnapshotHandle,
        target: &str,
        sink: &mut dyn BoundedSink,
    ) -> Result<Answer, DctxError> {
        let mut request = MapRequest::new(Verb::Neighbors);
        request.target = target.to_string();
        request.direction = Direction::Out;
        self.query(handle, &request, sink)
    }

    /// Explain `target`. Same path as [`Self::query`].
    /// # Errors
    /// Refuses invalid/stale handles or requests, denied source access, unsupported capabilities, exceeded budgets/deadlines, or sink failures.
    pub fn explain(
        &mut self,
        handle: &super::SnapshotHandle,
        target: &str,
        sink: &mut dyn BoundedSink,
    ) -> Result<Answer, DctxError> {
        let mut request = MapRequest::new(Verb::Explain);
        request.target = target.to_string();
        self.query(handle, &request, sink)
    }

    /// Read `target`. Same path as [`Self::query`].
    /// # Errors
    /// Refuses invalid/stale handles or requests, denied source access, unsupported capabilities, exceeded budgets/deadlines, or sink failures.
    pub fn read(
        &mut self,
        handle: &super::SnapshotHandle,
        target: &str,
        sink: &mut dyn BoundedSink,
    ) -> Result<Answer, DctxError> {
        let mut request = MapRequest::new(Verb::Read);
        request.target = target.to_string();
        self.query(handle, &request, sink)
    }

    pub(super) fn reject_transport(&self, request: &MapRequest) -> Result<(), DctxError> {
        if request.request_version != REQUEST_VERSION {
            return Err(DctxError::UnsupportedVersion {
                found: request.request_version,
            });
        }
        reject_format(request.output_format)?;
        reject_git(request.git_revision.as_deref())?;
        self.cancel.checkpoint().map_err(DctxError::from_core)?;
        Ok(())
    }

    pub(super) fn query_limits(&self, request: &MapRequest) -> Result<ResolvedLimits, DctxError> {
        let policy = self
            .limits
            .intersect(OperationLimits::from_ceiling(&self.ceiling))
            .intersect(self.plan_limits()?);
        crate::domain::resolve_query(request.verb, request, policy)
    }

    pub(crate) fn context_output_limits(&self) -> Result<OperationLimits, DctxError> {
        Ok(self
            .limits
            .intersect(OperationLimits::from_ceiling(&self.ceiling))
            .intersect(self.plan_limits()?))
    }

    fn plan_limits(&self) -> Result<OperationLimits, DctxError> {
        let Some(plan) = &self.plan else {
            return Ok(OperationLimits::default());
        };
        let view = plan
            .views
            .iter()
            .find(|view| view.name == self.view)
            .ok_or(DctxError::PolicyUnavailable)?;
        if view.budget.is_empty() {
            return Ok(OperationLimits::default());
        }
        let budget = plan
            .budgets
            .iter()
            .find(|budget| budget.name == view.budget)
            .ok_or(DctxError::PolicyUnavailable)?;
        Ok(OperationLimits::from_budget(budget))
    }
}

#[cfg(test)]
mod tests;
