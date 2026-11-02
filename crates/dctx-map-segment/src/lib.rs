//! Pure safe codec for immutable map index segments.
//! Implements ARCHITECTURE-PROPOSAL §3 Segments and §7 DX-C1, plus SEGMENTS-v1.
//! Checksums use `dctx-hash`. Stores and tests borrow validated slices from the bytes they pass in.
//! Corruption is a [`SegmentError`]. Version 1 is a sorted dictionary, not an FST, and does not map memory.

#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]

pub mod adapters;
pub mod domain;
pub mod ports;

pub use domain::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_fact_segment_round_trips_and_view_role_is_refused() {
        let builder = SegmentBuilder::fact();
        let bytes = builder.build().unwrap();
        assert_eq!(bytes, builder.build().unwrap());
        let segment = decode(&bytes, SegmentRole::Fact).unwrap();
        assert_eq!(segment.role(), SegmentRole::Fact);
        assert_eq!(segment.dictionary(), [] as [&str; 0]);
        assert_eq!(segment.facts(), []);
        assert_eq!(segment.prefixes(), []);
        assert_eq!(
            decode(&bytes, SegmentRole::View).unwrap_err(),
            SegmentError::Role("forbidden")
        );
    }
}
