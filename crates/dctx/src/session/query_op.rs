//! The five verbs. Convenience methods only fill a [`MapRequest`] and call [`Session::query`].
//! Implements contracts/API-PORTS-v1.md query, budgets and continuation.
//! Depends on the index and the wire encoder. A refusal does not write the sink.
//! Read hashes the source again when the digest matches. Plugins are not scheduled.
//! `line_start` is the absolute read line. `depth` is the hop count only.
//! An over-budget read keeps a marked prefix and a continuation; a signature that does not fit is refused.

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
use crate::index::profile::{self, FileWatch, Stage};
use crate::index::{collect, Collected};

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
        if !cfg!(feature = "map") {
            return Err(DctxError::unsupported("map"));
        }
        self.reject_transport(request)?;
        request.validate()?;
        let generation = self.generation(handle)?;
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
        let collected = collect(&generation, request, resolved.depth)?;
        let mut records = match collected {
            Collected::Outcome(outcome) => {
                let answer = self.answer_of(&generation, request.verb, Vec::new(), 0, Some("map".to_string()), Some(outcome));
                return write_answer(sink, request, &self.place, answer, &resolved);
            }
            Collected::Read(record) => {
                if let Some(resume) = &resume
                    && resume.id != record.id
                {
                    return Err(DctxError::StaleContinuation);
                }
                let (start, count) = match &resume {
                    Some(item) => (Some(item.start), item.end.saturating_sub(item.start).saturating_add(1)),
                    None => (request.line_start, resolved.lines),
                };
                let (mut record, sliced_cap) = self.read_record(&generation, *record, count, start)?;
                let resume_beyond = resume.as_ref().is_some_and(|item| item.beyond);
                if resume_beyond && !sliced_cap
                    && let Some(source) = record.source_utf8.as_mut() {
                        source.push_str(OMITTED_LINE);
                    }
                let origin = start.unwrap_or(record.lines[0].max(1));
                return self.finish_read(sink, request, &generation, record, &resolved, origin, sliced_cap || resume_beyond);
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
    /// # Errors
    /// Refuses invalid/stale handles or requests, denied source access, unsupported capabilities, exceeded budgets/deadlines, or sink failures.
    pub fn map(&mut self, handle: &super::SnapshotHandle, target: &str, sink: &mut dyn BoundedSink) -> Result<Answer, DctxError> {
        let mut request = MapRequest::new(Verb::Map);
        request.target = target.to_string();
        self.query(handle, &request, sink)
    }

    /// Find `query`. Same path as [`Self::query`].
    /// # Errors
    /// Refuses invalid/stale handles or requests, denied source access, unsupported capabilities, exceeded budgets/deadlines, or sink failures.
    pub fn find(&mut self, handle: &super::SnapshotHandle, query: &str, sink: &mut dyn BoundedSink) -> Result<Answer, DctxError> {
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
    ) -> Result<(AnswerRecord, bool), DctxError> {
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
        let sliced = slice_span(&bytes, span, node_lines, lines, absolute_start)?;
        record.source_utf8 = Some(sliced.text);
        Ok((record, sliced.capped))
    }

    /// Writes one read page. A body that does not fit keeps the first lines that do.
    #[allow(clippy::needless_pass_by_value, reason = "The final renderer consumes its assembled answer record before selecting the page that fits the output budget.")]
    fn finish_read(
        &self,
        sink: &mut dyn BoundedSink,
        request: &MapRequest,
        generation: &crate::index::Generation,
        record: AnswerRecord,
        resolved: &ResolvedLimits,
        origin: u32,
        capped: bool,
    ) -> Result<Answer, DctxError> {
        let (answer, encoded) = self.package(request, generation, &record, record.source_utf8.clone(), None, 0);
        if within_budget(&encoded, resolved) {
            sink.write_page(&encoded)?;
            return Ok(answer);
        }
        let full_len = encoded.len().max(1);
        let source = record.source_utf8.as_deref().unwrap_or("");
        let body = if capped { source.strip_suffix(OMITTED_LINE).unwrap_or(source) } else { source };
        let lines: Vec<&str> = if body.is_empty() { Vec::new() } else { body.split_inclusive('\n').collect() };
        // One line is the signature. Splitting it would hide the refusal size.
        if lines.len() < 2 {
            return Err(DctxError::BudgetTooSmall { minimum_bytes: u64::try_from(full_len).unwrap_or(u64::MAX) });
        }
        let end_line = origin.saturating_add(u32::try_from(lines.len()).unwrap_or(u32::MAX).saturating_sub(1));
        let minimum = self.cut_page(request, generation, &record, &lines, 1, origin, end_line, capped);
        if !within_budget(&minimum.1, resolved) {
            let needed = minimum.1.len().min(full_len).max(1);
            return Err(DctxError::BudgetTooSmall { minimum_bytes: u64::try_from(needed).unwrap_or(u64::MAX) });
        }
        let mut best = minimum;
        for kept in 2..lines.len() {
            let page = self.cut_page(request, generation, &record, &lines, kept, origin, end_line, capped);
            if within_budget(&page.1, resolved) {
                best = page;
            } else {
                break;
            }
        }
        sink.write_page(&best.1)?;
        Ok(best.0)
    }

    fn cut_page(
        &self,
        request: &MapRequest,
        generation: &crate::index::Generation,
        record: &AnswerRecord,
        lines: &[&str],
        kept: usize,
        origin: u32,
        end_line: u32,
        beyond: bool,
    ) -> (Answer, Vec<u8>) {
        let mut source = String::new();
        for line in lines.iter().take(kept) {
            source.push_str(line);
        }
        let remaining = u64::try_from(lines.len().saturating_sub(kept)).unwrap_or(u64::MAX);
        let next_line = origin.saturating_add(u32::try_from(kept).unwrap_or(u32::MAX));
        let next = if remaining == 0 {
            if beyond {
                source.push_str(OMITTED_LINE);
            }
            None
        } else {
            source.push_str(&truncation_line(next_line, end_line));
            Some(read_cursor(generation.id, &record.id, next_line, end_line, beyond))
        };
        let more = if next.is_some() { remaining } else { 0 };
        self.package(request, generation, record, Some(source), next, more)
    }

    fn package(
        &self,
        request: &MapRequest,
        generation: &crate::index::Generation,
        record: &AnswerRecord,
        source: Option<String>,
        next: Option<String>,
        more: u64,
    ) -> (Answer, Vec<u8>) {
        let mut record = record.clone();
        record.source_utf8 = source;
        let answer = self.answer_of(generation, request.verb, vec![record], more, next, None);
        let encoded = encode_page(request.output_format, &self.place, &answer);
        (answer, encoded)
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
    /// it returns `SourceChanged`. Read/hash samples close before shared cache finalization.
    pub(in crate::session) fn digest_matches_source(
        &self,
        path: &str,
        expected: ContentDigest,
        max_bytes: u64,
    ) -> Result<bool, DctxError> {
        let relative = RelativePath::parse(path).map_err(DctxError::from_core)?;
        let grant = ReadGrant::authorize(&relative, FileHandlingDecision::Include, true).ok_or(DctxError::PolicyDenied)?;
        let mut watch = FileWatch::begin(path);
        let read_mark = profile::start();
        let read = self.source.read(&grant, max_bytes).map_err(DctxError::from_core);
        watch.charge(Stage::Read, read_mark);
        let bytes = read?;
        watch.set_bytes(u64::try_from(bytes.len()).unwrap_or(u64::MAX));
        let hash_mark = profile::start();
        let matches = hashes_to(&bytes, expected);
        watch.charge(Stage::Hash, hash_mark);
        matches
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

const OMITTED_LINE: &str = "[DCTX_OMITTED]\n";
const TRUNCATED_MARK: &str = "[DCTX_TRUNCATED]";

#[derive(Debug)]
struct SlicedSource {
    text: String,
    capped: bool,
}

struct ReadResume {
    id: String,
    start: u32,
    end: u32,
    beyond: bool,
}

fn truncation_line(next_line: u32, end_line: u32) -> String {
    format!("{TRUNCATED_MARK} lines {next_line}-{end_line}\n")
}

fn read_cursor(generation: u64, id: &str, start: u32, end: u32, beyond: bool) -> String {
    let flag = if beyond { "1" } else { "0" };
    format!("v1|{generation}|read|{id}|{start}|{end}|{flag}")
}

fn parse_resume(id: &str) -> Result<ReadResume, DctxError> {
    let (head, flag) = id.rsplit_once('|').ok_or(DctxError::StaleContinuation)?;
    let (head, end_text) = head.rsplit_once('|').ok_or(DctxError::StaleContinuation)?;
    let (node, start_text) = head.rsplit_once('|').ok_or(DctxError::StaleContinuation)?;
    let start = start_text.parse::<u32>().map_err(|_| DctxError::StaleContinuation)?;
    let end = end_text.parse::<u32>().map_err(|_| DctxError::StaleContinuation)?;
    let beyond = match flag {
        "1" => true,
        "0" => false,
        _ => return Err(DctxError::StaleContinuation),
    };
    if node.is_empty() || start == 0 || end < start {
        return Err(DctxError::StaleContinuation);
    }
    Ok(ReadResume { id: node.to_string(), start, end, beyond })
}

fn slice_span(
    bytes: &[u8],
    span: [u64; 2],
    node_lines: [u32; 2],
    lines: u32,
    absolute_start: Option<u32>,
) -> Result<SlicedSource, DctxError> {
    let start = usize::try_from(span[0]).map_err(|_| DctxError::CorruptIndex { reason: "span" })?;
    let end = usize::try_from(span[1]).map_err(|_| DctxError::CorruptIndex { reason: "span" })?;
    if start > end || end > bytes.len() {
        return Err(DctxError::CorruptIndex { reason: "span" });
    }
    let text = std::str::from_utf8(&bytes[start..end]).map_err(|_| DctxError::CorruptIndex { reason: "span" })?;
    if lines == 0 {
        return Ok(SlicedSource { text: text.to_string(), capped: false });
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
    let capped = truncated && absolute_start.is_none();
    if capped {
        out.push_str(OMITTED_LINE);
    }
    Ok(SlicedSource { text: out, capped })
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
    use super::{parse_resume, read_cursor, slice_span, truncation_line};

    #[test]
    fn absolute_lines_match_the_file_and_a_prefix_shift_does_not() {
        let bytes = b"L01\nL02\nL03\nL04\n";
        let span = [0, u64::try_from(bytes.len()).unwrap_or(0)];
        let exact = slice_span(bytes, span, [1, 4], 2, Some(3)).expect("range");
        assert_eq!(exact.text, "L03\nL04\n");
        assert!(!exact.capped);
        let prefix = slice_span(bytes, span, [1, 4], 2, None).expect("prefix");
        assert_eq!(prefix.text, "L01\nL02\n[DCTX_OMITTED]\n");
        assert!(prefix.capped);
        let outside = slice_span(bytes, span, [1, 4], 2, Some(4)).expect_err("past end");
        assert!(outside.to_string().contains("outside"), "{outside}");
    }

    #[test]
    fn a_noninitial_range_inside_a_span_is_not_the_span_prefix() {
        let bytes = b"line01\nline02\nline03\nline04\nline05\n";
        let span = [7, 28];
        let inner = slice_span(bytes, span, [2, 4], 2, Some(3)).expect("inner");
        assert_eq!(inner.text, "line03\nline04\n");
        assert_ne!(inner.text, "line02\nline03\n");
        let outside = slice_span(bytes, span, [2, 4], 1, Some(1)).expect_err("before node");
        assert!(outside.to_string().contains("outside"), "{outside}");
    }

    #[test]
    fn a_read_cursor_names_the_next_line_and_keeps_the_id() {
        use crate::domain::Verb;

        let cursor = read_cursor(4, "s:a.rs#inner", 12, 40, true);
        let id = crate::domain::parse_cursor(&cursor, 4, Verb::Read).expect("cursor");
        let resume = parse_resume(&id).expect("resume");
        assert_eq!(resume.id, "s:a.rs#inner");
        assert_eq!(resume.start, 12);
        assert_eq!(resume.end, 40);
        assert!(resume.beyond);
        assert_eq!(truncation_line(12, 40), "[DCTX_TRUNCATED] lines 12-40\n");
        assert!(parse_resume("m:a.rs").is_err());
    }
}
