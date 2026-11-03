// SPDX-License-Identifier: Apache-2.0
//! Length-delimited fact records. Keys are stored in strictly increasing canonical bytes.
//! Implements SEGMENTS-v1 `record_length:u32 || payload` for kind 3.
//! Depends on [`super::facts`] for the field order. The segment decoder calls [`decode`] after the checksum.
//! Duplicate keys are refused, including two identical payloads.

use std::cmp::Ordering;

use super::cursor::{push_bool, push_bytes, push_narrow, push_record, push_u16, push_u32, push_u64, Cursor};
use super::error::SegmentError;
use super::facts::{encode_key, parse_key, validate_fact, Declaration, Fact, Provenance, RawRef};
use super::kinds::{Completion, Coverage, DeclarationKind, EdgeKind, SummarySource};
use super::limits::{bounded_count, DecoderLimits};

pub(crate) fn encode(facts: &[Fact], limits: &DecoderLimits) -> Result<(Vec<u8>, u64), SegmentError> {
    if facts.is_empty() {
        return Ok((Vec::new(), 0));
    }
    let mut out = Vec::new();
    let mut previous: Option<Vec<u8>> = None;
    for fact in facts {
        let key = encode_key(&fact.key);
        if let Some(prior) = &previous {
            match key.cmp(prior) {
                Ordering::Greater => {}
                Ordering::Equal => return Err(SegmentError::Facts("duplicate")),
                Ordering::Less => return Err(SegmentError::Facts("order")),
            }
        }
        previous = Some(key.clone());
        let mut body = key;
        write_declarations(&mut body, fact, limits)?;
        write_refs(&mut body, fact, limits)?;
        write_provenance(&mut body, &fact.provenance)?;
        push_record(&mut out, &body, limits.max_record_bytes)?;
    }
    Ok((out, u64::try_from(facts.len()).map_err(|_| SegmentError::Arithmetic)?))
}

fn write_declarations(out: &mut Vec<u8>, fact: &Fact, limits: &DecoderLimits) -> Result<(), SegmentError> {
    let count = u32::try_from(fact.declarations.len()).map_err(|_| SegmentError::Limit("records"))?;
    if u64::from(count) > limits.max_records {
        return Err(SegmentError::Limit("records"));
    }
    push_u32(out, count);
    for declaration in &fact.declarations {
        push_u32(out, declaration.name);
        push_u16(out, declaration.kind.code());
        push_u64(out, declaration.span_start);
        push_u64(out, declaration.span_end);
        push_u32(out, declaration.summary);
        push_narrow(out, declaration.summary_source.code())?;
        push_bool(out, declaration.public);
    }
    Ok(())
}

fn write_refs(out: &mut Vec<u8>, fact: &Fact, limits: &DecoderLimits) -> Result<(), SegmentError> {
    let count = u32::try_from(fact.raw_refs.len()).map_err(|_| SegmentError::Limit("records"))?;
    if u64::from(count) > limits.max_records {
        return Err(SegmentError::Limit("records"));
    }
    push_u32(out, count);
    for reference in &fact.raw_refs {
        push_u32(out, reference.src);
        push_u16(out, reference.kind.code());
        push_u32(out, reference.spelling);
        push_u32(out, reference.scope);
        if let Some(alias) = reference.alias {
            push_bool(out, true);
            push_u32(out, alias);
        } else {
            push_bool(out, false);
            push_u32(out, 0);
        }
        push_u32(out, reference.line_start);
        push_u32(out, reference.line_end);
    }
    Ok(())
}

fn write_provenance(out: &mut Vec<u8>, provenance: &Provenance) -> Result<(), SegmentError> {
    push_narrow(out, provenance.coverage.code())?;
    push_narrow(out, provenance.completion.code())?;
    push_u32(out, provenance.completion_reason);
    push_u32(out, provenance.extractor);
    push_u64(out, provenance.generation);
    if let Some(model) = provenance.model {
        push_bool(out, true);
        push_u32(out, model);
    } else {
        push_bool(out, false);
        push_u32(out, 0);
    }
    if let Some(digest) = provenance.body_fingerprint {
        push_bool(out, true);
        push_bytes(out, &digest);
    } else {
        push_bool(out, false);
        push_bytes(out, &[0u8; 32]);
    }
    Ok(())
}

pub(crate) fn decode<S: AsRef<str>>(
    payload: &[u8],
    record_count: u64,
    dictionary: &[S],
    limits: &DecoderLimits,
) -> Result<Vec<Fact>, SegmentError> {
    if payload.is_empty() {
        if record_count != 0 {
            return Err(SegmentError::Cardinality("count"));
        }
        return Ok(Vec::new());
    }
    let available = u64::try_from(payload.len()).unwrap_or(u64::MAX);
    let room = bounded_count(record_count, 4, available, limits.max_records)?;
    let mut cursor = Cursor::new(payload, 0);
    let mut facts = Vec::with_capacity(room);
    let mut previous: Option<Vec<u8>> = None;
    for _ in 0..record_count {
        let rec_len = cursor.u32()?;
        if rec_len > limits.max_record_bytes {
            return Err(SegmentError::Limit("record"));
        }
        let record = cursor.take(usize::try_from(rec_len).map_err(|_| SegmentError::Arithmetic)?)?;
        let fact = read_fact(record, dictionary, limits)?;
        let raw_key = encode_key(&fact.key);
        if let Some(prior) = &previous {
            match raw_key.cmp(prior) {
                Ordering::Greater => {}
                Ordering::Equal => return Err(SegmentError::Facts("duplicate")),
                Ordering::Less => return Err(SegmentError::Facts("order")),
            }
        }
        previous = Some(raw_key);
        facts.push(fact);
    }
    if !cursor.rest().is_empty() {
        return Err(SegmentError::Facts("trailing"));
    }
    Ok(facts)
}

