// SPDX-License-Identifier: Apache-2.0
//! Forward edges and the reverse index derived from them.
//! Implements SEGMENTS-v1 kinds 6 and 7. Reverse bytes are a pure function of forward bytes.
//! Depends on [`super::kinds`] for the confidence matrix. The decoder recomputes reverse and compares.
//! Empty adjacency is a zero-length section. Self-edges are allowed. `contains` raw refs are not.

use super::cursor::{push_narrow, push_record, push_u16, push_u32, push_u8, Cursor};
use super::dictionary::dict_str;
use super::error::SegmentError;
use super::kinds::{Confidence, EdgeKind};
use super::limits::{bounded_count, DecoderLimits};

const TAG_RESOLVED: u8 = 1;
const TAG_UNRESOLVED: u8 = 2;

/// The far end of a forward edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// Dense id in this view segment.
    Resolved(u32),
    /// Dictionary id of an unresolved spelling.
    Unresolved(u32),
}

/// One forward edge from a dense source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ForwardEdge {
    /// Dense id of the source node.
    pub source: u32,
    /// Relation.
    pub kind: EdgeKind,
    /// Confidence permitted for [`Self::kind`].
    pub confidence: Confidence,
    /// Resolved node or unresolved spelling.
    pub target: Target,
}

/// A resolved edge indexed by its target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolvedReverse {
    /// Dense id of the target.
    pub target: u32,
    /// Relation.
    pub kind: EdgeKind,
    /// Confidence copied from the forward edge.
    pub confidence: Confidence,
    /// Dense id of the source.
    pub source: u32,
}

/// An unresolved spelling indexed for dependency repair.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnresolvedReverse {
    /// Dictionary id of the spelling.
    pub name: u32,
    /// Relation.
    pub kind: EdgeKind,
    /// Confidence copied from the forward edge.
    pub confidence: Confidence,
    /// Dense id of the source.
    pub source: u32,
}

pub(crate) fn forward_key(edge: &ForwardEdge) -> (u32, u16, u16, u8, u32, u32) {
    let (tag, target, name) = match edge.target {
        Target::Resolved(id) => (TAG_RESOLVED, id, 0),
        Target::Unresolved(id) => (TAG_UNRESOLVED, 0, id),
    };
    (edge.source, edge.kind.code(), edge.confidence.code(), tag, target, name)
}

/// Sorted reverse rows for `forward`. Resolved rows precede unresolved rows on the wire.
pub(crate) fn reverse_of(forward: &[ForwardEdge]) -> (Vec<ResolvedReverse>, Vec<UnresolvedReverse>) {
    let mut resolved = Vec::new();
    let mut unresolved = Vec::new();
    for edge in forward {
        match edge.target {
            Target::Resolved(target) => resolved.push(ResolvedReverse {
                target,
                kind: edge.kind,
                confidence: edge.confidence,
                source: edge.source,
            }),
            Target::Unresolved(name) => unresolved.push(UnresolvedReverse {
                name,
                kind: edge.kind,
                confidence: edge.confidence,
                source: edge.source,
            }),
        }
    }
    resolved.sort_by_key(resolved_key);
    unresolved.sort_by_key(unresolved_key);
    (resolved, unresolved)
}

fn resolved_key(row: &ResolvedReverse) -> (u32, u16, u16, u32) {
    (row.target, row.kind.code(), row.confidence.code(), row.source)
}

fn unresolved_key(row: &UnresolvedReverse) -> (u32, u16, u16, u32) {
    (row.name, row.kind.code(), row.confidence.code(), row.source)
}

pub(crate) fn encode_forward<S: AsRef<str>>(
    edges: &[ForwardEdge],
    node_count: u32,
    dictionary: &[S],
    limits: &DecoderLimits,
) -> Result<(Vec<u8>, u64), SegmentError> {
    if edges.is_empty() {
        return Ok((Vec::new(), 0));
    }
    guard_len(edges.len(), limits)?;
    let mut out = Vec::new();
    let mut previous: Option<(u32, u16, u16, u8, u32, u32)> = None;
    for edge in edges {
        check_forward(edge, node_count, dictionary)?;
        let key = forward_key(edge);
        match previous {
            Some(prior) if key == prior => return Err(SegmentError::Adjacency("duplicate")),
            Some(prior) if key < prior => return Err(SegmentError::Adjacency("order")),
            _ => {}
        }
        previous = Some(key);
        let mut body = Vec::new();
        push_u32(&mut body, edge.source);
        push_u16(&mut body, edge.kind.code());
        push_narrow(&mut body, edge.confidence.code())?;
        let (tag, target, name) = match edge.target {
            Target::Resolved(id) => (TAG_RESOLVED, id, 0),
            Target::Unresolved(id) => (TAG_UNRESOLVED, 0, id),
        };
        push_u8(&mut body, tag);
        push_u32(&mut body, target);
        push_u32(&mut body, name);
        push_record(&mut out, &body, limits.max_record_bytes)?;
    }
    Ok((out, u64::try_from(edges.len()).map_err(|_| SegmentError::Arithmetic)?))
}

