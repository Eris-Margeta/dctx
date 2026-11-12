// SPDX-License-Identifier: Apache-2.0
//! Read verification, span serving and the read page that fits the output budget.
//! `read_verified` rehashes the source before it is served: a sensitive name is never read, and a
//! byte change refuses the query. A body that does not fit keeps a prefix of whole
//! lines, moving trailing closing delimiters into the continuation at a budget cut.
//! `finish_read` and `cut_page` pick that marked prefix; `answer_of` assembles every answer from
//! the pinned generation.

use dctx_core::FileHandlingDecision;
use dctx_core::facts::ReadGrant;
use dctx_core::filter::{has_git_segment, is_secret_shaped_name};
use dctx_core::ids::RelativePath;
use dctx_hash::ContentDigest;

use super::super::ledger::READ_CAP;
use super::Session;
use super::read_resume::{OMITTED_LINE, read_cursor, slice_span, truncation_line};
use crate::domain::{
    Answer, AnswerRecord, BoundedSink, DctxError, Freshness, IdentityOutcome, MapRequest,
    Precision, ResolvedLimits, Verb, encode_answer_with, encode_text, within_budget,
};
use crate::index::profile::{self, FileWatch, Stage};

impl Session {
    pub(super) fn read_record(
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
            .ok_or(DctxError::Stale {
                reason: "no stored digest",
            })?;
        let bytes = self.read_verified(&record.path, expected)?;
        let span = record.span.ok_or(DctxError::CorruptIndex {
            reason: "missing span",
        })?;
        let node_lines = record.lines;
        let sliced = slice_span(&bytes, span, node_lines, lines, absolute_start)?;
        record.source_utf8 = Some(sliced.text);
        Ok((record, sliced.capped))
    }

