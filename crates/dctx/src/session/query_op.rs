//! The five verbs. Convenience methods only fill a [`MapRequest`] and call [`Session::query`].
//! Implements contracts/API-PORTS-v1.md query, budgets and continuation.
//! Depends on the index and the wire encoder. A refusal does not write the sink.
//! Read hashes the source again when the digest matches. Plugins are not scheduled.
//! When read sets both `lines` and `depth`, `depth` is the absolute start line, not a hop count.

use dctx_core::facts::ReadGrant;
use dctx_core::ids::RelativePath;
use dctx_core::FileHandlingDecision;
use dctx_hash::ContentDigest;

use super::ledger::READ_CAP;
use super::pack_op::{reject_format, reject_git};
use super::Session;
use crate::domain::{
    check_deadline, cursor, encode_answer, encode_text, parse_cursor, within_budget, Answer, AnswerRecord,
    BoundedSink, Direction, Freshness, IdentityOutcome, MapRequest, OperationLimits, Precision, ResolvedLimits, Verb,
    DctxError, REQUEST_VERSION,
};
use crate::index::{collect, Collected};

impl Session {
    /// Runs `request` against `handle` and writes one page when the answer fits.
    pub fn query(
        &mut self,
        handle: &super::SnapshotHandle,
        request: &MapRequest,
        sink: &mut dyn BoundedSink,
    ) -> Result<Answer, DctxError> {
        if !cfg!(feature = "map") {
            return Err(DctxError::unsupported("map"));
        }
        self.reject_transport(request)?;
        let generation = self.generation(handle)?;
        let cursor_id = match &request.continuation {
            Some(token) => Some(parse_cursor(token, generation.id, request.verb)?),
            None => None,
        };
        let resolved = self.query_limits(request)?;
        let now = self.clock.now().millis();
        check_deadline(now, now, request.limits.deadline_ms)?;
        let collected = collect(&generation, request, resolved.depth)?;
        let mut records = match collected {
            Collected::Outcome(outcome) => {
                let answer = self.answer_of(&generation, request.verb, Vec::new(), 0, Some("map".to_string()), Some(outcome));
                return write_answer(sink, request, &self.place, answer, &resolved);
            }
            Collected::Read(record) => {
                let record = self.read_record(&generation, *record, resolved.lines, read_start(request))?;
                vec![record]
            }
            Collected::Records(records) => {
                let explain_path = records.first().map(|record| record.path.clone());
                if request.verb == Verb::Explain
                    && let Some(path) = explain_path
                {
                    self.authorize(&path)?;
                }
                records
            }
        };
        if request.verb != Verb::Read
            && let Some(last) = cursor_id
        {
            let position = records.iter().position(|record| record.id == last).ok_or(DctxError::StaleContinuation)?;
            records.drain(..=position);
        }
        let cap = usize::try_from(resolved.records).unwrap_or(usize::MAX);
        let mut more = 0u64;
        if records.len() > cap {
            more = u64::try_from(records.len() - cap).unwrap_or(u64::MAX);
            records.truncate(cap);
        }
        let had_records = !records.is_empty();
        let mut last_fail = 1u64;
        loop {
            let next = if more > 0 { records.last().map(|record| cursor(generation.id, request.verb, &record.id)) } else { None };
            let answer = self.answer_of(&generation, request.verb, records.clone(), more, next.clone(), None);
            let encoded = encode_page(request.output_format, &self.place, &answer);
            let len = u64::try_from(encoded.len()).unwrap_or(u64::MAX);
            if within_budget(&encoded, &resolved) {
                if records.is_empty() && had_records {
                    return Err(DctxError::BudgetTooSmall { minimum_bytes: last_fail });
                }
                sink.write_page(&encoded)?;
                return Ok(answer);
            }
            last_fail = len.max(1);
            if records.is_empty() {
                return Err(DctxError::BudgetTooSmall { minimum_bytes: last_fail });
            }
            records.pop();
            more = more.saturating_add(1);
        }
    }

    /// Map of `target`. Same path as [`Self::query`].
    pub fn map(&mut self, handle: &super::SnapshotHandle, target: &str, sink: &mut dyn BoundedSink) -> Result<Answer, DctxError> {
        let mut request = MapRequest::new(Verb::Map);
        request.target = target.to_string();
        self.query(handle, &request, sink)
    }

    /// Find `query`. Same path as [`Self::query`].
    pub fn find(&mut self, handle: &super::SnapshotHandle, query: &str, sink: &mut dyn BoundedSink) -> Result<Answer, DctxError> {
        let mut request = MapRequest::new(Verb::Find);
        request.query = query.to_string();
        self.query(handle, &request, sink)
    }