pub(crate) fn encode_reverse(
    resolved: &[ResolvedReverse],
    unresolved: &[UnresolvedReverse],
    limits: &DecoderLimits,
) -> Result<(Vec<u8>, u64), SegmentError> {
    if resolved.is_empty() && unresolved.is_empty() {
        return Ok((Vec::new(), 0));
    }
    let resolved_len = u64::try_from(resolved.len()).map_err(|_| SegmentError::Arithmetic)?;
    let unresolved_len = u64::try_from(unresolved.len()).map_err(|_| SegmentError::Arithmetic)?;
    let total = resolved_len.checked_add(unresolved_len).ok_or(SegmentError::Arithmetic)?;
    if total > limits.max_records {
        return Err(SegmentError::Limit("records"));
    }
    let mut out = Vec::new();
    push_u32(&mut out, u32::try_from(resolved.len()).map_err(|_| SegmentError::Arithmetic)?);
    write_resolved(&mut out, resolved, limits)?;
    push_u32(&mut out, u32::try_from(unresolved.len()).map_err(|_| SegmentError::Arithmetic)?);
    write_unresolved(&mut out, unresolved, limits)?;
    Ok((out, total))
}

pub(crate) fn decode_forward<S: AsRef<str>>(
    payload: &[u8],
    record_count: u64,
    node_count: u32,
    dictionary: &[S],
    limits: &DecoderLimits,
) -> Result<Vec<ForwardEdge>, SegmentError> {
    if payload.is_empty() {
        if record_count != 0 {
            return Err(SegmentError::Cardinality("count"));
        }
        return Ok(Vec::new());
    }
    let mut cursor = Cursor::new(payload, 0);
    let room = bounded_count(
        record_count,
        4,
        u64::try_from(payload.len()).unwrap_or(u64::MAX),
        limits.max_records,
    )?;
    let mut edges = Vec::with_capacity(room);
    let mut previous: Option<(u32, u16, u16, u8, u32, u32)> = None;
    for _ in 0..record_count {
        let edge = read_forward_record(&mut cursor, node_count, dictionary, limits)?;
        let key = forward_key(&edge);
        match previous {
            Some(prior) if key == prior => return Err(SegmentError::Adjacency("duplicate")),
            Some(prior) if key < prior => return Err(SegmentError::Adjacency("order")),
            _ => {}
        }
        previous = Some(key);
        edges.push(edge);
    }
    if !cursor.rest().is_empty() {
        return Err(SegmentError::Adjacency("order"));
    }
    Ok(edges)
}

pub(crate) fn decode_reverse<S: AsRef<str>>(
    payload: &[u8],
    record_count: u64,
    node_count: u32,
    dictionary: &[S],
    limits: &DecoderLimits,
) -> Result<(Vec<ResolvedReverse>, Vec<UnresolvedReverse>), SegmentError> {
    if payload.is_empty() {
        if record_count != 0 {
            return Err(SegmentError::Cardinality("count"));
        }
        return Ok((Vec::new(), Vec::new()));
    }
    let mut cursor = Cursor::new(payload, 0);
    let resolved_count = cursor.u32()?;
    let resolved = read_resolved(&mut cursor, resolved_count, node_count, limits)?;
    let unresolved_count = cursor.u32()?;
    let unresolved = read_unresolved(&mut cursor, unresolved_count, node_count, dictionary, limits)?;
    if !cursor.rest().is_empty() {
        return Err(SegmentError::Adjacency("order"));
    }
    let total = u64::from(resolved_count)
        .checked_add(u64::from(unresolved_count))
        .ok_or(SegmentError::Arithmetic)?;
    if total != record_count {
        return Err(SegmentError::Cardinality("count"));
    }
    if resolved.is_empty() && unresolved.is_empty() {
        return Err(SegmentError::Adjacency("canonical"));
    }
    Ok((resolved, unresolved))
}