    /// Writes one read page. A body that does not fit keeps the first lines that do.
    #[allow(
        clippy::needless_pass_by_value,
        reason = "The final renderer consumes its assembled answer record before selecting the page that fits the output budget."
    )]
    pub(super) fn finish_read(
        &self,
        sink: &mut dyn BoundedSink,
        request: &MapRequest,
        generation: &crate::index::Generation,
        record: AnswerRecord,
        resolved: &ResolvedLimits,
        origin: u32,
        capped: bool,
    ) -> Result<Answer, DctxError> {
        let (answer, encoded) = self.package(
            request,
            generation,
            &record,
            record.source_utf8.clone(),
            None,
            0,
        );
        if within_budget(&encoded, resolved) {
            sink.write_page(&encoded)?;
            return Ok(answer);
        }
        let full_len = encoded.len().max(1);
        let source = record.source_utf8.as_deref().unwrap_or("");
        let body = if capped {
            source.strip_suffix(OMITTED_LINE).unwrap_or(source)
        } else {
            source
        };
        let lines: Vec<&str> = if body.is_empty() {
            Vec::new()
        } else {
            body.split_inclusive('\n').collect()
        };
        // One line is the signature. Splitting it would hide the refusal size.
        if lines.len() < 2 {
            return Err(DctxError::BudgetTooSmall {
                minimum_bytes: u64::try_from(full_len).unwrap_or(u64::MAX),
            });
        }
        let end_line = origin.saturating_add(
            u32::try_from(lines.len())
                .unwrap_or(u32::MAX)
                .saturating_sub(1),
        );
        // A cut must end on content, not an isolated closing delimiter or blank.
        // In particular, a resumed page may begin with the closing lines moved
        // from its predecessor. Refuse rather than emit only those lines.
        let Some(first) = (1..lines.len()).find(|&kept| !cut_delimiter(lines[kept - 1])) else {
            return Err(DctxError::BudgetTooSmall {
                minimum_bytes: u64::try_from(full_len).unwrap_or(u64::MAX),
            });
        };
        let minimum = self.cut_page(
            request, generation, &record, &lines, first, origin, end_line, capped,
        );
        if !within_budget(&minimum.1, resolved) {
            let needed = minimum.1.len().min(full_len).max(1);
            return Err(DctxError::BudgetTooSmall {
                minimum_bytes: u64::try_from(needed).unwrap_or(u64::MAX),
            });
        }
        let mut best = minimum;
        for kept in first + 1..lines.len() {
            if cut_delimiter(lines[kept - 1]) {
                continue;
            }
            let page = self.cut_page(
                request, generation, &record, &lines, kept, origin, end_line, capped,
            );
            if within_budget(&page.1, resolved) {
                best = page;
            } else {
                break;
            }
        }
        sink.write_page(&best.1)?;
        Ok(best.0)
    }

    pub(super) fn cut_page(
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
            Some(read_cursor(
                generation.id,
                &record.id,
                next_line,
                end_line,
                beyond,
            ))
        };
        let more = if next.is_some() { remaining } else { 0 };
        self.package(request, generation, record, Some(source), next, more)
    }

    pub(super) fn package(
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
        let encoded = encode_page(request, &self.place, &answer);
        (answer, encoded)
    }

    pub(in crate::session) fn read_verified(
        &mut self,
        path: &str,
        expected: ContentDigest,
    ) -> Result<Vec<u8>, DctxError> {
        let relative = RelativePath::parse(path).map_err(DctxError::from_core)?;
        // Last line of defence: a sensitive name is never read, even from an older cached generation.
        if has_git_segment(path) || is_secret_shaped_name(path.rsplit('/').next().unwrap_or(path)) {
            return Err(DctxError::PolicyDenied);
        }
        let grant = ReadGrant::authorize(&relative, FileHandlingDecision::Include, true)
            .ok_or(DctxError::PolicyDenied)?;
        self.content_reads += 1;
        let bytes = self
            .source
            .read(&grant, READ_CAP)
            .map_err(DctxError::from_core)?;
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
        let grant = ReadGrant::authorize(&relative, FileHandlingDecision::Include, true)
            .ok_or(DctxError::PolicyDenied)?;
        let mut watch = FileWatch::begin(path);
        let read_mark = profile::start();
        let read = self
            .source
            .read(&grant, max_bytes)
            .map_err(DctxError::from_core);
        watch.charge(Stage::Read, read_mark);
        let bytes = read?;
        watch.set_bytes(u64::try_from(bytes.len()).unwrap_or(u64::MAX));
        let hash_mark = profile::start();
        let matches = hashes_to(&bytes, expected);
        watch.charge(Stage::Hash, hash_mark);
        matches
    }

    pub(super) fn answer_of(
        &self,
        generation: &crate::index::Generation,
        verb: Verb,
        records: Vec<AnswerRecord>,
        more: u64,
        next: Option<String>,
        outcome: Option<IdentityOutcome>,
    ) -> Answer {
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
        Answer {
            generation: generation.id,
            snapshot: generation.digest.clone(),
            verb,
            records,
            more,
            next,
            freshness: Freshness {
                mode: generation.freshness.as_str().to_string(),
                state: state.to_string(),
                age_s,
            },
            precision: Precision {
                coverage: generation.coverage.clone(),
                completion: generation.completion.clone(),
                parser: generation.parser.clone(),
            },
            outcome,
        }
    }
}

fn cut_delimiter(line: &str) -> bool {
    line.trim().chars().all(|ch| "}]),;".contains(ch))
}

pub(super) fn hashes_to(bytes: &[u8], expected: ContentDigest) -> Result<bool, DctxError> {
    Ok(crate::domain::digest_text(bytes)? == expected)
}

pub(super) fn write_answer(
    sink: &mut dyn BoundedSink,
    request: &MapRequest,
    place: &str,
    answer: Answer,
    resolved: &ResolvedLimits,
) -> Result<Answer, DctxError> {
    let encoded = encode_page(request, place, &answer);
    if !within_budget(&encoded, resolved) {
        let minimum_bytes = u64::try_from(encoded.len()).unwrap_or(u64::MAX).max(1);
        return Err(DctxError::BudgetTooSmall { minimum_bytes });
    }
    sink.write_page(&encoded)?;
    Ok(answer)
}

pub(super) fn encode_page(request: &MapRequest, place: &str, answer: &Answer) -> Vec<u8> {
    let _timing = crate::timings::Span::begin(crate::timings::Phase::Render);
    if request.output_format == 1 {
        encode_text(place, answer, request.meta).into_bytes()
    } else {
        encode_answer_with(place, answer, request.meta).into_bytes()
    }
}
