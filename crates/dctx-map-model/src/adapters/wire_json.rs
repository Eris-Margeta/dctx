//! JSON v1 conversion retained for the future wire adapter.
//! Implements SPEC-MAP-MODE sections 4.1 and 4.2. This is not domain storage.
//! Depends on serde and serde_json through the `wire-json` feature. The text renderer calls it.
//! Golden files in `fixtures/wire-json/` are the byte oracle for this projection.

use super::RenderError;
use crate::domain::*;
use serde::Serialize;

pub(super) fn canonical(record: &Record) -> Result<String, RenderError> {
    Ok(serde_json::to_string(record)?)
}

pub(super) fn encode(
    answer: &Answer,
    records: &[&Record],
    more: usize,
    needs_next: bool,
) -> Result<String, RenderError> {
    Ok(serde_json::to_string(&JsonAnswer {
        v: 1,
        place: &answer.place,
        rev: &answer.rev[..7],
        dirty: answer.dirty,
        verb: answer.verb,
        records,
        more,
        next: if needs_next { &answer.next } else { "" },
        identity: &answer.identity,
        reason: answer.reason.as_deref().or(
            if answer.records.is_empty() && answer.identity.is_none() {
                Some("no match")
            } else {
                None
            },
        ),
    })?)
}

#[derive(Serialize)]
struct JsonAnswer<'a> {
    v: u8,
    place: &'a str,
    rev: &'a str,
    dirty: usize,
    verb: Verb,
    records: &'a [&'a Record],
    more: usize,
    next: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    identity: &'a Option<IdentityStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<&'a str>,
}