    /// Neighbors of `target`. Same path as [`Self::query`].
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

    fn reject_transport(&self, request: &MapRequest) -> Result<(), DctxError> {
        if request.request_version != REQUEST_VERSION {
            return Err(DctxError::UnsupportedVersion { found: request.request_version });
        }
        reject_format(request.output_format)?;
        reject_git(request.git_revision.as_deref())?;
        self.cancel.checkpoint().map_err(DctxError::from_core)?;
        Ok(())
    }

    fn query_limits(&self, request: &MapRequest) -> Result<ResolvedLimits, DctxError> {
        let policy = self.limits.intersect(OperationLimits::from_ceiling(&self.ceiling)).intersect(self.plan_limits()?);
        crate::domain::resolve_query(request.verb, request, policy)
    }

    fn plan_limits(&self) -> Result<OperationLimits, DctxError> {
        let Some(plan) = &self.plan else {
            return Ok(OperationLimits::default());
        };
        let view = plan.views.iter().find(|view| view.name == self.view).ok_or(DctxError::PolicyUnavailable)?;
        if view.budget.is_empty() {
            return Ok(OperationLimits::default());
        }
        let budget = plan.budgets.iter().find(|budget| budget.name == view.budget).ok_or(DctxError::PolicyUnavailable)?;
        Ok(OperationLimits::from_budget(budget))
    }

    fn read_record(
        &mut self,
        generation: &crate::index::Generation,
        mut record: AnswerRecord,
        lines: u32,
        absolute_start: Option<u32>,
    ) -> Result<AnswerRecord, DctxError> {
        self.authorize(&record.path)?;
        let expected = generation
            .files
            .iter()
            .find(|file| file.path == record.path)
            .and_then(|file| file.digest)
            .ok_or(DctxError::Stale { reason: "no stored digest" })?;
        let bytes = self.read_verified(&record.path, expected)?;
        let span = record.span.ok_or(DctxError::CorruptIndex { reason: "missing span" })?;
        let node_lines = record.lines;
        record.source_utf8 = Some(slice_span(&bytes, span, node_lines, lines, absolute_start)?);
        Ok(record)
    }

    fn read_verified(&mut self, path: &str, expected: ContentDigest) -> Result<Vec<u8>, DctxError> {
        let relative = RelativePath::parse(path).map_err(DctxError::from_core)?;
        let grant = ReadGrant::authorize(&relative, FileHandlingDecision::Include, true).ok_or(DctxError::PolicyDenied)?;
        self.content_reads += 1;
        let bytes = self.source.read(&grant, READ_CAP).map_err(DctxError::from_core)?;
        if !hashes_to(&bytes, expected)? {
            return Err(DctxError::SourceChanged);
        }
        Ok(bytes)
    }

    /// True when `path` still hashes to `expected`. This does not increment content reads.
    ///
    /// The comparison is `digest_text` equality, the same check `read_verified` uses before
    /// it returns `SourceChanged`.
    pub(in crate::session) fn digest_matches_source(
        &self,
        path: &str,
        expected: ContentDigest,
        max_bytes: u64,
    ) -> Result<bool, DctxError> {
        let relative = RelativePath::parse(path).map_err(DctxError::from_core)?;
        let grant = ReadGrant::authorize(&relative, FileHandlingDecision::Include, true).ok_or(DctxError::PolicyDenied)?;
        let bytes = self.source.read(&grant, max_bytes).map_err(DctxError::from_core)?;
        hashes_to(&bytes, expected)
    }

    fn answer_of(
        &self,
        generation: &crate::index::Generation,
        verb: Verb,
        records: Vec<AnswerRecord>,
        more: u64,
        next: Option<String>,
        outcome: Option<IdentityOutcome>,
    ) -> Answer {
        let age_s = self.clock.now().millis().saturating_sub(generation.taken_at) / 1000;
        let state = if generation.freshness == crate::domain::FreshnessMode::Metadata { "metadata" } else { "verified" };
        Answer {
            generation: generation.id,
            snapshot: generation.digest.clone(),
            verb,
            records,
            more,
            next,
            freshness: Freshness { mode: generation.freshness.as_str().to_string(), state: state.to_string(), age_s },
            precision: Precision {
                coverage: generation.coverage.clone(),
                completion: generation.completion.clone(),
                parser: generation.parser.clone(),
            },
            outcome,
        }
    }
}

fn hashes_to(bytes: &[u8], expected: ContentDigest) -> Result<bool, DctxError> {
    Ok(crate::domain::digest_text(bytes)? == expected)
}

