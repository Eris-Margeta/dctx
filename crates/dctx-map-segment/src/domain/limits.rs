// SPDX-License-Identifier: Apache-2.0
//! Decoder ceilings from SEGMENTS-v1. A caller may tighten them, never raise them.
//! Implements the default caps: 64 MiB, 64 sections, 1 MiB strings and records, `1_000_000` records.
//! Depends on [`super::error::SegmentError`]. Every section parser calls [`bounded_count`] before it reserves.
//! The segment builder uses the same effective limits so it cannot emit a file the decoder must refuse.

use super::error::SegmentError;

/// Default maximum segment length, in bytes.
pub const MAX_SEGMENT_BYTES: u64 = 64 * 1024 * 1024;
/// Default maximum directory entries.
pub const MAX_SECTIONS: u32 = 64;
/// Default maximum UTF-8 string, in bytes.
pub const MAX_STRING_BYTES: u32 = 1024 * 1024;
/// Default maximum records in one section.
pub const MAX_RECORDS: u64 = 1_000_000;
/// Default maximum length-delimited record, in bytes.
pub const MAX_RECORD_BYTES: u32 = 1024 * 1024;

/// Ceilings applied before any allocation from a claimed count.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecoderLimits {
    /// Maximum accepted file length.
    pub max_segment_bytes: u64,
    /// Maximum section count.
    pub max_sections: u32,
    /// Maximum dictionary string and prefix, in bytes.
    pub max_string_bytes: u32,
    /// Maximum records in one section, and the maximum dictionary size.
    pub max_records: u64,
    /// Maximum `record_length` payload, in bytes.
    pub max_record_bytes: u32,
}

impl DecoderLimits {
    /// The SEGMENTS-v1 default ceilings.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub const fn standard() -> Self {
        Self {
            max_segment_bytes: MAX_SEGMENT_BYTES,
            max_sections: MAX_SECTIONS,
            max_string_bytes: MAX_STRING_BYTES,
            max_records: MAX_RECORDS,
            max_record_bytes: MAX_RECORD_BYTES,
        }
    }

    /// Returns these limits capped by [`Self::standard`]. The stricter value wins.
    #[allow(clippy::must_use_candidate, clippy::return_self_not_must_use, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub const fn effective(self) -> Self {
        Self {
            max_segment_bytes: min_u64(self.max_segment_bytes, MAX_SEGMENT_BYTES),
            max_sections: min_u32(self.max_sections, MAX_SECTIONS),
            max_string_bytes: min_u32(self.max_string_bytes, MAX_STRING_BYTES),
            max_records: min_u64(self.max_records, MAX_RECORDS),
            max_record_bytes: min_u32(self.max_record_bytes, MAX_RECORD_BYTES),
        }
    }
}

impl Default for DecoderLimits {
    fn default() -> Self {
        Self::standard()
    }
}

const fn min_u64(left: u64, right: u64) -> u64 {
    if left < right { left } else { right }
}

const fn min_u32(left: u32, right: u32) -> u32 {
    if left < right { left } else { right }
}

/// Accepts `count` only when it is within the record cap and its minimum footprint fits.
pub(crate) fn bounded_count(
    count: u64,
    min_each: u64,
    available: u64,
    max_records: u64,
) -> Result<usize, SegmentError> {
    if count > max_records {
        return Err(SegmentError::Limit("records"));
    }
    let need = count
        .checked_mul(min_each)
        .ok_or(SegmentError::Arithmetic)?;
    if need > available {
        return Err(SegmentError::Cardinality("fit"));
    }
    usize::try_from(count).map_err(|_| SegmentError::Arithmetic)
}

pub(crate) fn offset_of(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effective_limits_keep_the_stricter_cap() {
        let raised = DecoderLimits {
            max_segment_bytes: u64::MAX,
            max_sections: u32::MAX,
            max_string_bytes: u32::MAX,
            max_records: u64::MAX,
            max_record_bytes: u32::MAX,
        };
        assert_eq!(raised.effective(), DecoderLimits::standard());
        let tight = DecoderLimits {
            max_records: 3,
            ..DecoderLimits::standard()
        };
        assert_eq!(tight.effective().max_records, 3);
        assert_eq!(bounded_count(2, 4, 8, 3).unwrap(), 2);
        assert_eq!(
            bounded_count(4, 1, 4, 3).unwrap_err(),
            SegmentError::Limit("records")
        );
        assert_eq!(
            bounded_count(2, 4, 7, 3).unwrap_err(),
            SegmentError::Cardinality("fit")
        );
    }
}
