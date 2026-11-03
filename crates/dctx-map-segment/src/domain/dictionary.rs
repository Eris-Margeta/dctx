// SPDX-License-Identifier: Apache-2.0
//! Sorted unique UTF-8 dictionary. Exact lookup is a binary search.
//! Implements SEGMENTS-v1 kind 1. An empty section is zero bytes, not a zero count.
//! Depends on the cursor and the record cap. Facts, nodes and postings store ids into this table.
//! Strings are borrowed from the section payload. The decoder checks order before it returns them.

use std::collections::BTreeSet;

use super::cursor::{push_u32, Cursor};
use super::error::SegmentError;
use super::limits::{bounded_count, DecoderLimits};

pub(crate) struct OwnedStrings {
    strings: Vec<String>,
}

impl OwnedStrings {
    pub(crate) fn from_set(set: BTreeSet<String>, limits: &DecoderLimits) -> Result<Self, SegmentError> {
        if offset_len(set.len())? > limits.max_records {
            return Err(SegmentError::Limit("records"));
        }
        for value in &set {
            if too_long(value, limits) {
                return Err(SegmentError::Limit("string"));
            }
        }
        Ok(Self {
            strings: set.into_iter().collect(),
        })
    }

    pub(crate) fn as_slice(&self) -> &[String] {
        &self.strings
    }

    pub(crate) fn id(&self, value: &str) -> Result<u32, SegmentError> {
        match self.strings.binary_search_by(|item| item.as_str().cmp(value)) {
            Ok(index) => u32::try_from(index).map_err(|_| SegmentError::Arithmetic),
            Err(_) => Err(SegmentError::Dictionary("missing string")),
        }
    }
}

fn offset_len(len: usize) -> Result<u64, SegmentError> {
    u64::try_from(len).map_err(|_| SegmentError::Arithmetic)
}

fn too_long(value: &str, limits: &DecoderLimits) -> bool {
    usize::try_from(limits.max_string_bytes).is_ok_and(|max| value.len() > max)
}

pub(crate) fn dict_str<S: AsRef<str>>(dictionary: &[S], id: u32) -> Result<&str, SegmentError> {
    let index = usize::try_from(id).map_err(|_| SegmentError::Arithmetic)?;
    dictionary
        .get(index)
        .map(AsRef::as_ref)
        .ok_or(SegmentError::Dictionary("id"))
}

pub(crate) fn encode(strings: &[String]) -> Result<(Vec<u8>, u64), SegmentError> {
    if strings.is_empty() {
        return Ok((Vec::new(), 0));
    }
    let count = u32::try_from(strings.len()).map_err(|_| SegmentError::Limit("records"))?;
    let mut out = Vec::new();
    push_u32(&mut out, count);
    let mut previous: Option<&str> = None;
    for value in strings {
        if previous.is_some_and(|prior| value.as_str() <= prior) {
            return Err(SegmentError::Dictionary("order"));
        }
        previous = Some(value.as_str());
        let len = u32::try_from(value.len()).map_err(|_| SegmentError::Limit("string"))?;
        push_u32(&mut out, len);
        out.extend_from_slice(value.as_bytes());
    }
    Ok((out, u64::from(count)))
}

pub(crate) fn decode<'a>(
    payload: &'a [u8],
    record_count: u64,
    limits: &DecoderLimits,
) -> Result<Vec<&'a str>, SegmentError> {
    if payload.is_empty() {
        if record_count != 0 {
            return Err(SegmentError::Cardinality("count"));
        }
        return Ok(Vec::new());
    }
    let mut cursor = Cursor::new(payload, 0);
    let count = u64::from(cursor.u32()?);
    if count != record_count || count == 0 {
        return Err(SegmentError::Cardinality("count"));
    }
    let room = bounded_count(count, 4, offset_len(payload.len())?, limits.max_records)?;
    let mut out = Vec::with_capacity(room);
    let mut previous: Option<&str> = None;
    for _ in 0..count {
        let len = cursor.u32()?;
        if len > limits.max_string_bytes {
            return Err(SegmentError::Limit("string"));
        }
        let bytes = cursor.take(usize::try_from(len).map_err(|_| SegmentError::Arithmetic)?)?;
        let value = std::str::from_utf8(bytes).map_err(|_| SegmentError::Utf8 { kind: 1 })?;
        if previous.is_some_and(|prior| value <= prior) {
            return Err(SegmentError::Dictionary("order"));
        }
        previous = Some(value);
        out.push(value);
    }
    if !cursor.rest().is_empty() {
        return Err(SegmentError::Dictionary("trailing"));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorted_utf8_round_trips_and_bad_bytes_are_refused() {
        let limits = DecoderLimits::standard();
        assert_eq!(
            encode(&["é".to_owned(), "a".to_owned()]).unwrap_err(),
            SegmentError::Dictionary("order")
        );
        let (payload, count) = encode(&["a".to_owned(), "é".to_owned()]).unwrap();
        assert_eq!(decode(&payload, count, &limits).unwrap(), ["a", "é"]);
        let mut bad = payload.clone();
        *bad.last_mut().unwrap() = 0xff;
        assert_eq!(
            decode(&bad, count, &limits).unwrap_err(),
            SegmentError::Utf8 { kind: 1 }
        );
        let mut unsorted = Vec::new();
        push_u32(&mut unsorted, 2);
        push_u32(&mut unsorted, 1);
        unsorted.push(b'b');
        push_u32(&mut unsorted, 1);
        unsorted.push(b'a');
        assert_eq!(
            decode(&unsorted, 2, &limits).unwrap_err(),
            SegmentError::Dictionary("order")
        );
        assert_eq!(decode(&[], 0, &limits).unwrap(), [] as [&str; 0]);
    }
}
