// SPDX-License-Identifier: Apache-2.0
//! Deterministic whole-record text answers, with JSON delegated to `wire-json`.
//! Implements SPEC-MAP-MODE sections 3, 4.1 and 4.2.
//! Depends on domain records. Callers transport the returned string. No IO.
use crate::domain::{Answer, AnswerBudget, Verb, IdentityStatus, Record, StableId, Edge};
use std::fmt::{self, Write as _};

/// Output wire format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// Agent-facing lines.
    Text,
    /// Version-one machine-facing JSON.
    Json,
}

/// Rendering refusal, with no partial output emitted.
#[derive(Debug)]
pub enum RenderError {
    /// Metadata, notices and continuation alone do not fit.
    BudgetTooSmall,
    /// An empty answer or cut page requires an exact next command.
    MissingNext,
    /// Query records violate a source span or verb limit.
    InvalidRecord,
    /// Answer metadata is not a valid single-line header.
    InvalidMetadata,
    /// The remaining result count overflowed.
    CountOverflow,
    /// JSON serialization failed.
    #[cfg(feature = "wire-json")]
    Json(serde_json::Error),
    /// `Format::Json` was requested without the `wire-json` feature.
    #[cfg(not(feature = "wire-json"))]
    WireJsonDisabled,
}
impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BudgetTooSmall => {
                f.write_str("budget cannot fit answer metadata and next command")
            }
            Self::MissingNext => f.write_str("answer requires an exact next command"),
            Self::InvalidRecord => f.write_str("record violates source span or verb limits"),
            Self::InvalidMetadata => f.write_str("invalid answer metadata"),
            Self::CountOverflow => f.write_str("remaining record count overflow"),
            #[cfg(feature = "wire-json")]
            Self::Json(error) => error.fmt(f),
            #[cfg(not(feature = "wire-json"))]
            Self::WireJsonDisabled => f.write_str("json conversion requires the wire-json feature"),
        }
    }
}
impl std::error::Error for RenderError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        #[cfg(feature = "wire-json")]
        if let Self::Json(error) = self {
            return Some(error);
        }
        None
    }
}
#[cfg(feature = "wire-json")]
impl From<serde_json::Error> for RenderError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

/// Renders an answer within the complete serialized byte budget.
/// A cut retains a ranked prefix of whole records and counts every omitted record.
///
/// # Errors
/// Refuses invalid metadata or records, missing continuation commands, count overflow and a budget too small for metadata; JSON output can also fail or be disabled.
pub fn render(
    answer: &Answer,
    budget: AnswerBudget,
    format: Format,
) -> Result<String, RenderError> {
    validate(answer)?;
    let byte_limit = if answer.verb == Verb::Explain {
        budget.bytes().min(3500)
    } else {
        budget.bytes()
    };
    let records = ranked(answer)?;
    let hard_count = match answer.verb {
        Verb::Map | Verb::Neighbors => 60,
        Verb::Find => 40,
        Verb::Explain | Verb::Read => 1,
    };
    for keep in (0..=records.len().min(hard_count)).rev() {
        let more = answer
            .more
            .checked_add(records.len() - keep)
            .ok_or(RenderError::CountOverflow)?;
        let needs_next = more > 0
            || records.is_empty()
            || answer.reason.is_some()
            || matches!(answer.identity, Some(IdentityStatus::Gone { .. }));
        if needs_next && answer.next.trim().is_empty() {
            return Err(RenderError::MissingNext);
        }
        let output = match format {
            Format::Text => text(answer, &records[..keep], more, needs_next),
            Format::Json => json(answer, &records[..keep], more, needs_next)?,
        };
        if output.len() <= byte_limit {
            return Ok(output);
        }
    }
    Err(RenderError::BudgetTooSmall)
}

/// Convenience text renderer.
///
/// # Errors
/// Returns the validation, continuation, count-overflow or byte-budget refusal from [`render`].
pub fn render_text(answer: &Answer, budget: AnswerBudget) -> Result<String, RenderError> {
    render(answer, budget, Format::Text)
}
/// Convenience JSON renderer.
///
/// # Errors
/// Returns the validation, continuation, count-overflow or byte-budget refusal from [`render`], or a JSON serialization/feature error.
pub fn render_json(answer: &Answer, budget: AnswerBudget) -> Result<String, RenderError> {
    render(answer, budget, Format::Json)
}

fn validate(answer: &Answer) -> Result<(), RenderError> {
    if answer.place.trim().is_empty()
        || answer.place.chars().any(char::is_whitespace)
        || answer.place.chars().any(char::is_control)
        || answer.rev.len() < 7
        || !answer.rev.bytes().all(|b| b.is_ascii_hexdigit())
        || answer.next.chars().any(char::is_control)
    {
        return Err(RenderError::InvalidMetadata);
    }
    for r in &answer.records {
        if r.l[0] == 0
            || r.l[1] < r.l[0]
            || r.kind.trim().is_empty()
            || r.incoming.len() > 5
            || r.outgoing.len() > 5
            || r.relation
                .as_ref()
                .is_some_and(|rel| !rel.edge.permits(rel.confidence))
            || r.incoming
                .iter()
                .chain(&r.outgoing)
                .any(|e| !e.kind.permits(e.conf))
            || (answer.verb == Verb::Neighbors && r.relation.is_none())
            || (answer.verb == Verb::Read
                && (r.body.is_none()
                    || r.l[1] - r.l[0] >= 400
                    || r.body
                        .as_ref()
                        .is_some_and(|body| body.lines().count() > 400)))
        {
            return Err(RenderError::InvalidRecord);
        }
    }
    if let Some(IdentityStatus::Moved { new, .. }) = &answer.identity
        && !answer.records.iter().any(|r| &r.id == new)
    {
        return Err(RenderError::InvalidRecord);
    }
    if matches!(answer.identity, Some(IdentityStatus::Gone { .. })) && !answer.records.is_empty() {
        return Err(RenderError::InvalidRecord);
    }
    Ok(())
}