fn write_answer(
    sink: &mut dyn BoundedSink,
    request: &MapRequest,
    place: &str,
    answer: Answer,
    resolved: &ResolvedLimits,
) -> Result<Answer, DctxError> {
    let encoded = encode_page(request.output_format, place, &answer);
    if !within_budget(&encoded, resolved) {
        let minimum_bytes = u64::try_from(encoded.len()).unwrap_or(u64::MAX).max(1);
        return Err(DctxError::BudgetTooSmall { minimum_bytes });
    }
    sink.write_page(&encoded)?;
    Ok(answer)
}

fn encode_page(format: u32, place: &str, answer: &Answer) -> Vec<u8> {
    if format == 1 { encode_text(place, answer).into_bytes() } else { encode_answer(place, answer).into_bytes() }
}

/// `depth` on a read that also sets `lines` is the absolute 1-based start. The count stays in `lines`.
fn read_start(request: &MapRequest) -> Option<u32> {
    if request.verb == Verb::Read && request.lines.is_some() { request.depth } else { None }
}

fn slice_span(
    bytes: &[u8],
    span: [u64; 2],
    node_lines: [u32; 2],
    lines: u32,
    absolute_start: Option<u32>,
) -> Result<String, DctxError> {
    let start = usize::try_from(span[0]).map_err(|_| DctxError::CorruptIndex { reason: "span" })?;
    let end = usize::try_from(span[1]).map_err(|_| DctxError::CorruptIndex { reason: "span" })?;
    if start > end || end > bytes.len() {
        return Err(DctxError::CorruptIndex { reason: "span" });
    }
    let text = std::str::from_utf8(&bytes[start..end]).map_err(|_| DctxError::CorruptIndex { reason: "span" })?;
    if lines == 0 {
        return Ok(text.to_string());
    }
    let (skip, take) = match absolute_start {
        None => (0usize, usize::try_from(lines).unwrap_or(usize::MAX)),
        Some(line) => absolute_window(line, lines, node_lines)?,
    };
    let mut out = String::new();
    let mut index = 0usize;
    let mut taken = 0usize;
    let mut truncated = false;
    for line in text.split_inclusive('\n') {
        if index < skip {
            index += 1;
            continue;
        }
        if taken >= take {
            truncated = true;
            break;
        }
        out.push_str(line);
        taken += 1;
        index += 1;
    }
    if absolute_start.is_some() && taken != take {
        return Err(DctxError::invalid("lines are outside the node span"));
    }
    if truncated && absolute_start.is_none() {
        out.push_str("[DCTX_OMITTED]\n");
    }
    Ok(out)
}

fn absolute_window(line: u32, lines: u32, node_lines: [u32; 2]) -> Result<(usize, usize), DctxError> {
    let last = line.checked_add(lines.saturating_sub(1)).ok_or_else(|| DctxError::invalid("lines are outside the node span"))?;
    if line == 0 || line < node_lines[0] || last > node_lines[1] {
        return Err(DctxError::invalid("lines are outside the node span"));
    }
    let skip = usize::try_from(line - node_lines[0]).unwrap_or(usize::MAX);
    let take = usize::try_from(lines).unwrap_or(usize::MAX);
    Ok((skip, take))
}

#[cfg(test)]
mod tests {
    use super::slice_span;

    #[test]
    fn absolute_lines_match_the_file_and_a_prefix_shift_does_not() {
        let bytes = b"L01\nL02\nL03\nL04\n";
        let span = [0, u64::try_from(bytes.len()).unwrap_or(0)];
        let exact = slice_span(bytes, span, [1, 4], 2, Some(3)).expect("range");
        assert_eq!(exact, "L03\nL04\n");
        let prefix = slice_span(bytes, span, [1, 4], 2, None).expect("prefix");
        assert_eq!(prefix, "L01\nL02\n[DCTX_OMITTED]\n");
        let outside = slice_span(bytes, span, [1, 4], 2, Some(4)).expect_err("past end");
        assert!(outside.to_string().contains("outside"), "{outside}");
    }

    #[test]
    fn a_noninitial_range_inside_a_span_is_not_the_span_prefix() {
        let bytes = b"line01\nline02\nline03\nline04\nline05\n";
        let span = [7, 28];
        let inner = slice_span(bytes, span, [2, 4], 2, Some(3)).expect("inner");
        assert_eq!(inner, "line03\nline04\n");
        assert_ne!(inner, "line02\nline03\n");
        let outside = slice_span(bytes, span, [2, 4], 1, Some(1)).expect_err("before node");
        assert!(outside.to_string().contains("outside"), "{outside}");
    }
}
