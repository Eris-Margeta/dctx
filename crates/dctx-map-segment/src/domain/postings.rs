// SPDX-License-Identifier: Apache-2.0
//! Delta-coded field postings and per-document token lengths.
//! Implements SEGMENTS-v1 kind 4. Deltas are canonical base-128 and strictly increasing after the first.
//! Depends on [`super::varint`] and [`super::kinds::Field`]. Ranking reads the token lengths from here.
//! Path rows are refused when the segment role is fact. Zero-length sections mean no documents.

use super::cursor::{Cursor, push_narrow, push_record, push_u32};
use super::error::SegmentError;
use super::kinds::{Field, SegmentRole};
use super::limits::{DecoderLimits, bounded_count};
use super::varint::{push_varint, read_varint};

/// One document's positions for a term. Positions are absolute, not deltas.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TermHit {
    /// Document index into [`Postings::lengths`].
    pub doc: u32,
    /// Absolute token positions, strictly increasing.
    pub positions: Vec<u32>,
}

/// One field/term row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TermRow {
    /// Identifier, path, or doc.
    pub field: Field,
    /// Dictionary id of the term.
    pub term_id: u32,
    /// Documents that contain the term.
    pub hits: Vec<TermHit>,
}

/// Document lengths and term rows. Lengths are ranking inputs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Postings {
    /// Token length of each document, indexed by document id.
    pub lengths: Vec<u32>,
    /// Rows sorted by field tag, then term id.
    pub rows: Vec<TermRow>,
}

impl Postings {
    /// Finds the row for `field` and `term_id`.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn row(&self, field: Field, term_id: u32) -> Option<&TermRow> {
        self.rows
            .binary_search_by(|row| (row.field.code(), row.term_id).cmp(&(field.code(), term_id)))
            .ok()
            .and_then(|index| self.rows.get(index))
    }
}

pub(crate) fn encode(
    postings: &Postings,
    role: SegmentRole,
    limits: &DecoderLimits,
) -> Result<(Vec<u8>, u64), SegmentError> {
    if postings.lengths.is_empty() && postings.rows.is_empty() {
        return Ok((Vec::new(), 0));
    }
    if postings.lengths.is_empty() {
        return Err(SegmentError::Postings("length"));
    }
    let mut out = Vec::new();
    let doc_count =
        u32::try_from(postings.lengths.len()).map_err(|_| SegmentError::Limit("records"))?;
    push_u32(&mut out, doc_count);
    for length in &postings.lengths {
        push_u32(&mut out, *length);
    }
    let mut previous: Option<(u16, u32)> = None;
    for row in &postings.rows {
        if role == SegmentRole::Fact && row.field == Field::Path {
            return Err(SegmentError::Postings("path"));
        }
        let key = (row.field.code(), row.term_id);
        if previous.is_some_and(|prior| key <= prior) {
            return Err(SegmentError::Postings("order"));
        }
        previous = Some(key);
        let mut body = Vec::new();
        push_narrow(&mut body, row.field.code())?;
        push_u32(&mut body, row.term_id);
        let df = u32::try_from(row.hits.len()).map_err(|_| SegmentError::Limit("records"))?;
        if df == 0 {
            return Err(SegmentError::Postings("frequency"));
        }
        push_u32(&mut body, df);
        write_hits(&mut body, row, &postings.lengths)?;
        push_record(&mut out, &body, limits.max_record_bytes)?;
    }
    Ok((
        out,
        u64::try_from(postings.rows.len()).map_err(|_| SegmentError::Arithmetic)?,
    ))
}

fn write_hits(out: &mut Vec<u8>, row: &TermRow, lengths: &[u32]) -> Result<(), SegmentError> {
    let mut previous_doc: Option<u32> = None;
    for hit in &row.hits {
        let doc = match previous_doc {
            None => hit.doc,
            Some(prior) if hit.doc > prior => hit.doc,
            Some(_) => return Err(SegmentError::Postings("zero delta")),
        };
        let length = lengths
            .get(usize::try_from(doc).map_err(|_| SegmentError::Arithmetic)?)
            .copied()
            .ok_or(SegmentError::Postings("doc"))?;
        // The first document is stored as an absolute id. Later documents store a positive delta.
        push_varint(out, u64::from(doc - previous_doc.unwrap_or(0)));
        previous_doc = Some(doc);
        write_positions(out, &hit.positions, length)?;
    }
    Ok(())
}