fn line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn record_line(r: &Record) -> String {
    let prefix = r
        .relation
        .as_ref()
        .map(|rel| {
            let arrow = if rel.incoming {
                format!("←{}", rel.edge.as_str())
            } else {
                format!("{}→", rel.edge.as_str())
            };
            format!("{arrow}  {}  ", rel.confidence.as_str())
        })
        .unwrap_or_default();
    format!(
        "{prefix}{}  {}  L{}-L{}  {}  [{}]{}\n",
        r.id,
        line(&r.kind),
        r.l[0],
        r.l[1],
        line(&r.sum),
        r.src.as_str(),
        if r.stale { "  stale" } else { "" }
    )
}

fn id_list(ids: &[StableId]) -> String {
    ids.iter()
        .map(StableId::as_str)
        .collect::<Vec<_>>()
        .join(", ")
}

fn edge_list(edges: &[Edge], incoming: bool) -> String {
    edges
        .iter()
        .map(|edge| {
            if incoming {
                format!(
                    "←{} {} {}",
                    edge.kind.as_str(),
                    edge.conf.as_str(),
                    edge.src
                )
            } else {
                format!(
                    "{}→ {} {}",
                    edge.kind.as_str(),
                    edge.conf.as_str(),
                    edge.dst
                )
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn explain(r: &Record) -> String {
    format!(
        "id: {}\nkind: {}\nsignature: {}\nlocation: {}:L{}-L{}\nsummary: {}\nsource: {}\nin: {}\nout: {}\ntests: {}\ndocuments: {}\nstale: {}\n",
        r.id,
        line(&r.kind),
        line(r.signature.as_deref().unwrap_or_default()),
        line(&r.path),
        r.l[0],
        r.l[1],
        line(&r.sum),
        r.src.as_str(),
        edge_list(&r.incoming, true),
        edge_list(&r.outgoing, false),
        id_list(&r.tests),
        id_list(&r.documents),
        r.stale
    )
}

#[allow(clippy::expect_used, reason = "writing to a String cannot fail")]
fn text(answer: &Answer, records: &[&Record], more: usize, needs_next: bool) -> String {
    let dirty = if answer.dirty == 0 {
        String::new()
    } else {
        format!("+dirty {}", answer.dirty)
    };
    let mut output = format!(
        "# map {} rev={}{} age={}\n",
        answer.place,
        &answer.rev[..7],
        dirty,
        answer.age_seconds
    );
    if let Some(identity) = &answer.identity {
        match identity {
            IdentityStatus::Moved { old, new } => writeln!(output, "moved: {old} -> {new}"),
            IdentityStatus::Gone { old } => writeln!(output, "gone: {old}"),
        }.expect("writing to a String cannot fail");
    }
    for r in records {
        match answer.verb {
            Verb::Explain => output.push_str(&explain(r)),
            Verb::Read => {
                output.push_str(&record_line(r));
                if let Some(body) = &r.body {
                    output.push_str(body);
                    if !body.ends_with('\n') {
                        output.push('\n');
                    }
                }
            }
            _ => output.push_str(&record_line(r)),
        }
    }
    if more > 0 {
        writeln!(output, "+{more} more · next: {}", answer.next).expect("writing to a String cannot fail");
    } else if needs_next {
        let reason = answer
            .reason
            .as_deref()
            .unwrap_or(if answer.identity.is_some() {
                "resolve id"
            } else {
                "no match"
            });
        writeln!(output, "{} · try: {}", line(reason), answer.next).expect("writing to a String cannot fail");
    }
    output
}

fn ranked(answer: &Answer) -> Result<Vec<&Record>, RenderError> {
    #[cfg(feature = "wire-json")]
    {
        let mut records = answer
            .records
            .iter()
            .map(|record| Ok((record, super::wire_json::canonical(record)?)))
            .collect::<Result<Vec<_>, RenderError>>()?;
        records.sort_by(|(left, left_json), (right, right_json)| {
            right
                .score
                .cmp(&left.score)
                .then_with(|| left.id.cmp(&right.id))
                .then_with(|| left_json.cmp(right_json))
        });
        Ok(records.into_iter().map(|(record, _)| record).collect())
    }
    #[cfg(not(feature = "wire-json"))]
    {
        let mut records: Vec<&Record> = answer.records.iter().collect();
        records.sort_by(|left, right| {
            right.score.cmp(&left.score).then_with(|| left.id.cmp(&right.id))
        });
        Ok(records)
    }
}

fn json<'a>(
    answer: &'a Answer,
    records: &'a [&'a Record],
    more: usize,
    needs_next: bool,
) -> Result<String, RenderError> {
    #[cfg(feature = "wire-json")]
    {
        super::wire_json::encode(answer, records, more, needs_next)
    }
    #[cfg(not(feature = "wire-json"))]
    {
        let _ = (answer, records, more, needs_next);
        Err(RenderError::WireJsonDisabled)
    }
}

#[cfg(test)]
#[path = "render_tests.rs"]
mod tests;