fn check_forward<S: AsRef<str>>(
    edge: &ForwardEdge,
    node_count: u32,
    dictionary: &[S],
) -> Result<(), SegmentError> {
    if edge.source >= node_count {
        return Err(SegmentError::Adjacency("node"));
    }
    if !edge.kind.permits(edge.confidence) {
        return Err(SegmentError::Adjacency("confidence"));
    }
    match edge.target {
        Target::Resolved(id) if id >= node_count => Err(SegmentError::Adjacency("node")),
        Target::Resolved(_) => Ok(()),
        Target::Unresolved(id) => {
            let name = dict_str(dictionary, id).map_err(|_| SegmentError::Adjacency("name"))?;
            if name.is_empty() {
                return Err(SegmentError::Adjacency("name"));
            }
            Ok(())
        }
    }
}

fn write_resolved(
    out: &mut Vec<u8>,
    rows: &[ResolvedReverse],
    limits: &DecoderLimits,
) -> Result<(), SegmentError> {
    let mut previous: Option<(u32, u16, u16, u32)> = None;
    for row in rows {
        let key = resolved_key(row);
        if previous.is_some_and(|prior| key <= prior) {
            return Err(SegmentError::Adjacency("order"));
        }
        previous = Some(key);
        let mut body = Vec::new();
        push_u32(&mut body, row.target);
        push_u16(&mut body, row.kind.code());
        push_narrow(&mut body, row.confidence.code())?;
        push_u32(&mut body, row.source);
        push_record(out, &body, limits.max_record_bytes)?;
    }
    Ok(())
}

fn write_unresolved(
    out: &mut Vec<u8>,
    rows: &[UnresolvedReverse],
    limits: &DecoderLimits,
) -> Result<(), SegmentError> {
    let mut previous: Option<(u32, u16, u16, u32)> = None;
    for row in rows {
        let key = unresolved_key(row);
        if previous.is_some_and(|prior| key <= prior) {
            return Err(SegmentError::Adjacency("order"));
        }
        previous = Some(key);
        let mut body = Vec::new();
        push_u32(&mut body, row.name);
        push_u16(&mut body, row.kind.code());
        push_narrow(&mut body, row.confidence.code())?;
        push_u32(&mut body, row.source);
        push_record(out, &body, limits.max_record_bytes)?;
    }
    Ok(())
}

fn read_forward_record<S: AsRef<str>>(
    cursor: &mut Cursor<'_>,
    node_count: u32,
    dictionary: &[S],
    limits: &DecoderLimits,
) -> Result<ForwardEdge, SegmentError> {
    let rec_len = cursor.u32()?;
    if rec_len > limits.max_record_bytes {
        return Err(SegmentError::Limit("record"));
    }
    let record = cursor.take(usize::try_from(rec_len).map_err(|_| SegmentError::Arithmetic)?)?;
    if record.len() != 16 {
        return Err(SegmentError::Adjacency("canonical"));
    }
    let mut body = Cursor::new(record, 0);
    let source = body.u32()?;
    let kind = EdgeKind::from_code(body.u16()?).ok_or(SegmentError::Adjacency("canonical"))?;
    let confidence = Confidence::from_code(u16::from(body.u8()?)).ok_or(SegmentError::Adjacency("confidence"))?;
    let tag = body.u8()?;
    let target_dense = body.u32()?;
    let name_id = body.u32()?;
    let target = match tag {
        TAG_RESOLVED if name_id == 0 => Target::Resolved(target_dense),
        TAG_UNRESOLVED if target_dense == 0 => Target::Unresolved(name_id),
        _ => return Err(SegmentError::Adjacency("canonical")),
    };
    let edge = ForwardEdge { source, kind, confidence, target };
    check_forward(&edge, node_count, dictionary)?;
    Ok(edge)
}

fn read_resolved(
    cursor: &mut Cursor<'_>,
    count: u32,
    node_count: u32,
    limits: &DecoderLimits,
) -> Result<Vec<ResolvedReverse>, SegmentError> {
    let available = u64::try_from(cursor.rest().len()).unwrap_or(u64::MAX);
    let room = bounded_count(u64::from(count), 4, available, limits.max_records)?;
    let mut rows = Vec::with_capacity(room);
    let mut previous: Option<(u32, u16, u16, u32)> = None;
    for _ in 0..count {
        let record = take_fixed(cursor, 11, limits)?;
        let mut body = Cursor::new(record, 0);
        let row = ResolvedReverse {
            target: body.u32()?,
            kind: EdgeKind::from_code(body.u16()?).ok_or(SegmentError::Adjacency("canonical"))?,
            confidence: Confidence::from_code(u16::from(body.u8()?))
                .ok_or(SegmentError::Adjacency("confidence"))?,
            source: body.u32()?,
        };
        if row.target >= node_count || row.source >= node_count {
            return Err(SegmentError::Adjacency("node"));
        }
        let key = resolved_key(&row);
        if previous.is_some_and(|prior| key <= prior) {
            return Err(SegmentError::Adjacency("order"));
        }
        previous = Some(key);
        rows.push(row);
    }
    Ok(rows)
}