fn write_positions(out: &mut Vec<u8>, positions: &[u32], length: u32) -> Result<(), SegmentError> {
    if positions.is_empty() {
        return Err(SegmentError::Postings("frequency"));
    }
    push_varint(
        out,
        u64::try_from(positions.len()).map_err(|_| SegmentError::Arithmetic)?,
    );
    let mut previous: Option<u32> = None;
    for position in positions {
        if *position >= length {
            return Err(SegmentError::Postings("position"));
        }
        let delta = match previous {
            None => u64::from(*position),
            Some(prior) if *position > prior => u64::from(position - prior),
            Some(_) => return Err(SegmentError::Postings("zero delta")),
        };
        push_varint(out, delta);
        previous = Some(*position);
    }
    Ok(())
}

pub(crate) fn decode(
    payload: &[u8],
    record_count: u64,
    dict_len: u32,
    role: SegmentRole,
    limits: &DecoderLimits,
) -> Result<Postings, SegmentError> {
    if payload.is_empty() {
        if record_count != 0 {
            return Err(SegmentError::Cardinality("count"));
        }
        return Ok(Postings {
            lengths: Vec::new(),
            rows: Vec::new(),
        });
    }
    let mut cursor = Cursor::new(payload, 0);
    let doc_count = cursor.u32()?;
    if doc_count == 0 {
        return Err(SegmentError::Postings("empty"));
    }
    let available = u64::try_from(cursor.rest().len()).unwrap_or(u64::MAX);
    let room = bounded_count(u64::from(doc_count), 4, available, limits.max_records)?;
    let mut lengths = Vec::with_capacity(room);
    for _ in 0..doc_count {
        lengths.push(cursor.u32()?);
    }
    let row_available = u64::try_from(cursor.rest().len()).unwrap_or(u64::MAX);
    let row_room = bounded_count(record_count, 4, row_available, limits.max_records)?;
    let mut rows = Vec::with_capacity(row_room);
    let mut previous: Option<(u16, u32)> = None;
    for _ in 0..record_count {
        let rec_len = cursor.u32()?;
        if rec_len > limits.max_record_bytes {
            return Err(SegmentError::Limit("record"));
        }
        let record =
            cursor.take(usize::try_from(rec_len).map_err(|_| SegmentError::Arithmetic)?)?;
        let row = read_row(record, &lengths, dict_len, role, limits)?;
        let key = (row.field.code(), row.term_id);
        if previous.is_some_and(|prior| key <= prior) {
            return Err(SegmentError::Postings("order"));
        }
        previous = Some(key);
        rows.push(row);
    }
    if !cursor.rest().is_empty() {
        return Err(SegmentError::Postings("trailing"));
    }
    Ok(Postings { lengths, rows })
}

fn read_row(
    record: &[u8],
    lengths: &[u32],
    dict_len: u32,
    role: SegmentRole,
    limits: &DecoderLimits,
) -> Result<TermRow, SegmentError> {
    let mut cursor = Cursor::new(record, 0);
    let field = Field::from_code(u16::from(cursor.u8()?)).ok_or(SegmentError::Postings("field"))?;
    if role == SegmentRole::Fact && field == Field::Path {
        return Err(SegmentError::Postings("path"));
    }
    let term_id = cursor.u32()?;
    if term_id >= dict_len {
        return Err(SegmentError::Postings("term"));
    }
    let df = cursor.u32()?;
    if df == 0 {
        return Err(SegmentError::Postings("frequency"));
    }
    let available = u64::try_from(cursor.rest().len()).unwrap_or(u64::MAX);
    let room = bounded_count(u64::from(df), 2, available, limits.max_records)?;
    let mut hits = Vec::with_capacity(room);
    let mut previous_doc: Option<u32> = None;
    for _ in 0..df {
        let delta = take_varint(&mut cursor)?;
        let doc = match previous_doc {
            None => u32::try_from(delta).map_err(|_| SegmentError::Postings("doc"))?,
            Some(_) if delta == 0 => return Err(SegmentError::Postings("zero delta")),
            Some(prior) => {
                let step = u32::try_from(delta).map_err(|_| SegmentError::Postings("doc"))?;
                prior.checked_add(step).ok_or(SegmentError::Arithmetic)?
            }
        };
        let length = lengths
            .get(usize::try_from(doc).map_err(|_| SegmentError::Arithmetic)?)
            .copied()
            .ok_or(SegmentError::Postings("doc"))?;
        hits.push(TermHit {
            doc,
            positions: read_positions(&mut cursor, length, limits)?,
        });
        previous_doc = Some(doc);
    }
    if !cursor.rest().is_empty() {
        return Err(SegmentError::Postings("trailing"));
    }
    Ok(TermRow {
        field,
        term_id,
        hits,
    })
}