fn read_fact<S: AsRef<str>>(
    record: &[u8],
    dictionary: &[S],
    limits: &DecoderLimits,
) -> Result<Fact, SegmentError> {
    let mut cursor = Cursor::new(record, 0);
    let key = parse_key(&mut cursor)?;
    let decl_count = cursor.u32()?;
    let decl_room = bounded_count(
        u64::from(decl_count),
        28,
        u64::try_from(cursor.rest().len()).unwrap_or(u64::MAX),
        limits.max_records,
    )?;
    let mut declarations = Vec::with_capacity(decl_room);
    for _ in 0..decl_count {
        declarations.push(read_declaration(&mut cursor)?);
    }
    let raw_count = cursor.u32()?;
    let raw_room = bounded_count(
        u64::from(raw_count),
        27,
        u64::try_from(cursor.rest().len()).unwrap_or(u64::MAX),
        limits.max_records,
    )?;
    let mut raw_refs = Vec::with_capacity(raw_room);
    for _ in 0..raw_count {
        raw_refs.push(read_ref(&mut cursor)?);
    }
    let provenance = read_provenance(&mut cursor)?;
    if !cursor.rest().is_empty() {
        return Err(SegmentError::Facts("trailing"));
    }
    let fact = Fact {
        key,
        declarations,
        raw_refs,
        provenance,
    };
    validate_fact(&fact, dictionary)?;
    Ok(fact)
}

fn read_declaration(cursor: &mut Cursor<'_>) -> Result<Declaration, SegmentError> {
    Ok(Declaration {
        name: cursor.u32()?,
        kind: DeclarationKind::from_code(cursor.u16()?).ok_or(SegmentError::Facts("kind"))?,
        span_start: cursor.u64()?,
        span_end: cursor.u64()?,
        summary: cursor.u32()?,
        summary_source: SummarySource::from_code(u16::from(cursor.u8()?))
            .ok_or(SegmentError::Facts("kind"))?,
        public: cursor.flag()?,
    })
}

fn read_ref(cursor: &mut Cursor<'_>) -> Result<RawRef, SegmentError> {
    let src = cursor.u32()?;
    let kind = EdgeKind::from_code(cursor.u16()?).ok_or(SegmentError::Facts("kind"))?;
    let spelling = cursor.u32()?;
    let scope = cursor.u32()?;
    let has_alias = cursor.flag()?;
    let alias_id = cursor.u32()?;
    let alias = if has_alias {
        Some(alias_id)
    } else if alias_id != 0 {
        return Err(SegmentError::Facts("canonical"));
    } else {
        None
    };
    Ok(RawRef {
        src,
        kind,
        spelling,
        scope,
        alias,
        line_start: cursor.u32()?,
        line_end: cursor.u32()?,
    })
}

fn read_provenance(cursor: &mut Cursor<'_>) -> Result<Provenance, SegmentError> {
    let coverage = Coverage::from_code(u16::from(cursor.u8()?)).ok_or(SegmentError::Facts("kind"))?;
    let completion =
        Completion::from_code(u16::from(cursor.u8()?)).ok_or(SegmentError::Facts("kind"))?;
    let completion_reason = cursor.u32()?;
    let extractor = cursor.u32()?;
    let generation = cursor.u64()?;
    let has_model = cursor.flag()?;
    let model_id = cursor.u32()?;
    let model = if has_model {
        Some(model_id)
    } else if model_id != 0 {
        return Err(SegmentError::Facts("canonical"));
    } else {
        None
    };
    let has_body = cursor.flag()?;
    let body = cursor.array::<32>()?;
    let body_fingerprint = if has_body {
        Some(body)
    } else if body != [0; 32] {
        return Err(SegmentError::Facts("canonical"));
    } else {
        None
    };
    Ok(Provenance {
        coverage,
        completion,
        completion_reason,
        extractor,
        generation,
        model,
        body_fingerprint,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::facts::{Declaration, FactKey, Provenance};
    use crate::domain::kinds::{Coverage, DeclarationKind, Language, SummarySource};

    fn sample() -> Fact {
        Fact {
            key: FactKey {
                digest: [1; 32],
                byte_length: 4,
                language: Language::Rust,
                dialect: 0,
                parser_id: 0,
                parser_revision: 0,
                extraction_plan: [2; 32],
            },
            declarations: vec![Declaration {
                name: 0,
                kind: DeclarationKind::Function,
                span_start: 0,
                span_end: 4,
                summary: 0,
                summary_source: SummarySource::Signature,
                public: true,
            }],
            raw_refs: vec![],
            provenance: Provenance {
                coverage: Coverage::Syntax,
                completion: Completion::Complete,
                completion_reason: 0,
                extractor: 0,
                generation: 3,
                model: None,
                body_fingerprint: None,
            },
        }
    }

    #[test]
    fn records_round_trip_and_a_span_past_the_object_is_refused() {
        let limits = DecoderLimits::standard();
        let dictionary = ["name"];
        let fact = sample();
        let (payload, count) = encode(std::slice::from_ref(&fact), &limits).unwrap();
        assert_eq!(decode(&payload, count, &dictionary, &limits).unwrap(), vec![fact.clone()]);
        let mut wide = fact.clone();
        wide.declarations[0].span_end = 9;
        let (bad, bad_count) = encode(&[wide], &limits).unwrap();
        assert_eq!(
            decode(&bad, bad_count, &dictionary, &limits).unwrap_err(),
            SegmentError::Facts("span")
        );
        assert_eq!(
            encode(&[fact.clone(), fact], &limits).unwrap_err(),
            SegmentError::Facts("duplicate")
        );
    }
}
