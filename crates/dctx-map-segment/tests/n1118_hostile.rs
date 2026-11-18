// SPDX-License-Identifier: Apache-2.0
//! Hostile on-disk inputs through the public segment builder and decoder.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use dctx_hash::Sha256;
use dctx_map_segment::{
    DecoderLimits, DocId, Field, PostingDraft, SectionKind, SegmentBuilder, SegmentError,
    SegmentRole, TermDraft, TermHit, ValidatedSegment, decode, envelope_checksum,
};

const HEADER_LEN: usize = 80;
const ENTRY_LEN: usize = 64;
const FILE_LEN: usize = 16;
const SECTION_COUNT: usize = 24;
const PAYLOAD_OFFSET: usize = 8;
const PAYLOAD_LEN: usize = 16;
const RECORD_COUNT: usize = 24;
const PAYLOAD_HASH: usize = 32;
const BIT_FLIP_CASES: usize = 2_048;
const ROUND_TRIP_CASES: usize = 128;

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

struct CountingAllocator;

thread_local! {
    static REQUESTED_BYTES: Cell<Option<usize>> = const { Cell::new(None) };
}

fn count_request(size: usize) {
    let _ = REQUESTED_BYTES.try_with(|counter| {
        if let Some(total) = counter.get() {
            counter.set(Some(total.saturating_add(size)));
        }
    });
}

// SAFETY: Operations forward the caller's arguments unchanged to System.
// Accounting uses allocation-free thread-local storage.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count_request(layout.size());
        // SAFETY: The caller supplies a valid layout.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count_request(layout.size());
        // SAFETY: The caller supplies a valid layout.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: The caller supplies a live System allocation and its layout.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        count_request(size);
        // SAFETY: The caller supplies a live System allocation and a valid size.
        unsafe { System.realloc(ptr, layout, size) }
    }
}

fn decode_bounded(
    bytes: &[u8],
    case: impl std::fmt::Display,
) -> Result<ValidatedSegment<'_>, SegmentError> {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            REQUESTED_BYTES.set(None);
        }
    }

    REQUESTED_BYTES.set(Some(0));
    let reset = Reset;
    let result = decode(bytes, SegmentRole::Fact);
    let requested = REQUESTED_BYTES.get().unwrap();
    drop(reset);

    // Allow decoded vectors and their growth while ruling out reservations
    // proportional to oversized claims in these small inputs.
    let budget = 4_096 + bytes.len() * 64;
    assert!(
        requested <= budget,
        "{case}: decoder requested {requested} bytes for {} input bytes (budget {budget})",
        bytes.len()
    );
    result
}

fn next_random(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

fn fixture() -> Vec<u8> {
    let mut builder = SegmentBuilder::fact();
    builder.set_lengths(vec![256, 512]).unwrap();
    for term in ["aa", "bb", "é"] {
        builder
            .add_term(TermDraft {
                field: Field::Identifier,
                term: term.to_owned(),
                hits: vec![PostingDraft {
                    doc: DocId::Index(1),
                    positions: vec![0, 127, 255],
                }],
            })
            .unwrap();
    }
    builder.build().unwrap()
}

fn set_u64(bytes: &mut [u8], at: usize, value: u64) {
    bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
}

fn read_u64(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap())
}

fn resign_envelope(bytes: &mut [u8]) {
    let checksum = envelope_checksum(bytes).unwrap();
    bytes[48..HEADER_LEN].copy_from_slice(&checksum);
}

fn edit_dictionary(bytes: &mut [u8], edit: impl FnOnce(&mut [u8])) {
    let entry = HEADER_LEN;
    assert_eq!(
        &bytes[entry..entry + 2],
        &SectionKind::Dictionary.code().to_le_bytes()
    );
    let offset = usize::try_from(read_u64(bytes, entry + PAYLOAD_OFFSET)).unwrap();
    let len = usize::try_from(read_u64(bytes, entry + PAYLOAD_LEN)).unwrap();
    let payload = &mut bytes[offset..offset + len];
    edit(payload);
    let checksum = Sha256::digest(payload).unwrap();
    bytes[entry + PAYLOAD_HASH..entry + ENTRY_LEN].copy_from_slice(checksum.as_bytes());
    resign_envelope(bytes);
}

#[test]
fn every_truncation_returns_a_typed_error() {
    let bytes = fixture();
    let file_len = u64::try_from(bytes.len()).unwrap();
    for end in 0..bytes.len() {
        let at = u64::try_from(end).unwrap();
        let need = if end < HEADER_LEN {
            HEADER_LEN as u64
        } else {
            file_len
        };
        assert_eq!(
            decode_bounded(&bytes[..end], end).unwrap_err(),
            SegmentError::Truncated { at, need },
            "cut at {end}"
        );
    }
    let segment = decode_bounded(&bytes, "complete segment").unwrap();
    assert_eq!(segment.dictionary(), ["aa", "bb", "é"]);
}