fn read_unresolved<S: AsRef<str>>(
    cursor: &mut Cursor<'_>,
    count: u32,
    node_count: u32,
    dictionary: &[S],
    limits: &DecoderLimits,
) -> Result<Vec<UnresolvedReverse>, SegmentError> {
    let available = u64::try_from(cursor.rest().len()).unwrap_or(u64::MAX);
    let room = bounded_count(u64::from(count), 4, available, limits.max_records)?;
    let mut rows = Vec::with_capacity(room);
    let mut previous: Option<(u32, u16, u16, u32)> = None;
    for _ in 0..count {
        let record = take_fixed(cursor, 11, limits)?;
        let mut body = Cursor::new(record, 0);
        let row = UnresolvedReverse {
            name: body.u32()?,
            kind: EdgeKind::from_code(body.u16()?).ok_or(SegmentError::Adjacency("canonical"))?,
            confidence: Confidence::from_code(u16::from(body.u8()?))
                .ok_or(SegmentError::Adjacency("confidence"))?,
            source: body.u32()?,
        };
        if row.source >= node_count {
            return Err(SegmentError::Adjacency("node"));
        }
        let name = dict_str(dictionary, row.name).map_err(|_| SegmentError::Adjacency("name"))?;
        if name.is_empty() {
            return Err(SegmentError::Adjacency("name"));
        }
        let key = unresolved_key(&row);
        if previous.is_some_and(|prior| key <= prior) {
            return Err(SegmentError::Adjacency("order"));
        }
        previous = Some(key);
        rows.push(row);
    }
    Ok(rows)
}

fn take_fixed<'a>(
    cursor: &mut Cursor<'a>,
    len: usize,
    limits: &DecoderLimits,
) -> Result<&'a [u8], SegmentError> {
    let rec_len = cursor.u32()?;
    if rec_len > limits.max_record_bytes {
        return Err(SegmentError::Limit("record"));
    }
    let record = cursor.take(usize::try_from(rec_len).map_err(|_| SegmentError::Arithmetic)?)?;
    if record.len() != len {
        return Err(SegmentError::Adjacency("canonical"));
    }
    Ok(record)
}

fn guard_len(len: usize, limits: &DecoderLimits) -> Result<(), SegmentError> {
    if u64::try_from(len).unwrap_or(u64::MAX) > limits.max_records {
        return Err(SegmentError::Limit("records"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reverse_is_a_pure_function_of_forward() {
        let dictionary = ["missing"];
        let limits = DecoderLimits::standard();
        let forward = [
            ForwardEdge {
                source: 0,
                kind: EdgeKind::Imports,
                confidence: Confidence::Exact,
                target: Target::Resolved(1),
            },
            ForwardEdge {
                source: 1,
                kind: EdgeKind::Calls,
                confidence: Confidence::Name,
                target: Target::Unresolved(0),
            },
        ];
        let (payload, count) = encode_forward(&forward, 2, &dictionary, &limits).unwrap();
        assert_eq!(decode_forward(&payload, count, 2, &dictionary, &limits).unwrap(), forward);
        let (resolved, unresolved) = reverse_of(&forward);
        let (rev_payload, rev_count) = encode_reverse(&resolved, &unresolved, &limits).unwrap();
        assert_eq!(
            decode_reverse(&rev_payload, rev_count, 2, &dictionary, &limits).unwrap(),
            (resolved, unresolved)
        );
        assert_eq!(encode_reverse(&[], &[], &limits).unwrap().0, [] as [u8; 0]);
        let illegal = ForwardEdge {
            source: 0,
            kind: EdgeKind::Contains,
            confidence: Confidence::Guess,
            target: Target::Resolved(0),
        };
        assert_eq!(
            encode_forward(&[illegal], 1, &dictionary, &limits).unwrap_err(),
            SegmentError::Adjacency("confidence")
        );
    }
}