fn read_positions(
    cursor: &mut Cursor<'_>,
    length: u32,
    limits: &DecoderLimits,
) -> Result<Vec<u32>, SegmentError> {
    let tf = take_varint(cursor)?;
    if tf == 0 {
        return Err(SegmentError::Postings("frequency"));
    }
    let available = u64::try_from(cursor.rest().len()).unwrap_or(u64::MAX);
    let room = bounded_count(tf, 1, available, limits.max_records)?;
    let mut positions = Vec::with_capacity(room);
    let mut previous: Option<u32> = None;
    for _ in 0..tf {
        let delta = take_varint(cursor)?;
        if previous.is_some() && delta == 0 {
            return Err(SegmentError::Postings("zero delta"));
        }
        let step = u32::try_from(delta).map_err(|_| SegmentError::Postings("position"))?;
        let position = match previous {
            None => step,
            Some(prior) => prior.checked_add(step).ok_or(SegmentError::Arithmetic)?,
        };
        if position >= length {
            return Err(SegmentError::Postings("position"));
        }
        positions.push(position);
        previous = Some(position);
    }
    Ok(positions)
}

fn take_varint(cursor: &mut Cursor<'_>) -> Result<u64, SegmentError> {
    let (value, size) = read_varint(cursor.rest())?;
    cursor.take(size)?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits() -> DecoderLimits {
        DecoderLimits::standard()
    }

    #[test]
    fn one_identifier_row_matches_the_byte_golden() {
        let postings = Postings {
            lengths: vec![2],
            rows: vec![TermRow {
                field: Field::Identifier,
                term_id: 0,
                hits: vec![TermHit {
                    doc: 0,
                    positions: vec![1],
                }],
            }],
        };
        let (payload, count) = encode(&postings, SegmentRole::Fact, &limits()).unwrap();
        assert_eq!(count, 1);
        assert_eq!(
            payload,
            [
                0x01, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x0c, 0x00, 0x00, 0x00, 0x01, 0x00,
                0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x01, 0x01,
            ]
        );
        let decoded = decode(&payload, count, 1, SegmentRole::Fact, &limits()).unwrap();
        assert_eq!(decoded, postings);
        assert_eq!(
            encode(&postings, SegmentRole::Fact, &limits()).unwrap().0,
            payload
        );
    }

    #[test]
    fn a_zero_position_delta_is_refused() {
        let payload = [
            0x01, 0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00, 0x0d, 0x00, 0x00, 0x00, 0x01, 0x00,
            0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00,
        ];
        assert_eq!(
            decode(&payload, 1, 1, SegmentRole::Fact, &limits()).unwrap_err(),
            SegmentError::Postings("zero delta")
        );
        let ok = Postings {
            lengths: vec![1, 3],
            rows: vec![TermRow {
                field: Field::Identifier,
                term_id: 0,
                hits: vec![
                    TermHit {
                        doc: 0,
                        positions: vec![0],
                    },
                    TermHit {
                        doc: 1,
                        positions: vec![0, 2],
                    },
                ],
            }],
        };
        let (payload, count) = encode(&ok, SegmentRole::View, &limits()).unwrap();
        assert_eq!(
            decode(&payload, count, 1, SegmentRole::View, &limits()).unwrap(),
            ok
        );
    }
}