#[test]
fn seeded_bit_flips_preserve_decoded_content_or_return_typed_errors() {
    let bytes = fixture();
    let original = decode_bounded(&bytes, "unmodified control").unwrap();
    let mut state = 0x3eb4_beca_1219_6d91;
    let mut bits: Vec<_> = (0..bytes.len() * 8).collect();
    assert!(bits.len() >= BIT_FLIP_CASES);
    let mut mutated = bytes.clone();
    for case in 0..BIT_FLIP_CASES {
        let remaining = u64::try_from(bits.len() - case).unwrap();
        let chosen = case + usize::try_from(next_random(&mut state) % remaining).unwrap();
        bits.swap(case, chosen);
        let bit = bits[case];
        let offset = bit / 8;
        let mask = 1 << (bit % 8);
        mutated[offset] ^= mask;

        let result = decode_bounded(&mutated, format_args!("flip bit {bit}"));
        match result {
            Ok(segment) => {
                assert_eq!(segment.role(), original.role(), "bit {bit}");
                assert_eq!(segment.minor(), original.minor(), "bit {bit}");
                assert_eq!(segment.dictionary(), original.dictionary(), "bit {bit}");
                assert_eq!(segment.prefixes(), original.prefixes(), "bit {bit}");
                assert_eq!(segment.facts(), original.facts(), "bit {bit}");
                assert_eq!(segment.postings(), original.postings(), "bit {bit}");
            }
            Err(error) => assert_eq!(
                decode_bounded(&mutated, format_args!("repeat flip bit {bit}")).unwrap_err(),
                error,
                "bit {bit}"
            ),
        }
        mutated[offset] ^= mask;
    }
}

