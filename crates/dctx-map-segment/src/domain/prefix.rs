// SPDX-License-Identifier: Apache-2.0
//! Sparse prefix ranges over a sorted dictionary. Exact lookup stays a sorted range.
//! Implements SEGMENTS-v1 kind 2. This is not an FST; a later measured FST would be a new schema.
//! Depends on [`super::dictionary`] for the strings it points at. The builder emits one range per first character.
//! A stored range must be the exact dictionary slice that starts with that prefix.

use super::cursor::{push_u32, Cursor};
use super::error::SegmentError;
use super::limits::{bounded_count, DecoderLimits};

/// One inclusive dictionary range whose strings all start with `prefix`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrefixEntry<'a> {
    /// UTF-8 prefix borrowed from the section.
    pub prefix: &'a str,
    /// First dictionary index in the range.
    pub first: u32,
    /// Last dictionary index in the range.
    pub last: u32,
}

pub(crate) struct PrefixSpec {
    pub prefix: String,
    pub first: u32,
    pub last: u32,
}

pub(crate) fn sparse_first_char(strings: &[String]) -> Vec<PrefixSpec> {
    let mut out = Vec::new();
    let mut index = 0usize;
    while index < strings.len() {
        let Some(ch) = strings[index].chars().next() else {
            index += 1;
            continue;
        };
        let mut prefix = String::new();
        prefix.push(ch);
        let start = u32::try_from(index).unwrap_or(u32::MAX);
        index += 1;
        while index < strings.len() && strings[index].starts_with(&prefix) {
            index += 1;
        }
        out.push(PrefixSpec {
            prefix,
            first: start,
            last: u32::try_from(index - 1).unwrap_or(u32::MAX),
        });
    }
    out
}

pub(crate) fn check_range<S: AsRef<str>>(
    dictionary: &[S],
    prefix: &str,
    first: u32,
    last: u32,
) -> Result<(), SegmentError> {
    let start = usize::try_from(first).map_err(|_| SegmentError::Arithmetic)?;
    let end = usize::try_from(last).map_err(|_| SegmentError::Arithmetic)?;
    if start > end {
        return Err(SegmentError::Prefix("range"));
    }
    let Some(first_text) = dictionary.get(start).map(AsRef::as_ref) else {
        return Err(SegmentError::Prefix("range"));
    };
    let Some(last_text) = dictionary.get(end).map(AsRef::as_ref) else {
        return Err(SegmentError::Prefix("range"));
    };
    if !first_text.starts_with(prefix) || !last_text.starts_with(prefix) {
        return Err(SegmentError::Prefix("range"));
    }
    if start > 0 && dictionary.get(start - 1).is_some_and(|text| text.as_ref().starts_with(prefix)) {
        return Err(SegmentError::Prefix("not exact"));
    }
    if dictionary.get(end + 1).is_some_and(|text| text.as_ref().starts_with(prefix)) {
        return Err(SegmentError::Prefix("not exact"));
    }
    Ok(())
}

pub(crate) fn encode(specs: &[PrefixSpec]) -> Result<(Vec<u8>, u64), SegmentError> {
    if specs.is_empty() {
        return Ok((Vec::new(), 0));
    }
    let count = u32::try_from(specs.len()).map_err(|_| SegmentError::Limit("records"))?;
    let mut out = Vec::new();
    push_u32(&mut out, count);
    let mut previous: Option<&str> = None;
    for spec in specs {
        if previous.is_some_and(|prior| spec.prefix.as_str() <= prior) {
            return Err(SegmentError::Prefix("order"));
        }
        previous = Some(spec.prefix.as_str());
        let len = u32::try_from(spec.prefix.len()).map_err(|_| SegmentError::Limit("string"))?;
        push_u32(&mut out, len);
        out.extend_from_slice(spec.prefix.as_bytes());
        push_u32(&mut out, spec.first);
        push_u32(&mut out, spec.last);
    }
    Ok((out, u64::from(count)))
}

pub(crate) fn decode<'a, S: AsRef<str>>(
    payload: &'a [u8],
    record_count: u64,
    dictionary: &[S],
    limits: &DecoderLimits,
) -> Result<Vec<PrefixEntry<'a>>, SegmentError> {
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
    let room = bounded_count(count, 12, u64::try_from(payload.len()).unwrap_or(u64::MAX), limits.max_records)?;
    let mut out = Vec::with_capacity(room);
    let mut previous: Option<&str> = None;
    for _ in 0..count {
        let len = cursor.u32()?;
        if len > limits.max_string_bytes {
            return Err(SegmentError::Limit("string"));
        }
        let bytes = cursor.take(usize::try_from(len).map_err(|_| SegmentError::Arithmetic)?)?;
        let prefix = std::str::from_utf8(bytes).map_err(|_| SegmentError::Utf8 { kind: 2 })?;
        if previous.is_some_and(|prior| prefix <= prior) {
            return Err(SegmentError::Prefix("order"));
        }
        previous = Some(prefix);
        let first = cursor.u32()?;
        let last = cursor.u32()?;
        check_range(dictionary, prefix, first, last)?;
        out.push(PrefixEntry { prefix, first, last });
    }
    if !cursor.rest().is_empty() {
        return Err(SegmentError::Prefix("trailing"));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_character_ranges_are_exact() {
        let strings = vec!["a".to_owned(), "ab".to_owned(), "ba".to_owned()];
        let specs = sparse_first_char(&strings);
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].prefix, "a");
        assert_eq!((specs[0].first, specs[0].last), (0, 1));
        assert_eq!(specs[1].prefix, "b");
        check_range(&strings, "a", 0, 1).unwrap();
        assert_eq!(
            check_range(&strings, "a", 0, 0).unwrap_err(),
            SegmentError::Prefix("not exact")
        );
        let limits = DecoderLimits::standard();
        let (payload, count) = encode(&specs).unwrap();
        let decoded = decode(&payload, count, &strings, &limits).unwrap();
        assert_eq!(decoded[0].prefix, "a");
        assert_eq!(decoded[1].last, 2);
    }
}