#[test]
fn oversized_counts_and_lengths_are_rejected_before_large_allocations() {
    let original = fixture();
    let mut bytes = original.clone();
    bytes[SECTION_COUNT..SECTION_COUNT + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(
        decode_bounded(&bytes, "oversized section count").unwrap_err(),
        SegmentError::Limit("sections")
    );

    bytes.copy_from_slice(&original);
    set_u64(&mut bytes, FILE_LEN, u64::MAX);
    assert_eq!(
        decode_bounded(&bytes, "oversized file length").unwrap_err(),
        SegmentError::Limit("segment")
    );

    bytes.copy_from_slice(&original);
    set_u64(&mut bytes, HEADER_LEN + RECORD_COUNT, u64::MAX);
    assert_eq!(
        decode_bounded(&bytes, "oversized record count").unwrap_err(),
        SegmentError::Limit("records")
    );

    let count = DecoderLimits::standard().max_records;
    set_u64(&mut bytes, HEADER_LEN + RECORD_COUNT, count);
    edit_dictionary(&mut bytes, |payload| {
        payload[..4].copy_from_slice(&u32::try_from(count).unwrap().to_le_bytes());
    });
    assert_eq!(
        decode_bounded(&bytes, "in-limit count that cannot fit").unwrap_err(),
        SegmentError::Cardinality("fit")
    );

    bytes.copy_from_slice(&original);
    edit_dictionary(&mut bytes, |payload| {
        payload[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
    });
    assert_eq!(
        decode_bounded(&bytes, "oversized string length").unwrap_err(),
        SegmentError::Limit("string")
    );
}

#[test]
fn offsets_outside_the_file_or_overlapping_the_directory_are_rejected() {
    let original = fixture();
    let start = read_u64(&original, HEADER_LEN + PAYLOAD_OFFSET);
    let end = u64::try_from(original.len()).unwrap();
    for (field, value) in [
        (PAYLOAD_OFFSET, start - 1),
        (PAYLOAD_OFFSET, end),
        (PAYLOAD_OFFSET, end + 1),
        (PAYLOAD_OFFSET, u64::MAX),
        (PAYLOAD_LEN, u64::MAX),
    ] {
        let mut bytes = original.clone();
        set_u64(&mut bytes, HEADER_LEN + field, value);
        assert_eq!(
            decode_bounded(&bytes, format_args!("entry field {field} = {value}")).unwrap_err(),
            SegmentError::OffsetOverflow
        );
    }
}

#[test]
fn duplicate_and_unsorted_directory_and_dictionary_entries_are_rejected() {
    let original = fixture();
    let mut bytes = original.clone();
    let first = HEADER_LEN;
    let second = HEADER_LEN + ENTRY_LEN;
    let first_kind = [bytes[first], bytes[first + 1]];
    let second_kind = [bytes[second], bytes[second + 1]];
    bytes[second..second + 2].copy_from_slice(&first_kind);
    assert_eq!(
        decode_bounded(&bytes, "duplicate directory kind").unwrap_err(),
        SegmentError::Directory("order")
    );
    bytes[first..first + 2].copy_from_slice(&second_kind);
    assert_eq!(
        decode_bounded(&bytes, "unsorted directory kinds").unwrap_err(),
        SegmentError::Directory("order")
    );

    // After the count, each of these two-byte strings has a four-byte length.
    for replacement in [b"aa", b"AB"] {
        let mut bytes = original.clone();
        edit_dictionary(&mut bytes, |payload| {
            assert_eq!(&payload[8..10], b"aa");
            assert_eq!(&payload[14..16], b"bb");
            payload[14..16].copy_from_slice(replacement);
        });
        assert_eq!(
            decode_bounded(&bytes, format_args!("dictionary entry {replacement:?}")).unwrap_err(),
            SegmentError::Dictionary("order")
        );
    }
}

#[test]
fn invalid_utf8_is_rejected_after_both_checksums_pass() {
    let mut bytes = fixture();
    edit_dictionary(&mut bytes, |payload| {
        assert_eq!(&payload[20..22], "é".as_bytes());
        payload[21] = 0xff;
    });
    assert_eq!(
        decode_bounded(&bytes, "invalid UTF-8").unwrap_err(),
        SegmentError::Utf8 {
            kind: SectionKind::Dictionary.code()
        }
    );
}

#[test]
fn empty_input_and_encoded_empty_segment_have_distinct_results() {
    assert_eq!(
        decode_bounded(&[], "empty input").unwrap_err(),
        SegmentError::Truncated { at: 0, need: 80 }
    );
    let bytes = SegmentBuilder::fact().build().unwrap();
    let segment = decode_bounded(&bytes, "encoded empty segment").unwrap();
    assert_eq!(segment.bytes(), bytes);
    assert!(segment.dictionary().is_empty());
    assert!(segment.prefixes().is_empty());
    assert!(segment.facts().is_empty());
    assert!(segment.postings().lengths.is_empty());
    assert!(segment.postings().rows.is_empty());
}

#[test]
fn trailing_garbage_is_rejected_even_if_the_envelope_claims_it() {
    let mut bytes = fixture();
    bytes.extend_from_slice(b"\0\xffgarbage");
    assert_eq!(
        decode_bounded(&bytes, "appended bytes").unwrap_err(),
        SegmentError::TrailingBytes
    );

    let len = u64::try_from(bytes.len()).unwrap();
    set_u64(&mut bytes, FILE_LEN, len);
    resign_envelope(&mut bytes);
    assert_eq!(
        decode_bounded(&bytes, "appended bytes covered by the envelope").unwrap_err(),
        SegmentError::Directory("gap")
    );
}

#[test]
fn seeded_postings_round_trips_preserve_canonical_inputs() {
    let mut state = 0x9a3c_4457_2d67_154f;
    for case in 0..ROUND_TRIP_CASES {
        let doc_count = 1 + u32::try_from(next_random(&mut state) % 8).unwrap();
        let lengths: Vec<_> = (0..doc_count)
            .map(|_| 257 + u32::try_from(next_random(&mut state) % 512).unwrap())
            .collect();
        let term_count = 1 + usize::try_from(next_random(&mut state) % 6).unwrap();
        let field = [Field::Identifier, Field::Doc][case % 2];
        let dictionary = &["a", "alpha", "z", "é", "λ", "東京"][..term_count];
        let mut builder = SegmentBuilder::fact();
        builder.set_lengths(lengths.clone()).unwrap();
        let mut expected = Vec::new();
        for term in dictionary.iter().rev() {
            let first_doc = u32::try_from(next_random(&mut state) % u64::from(doc_count)).unwrap();
            let hits: Vec<_> = (first_doc..doc_count)
                .map(|doc| {
                    let middle = 1 + u32::try_from(next_random(&mut state) % 254).unwrap();
                    TermHit {
                        doc,
                        positions: vec![0, middle, lengths[usize::try_from(doc).unwrap()] - 1],
                    }
                })
                .collect();
            builder
                .add_term(TermDraft {
                    field,
                    term: (*term).to_owned(),
                    hits: hits
                        .iter()
                        .rev()
                        .map(|hit| PostingDraft {
                            doc: DocId::Index(hit.doc),
                            positions: hit.positions.iter().rev().copied().collect(),
                        })
                        .collect(),
                })
                .unwrap();
            expected.push(hits);
        }

        let bytes = builder.build().unwrap();
        let segment = decode_bounded(&bytes, format_args!("round trip {case}")).unwrap();
        assert_eq!(segment.role(), SegmentRole::Fact, "case {case}");
        assert_eq!(segment.dictionary(), dictionary, "case {case}");
        assert_eq!(segment.postings().lengths, lengths, "case {case}");
        assert_eq!(segment.postings().rows.len(), term_count, "case {case}");
        for (id, (term, hits)) in dictionary.iter().zip(expected.iter().rev()).enumerate() {
            let row = &segment.postings().rows[id];
            assert_eq!(row.field, field, "case {case}, term {term}");
            assert_eq!(usize::try_from(row.term_id).unwrap(), id, "case {case}");
            assert_eq!(&row.hits, hits, "case {case}, term {term}");
            assert_eq!(segment.term(field, term), Some(row));
        }
    }
}
