// SPDX-License-Identifier: Apache-2.0
//! Corruption checks for the safe segment decoder.
//! Implements SEGMENTS-v1: truncation, lengths, checksums, overflow, and unknown versions.
//! Depends on the builder and `dctx-hash`. [`super::decode`] is the subject under test.
//! Each refusal sits next to a valid segment. Arbitrary bytes return a typed error or a checked view.

use proptest::prelude::*;

use crate::{
    DecoderLimits, DocId, Field, PostingDraft, SectionKind, SegmentBuilder, SegmentError,
    SegmentRole, TermDraft, decode, decode_with_limits, envelope_checksum,
};

#[test]
fn every_truncation_is_refused_and_the_full_file_decodes() {
    for bytes in [empty_fact(), accent_segment()] {
        assert!(decode(&bytes, SegmentRole::Fact).is_ok());
        let declared = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        for cut in 0..bytes.len() {
            let err = decode(&bytes[..cut], SegmentRole::Fact).unwrap_err();
            match err {
                SegmentError::Truncated { at, need } if cut < 80 => {
                    assert_eq!(at, u64::try_from(cut).unwrap_or(u64::MAX));
                    assert_eq!(need, 80);
                }
                SegmentError::Truncated { at, need } => {
                    assert_eq!(at, u64::try_from(cut).unwrap_or(u64::MAX));
                    assert_eq!(need, declared);
                }
                other => panic!("cut {cut} yielded {other}"),
            }
        }
    }
}

#[test]
fn header_checksum_and_limit_refusals_keep_a_valid_control() {
    let bytes = empty_fact();
    assert!(decode(&bytes, SegmentRole::Fact).is_ok());
    assert_eq!(
        decode(&bytes, SegmentRole::View).unwrap_err(),
        SegmentError::Role("forbidden")
    );

    let mut magic = bytes.clone();
    magic[0] = b'X';
    assert_eq!(
        decode(&magic, SegmentRole::Fact).unwrap_err(),
        SegmentError::BadMagic
    );

    let mut version = bytes.clone();
    version[8..10].copy_from_slice(&2u16.to_le_bytes());
    resign(&mut version);
    assert_eq!(
        decode(&version, SegmentRole::Fact).unwrap_err(),
        SegmentError::UnknownVersion { major: 2, minor: 0 }
    );
    let mut minor = bytes.clone();
    minor[10..12].copy_from_slice(&7u16.to_le_bytes());
    resign(&mut minor);
    assert_eq!(decode(&minor, SegmentRole::Fact).unwrap().minor(), 7);

    let mut feature = bytes.clone();
    feature[40..48].copy_from_slice(&1u64.to_le_bytes());
    assert_eq!(
        decode(&feature, SegmentRole::Fact).unwrap_err(),
        SegmentError::UnknownFeature(1)
    );

    let mut checksum = bytes.clone();
    checksum[48] ^= 0xff;
    assert_eq!(
        decode(&checksum, SegmentRole::Fact).unwrap_err(),
        SegmentError::BadEnvelopeChecksum
    );

    let mut payload = bytes.clone();
    let (at, _, _) = entry(&payload, SectionKind::Dictionary.code());
    payload[at + 32] ^= 0xff;
    resign(&mut payload);
    assert_eq!(
        decode(&payload, SegmentRole::Fact).unwrap_err(),
        SegmentError::BadPayloadChecksum {
            kind: SectionKind::Dictionary.code()
        }
    );

    let mut trailing = bytes.clone();
    trailing.push(0);
    assert_eq!(
        decode(&trailing, SegmentRole::Fact).unwrap_err(),
        SegmentError::TrailingBytes
    );

    let mut offset = bytes.clone();
    let (at, _, _) = entry(&offset, SectionKind::Dictionary.code());
    offset[at + 8..at + 16].copy_from_slice(&u64::MAX.to_le_bytes());
    assert_eq!(
        decode(&offset, SegmentRole::Fact).unwrap_err(),
        SegmentError::OffsetOverflow
    );

    let mut length = bytes.clone();
    let (at, _, _) = entry(&length, SectionKind::Facts.code());
    length[at + 16..at + 24].copy_from_slice(&u64::MAX.to_le_bytes());
    assert_eq!(
        decode(&length, SegmentRole::Fact).unwrap_err(),
        SegmentError::OffsetOverflow
    );

    let mut flags = bytes.clone();
    let (at, _, _) = entry(&flags, SectionKind::Dictionary.code());
    flags[at + 4..at + 8].copy_from_slice(&3u32.to_le_bytes());
    assert_eq!(
        decode(&flags, SegmentRole::Fact).unwrap_err(),
        SegmentError::Directory("flag")
    );

    let mut schema = bytes.clone();
    let (at, _, _) = entry(&schema, SectionKind::Dictionary.code());
    schema[at + 2..at + 4].copy_from_slice(&2u16.to_le_bytes());
    resign(&mut schema);
    assert_eq!(
        decode(&schema, SegmentRole::Fact).unwrap_err(),
        SegmentError::UnknownSchema {
            kind: SectionKind::Dictionary.code(),
            schema: 2
        }
    );

    let mut required = bytes.clone();
    let (at, _, _) = entry(&required, SectionKind::Dictionary.code());
    required[at + 4..at + 8].copy_from_slice(&0u32.to_le_bytes());
    resign(&mut required);
    assert_eq!(
        decode(&required, SegmentRole::Fact).unwrap_err(),
        SegmentError::Role("flag")
    );

    let mut count = bytes.clone();
    let (at, _, _) = entry(&count, SectionKind::Dictionary.code());
    count[at + 24..at + 32].copy_from_slice(&1u64.to_le_bytes());
    assert_eq!(
        decode(&count, SegmentRole::Fact).unwrap_err(),
        SegmentError::Cardinality("count")
    );

    let mut huge = bytes.clone();
    huge[16..24].copy_from_slice(&(super::limits::MAX_SEGMENT_BYTES + 1).to_le_bytes());
    let raised = DecoderLimits {
        max_segment_bytes: u64::MAX,
        ..DecoderLimits::standard()
    };
    assert_eq!(
        decode_with_limits(&huge, SegmentRole::Fact, raised).unwrap_err(),
        SegmentError::Limit("segment")
    );
    assert!(decode_with_limits(&bytes, SegmentRole::Fact, raised).is_ok());

    let tight = DecoderLimits {
        max_segment_bytes: 80,
        ..DecoderLimits::standard()
    };
    assert_eq!(
        decode_with_limits(&bytes, SegmentRole::Fact, tight).unwrap_err(),
        SegmentError::Limit("segment")
    );
}

#[test]
fn utf8_gaps_and_string_limits_are_refused() {
    let bytes = accent_segment();
    let segment = decode(&bytes, SegmentRole::Fact).unwrap();
    assert_eq!(segment.dictionary(), ["é"]);
    assert_eq!(
        segment.section_payload(SectionKind::Postings).unwrap(),
        [
            0x01, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x0c, 0x00, 0x00, 0x00, 0x01, 0x00,
            0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x01, 0x01,
        ]
    );

    let mut bad_utf8 = bytes.clone();
    let (at, offset, len) = entry(&bad_utf8, SectionKind::Dictionary.code());
    let len = usize::try_from(len).unwrap();
    let end = offset + len;
    bad_utf8[end - 1] = 0xff;
    let hash = super::checksum::sha256(&bad_utf8[offset..end]).unwrap();
    bad_utf8[at + 32..at + 64].copy_from_slice(&hash);
    resign(&mut bad_utf8);
    assert_eq!(
        decode(&bad_utf8, SegmentRole::Fact).unwrap_err(),
        SegmentError::Utf8 {
            kind: SectionKind::Dictionary.code()
        }
    );

    let mut gap = bytes.clone();
    let (at, _, len) = entry(&gap, SectionKind::Dictionary.code());
    assert!(len > 1);
    gap[at + 16..at + 24].copy_from_slice(&(len - 1).to_le_bytes());
    assert_eq!(
        decode(&gap, SegmentRole::Fact).unwrap_err(),
        SegmentError::Directory("gap")
    );

    let records = DecoderLimits {
        max_records: 0,
        ..DecoderLimits::standard()
    };
    assert_eq!(
        decode_with_limits(&bytes, SegmentRole::Fact, records).unwrap_err(),
        SegmentError::Limit("records")
    );
    let strings = DecoderLimits {
        max_string_bytes: 1,
        ..DecoderLimits::standard()
    };
    assert_eq!(
        decode_with_limits(&bytes, SegmentRole::Fact, strings).unwrap_err(),
        SegmentError::Limit("string")
    );
    assert!(decode_with_limits(&bytes, SegmentRole::Fact, DecoderLimits::standard()).is_ok());
}

#[test]
fn every_single_byte_flip_is_refused() {
    let bytes = empty_fact();
    assert!(decode(&bytes, SegmentRole::Fact).is_ok());
    for (index, byte) in bytes.iter().enumerate() {
        let mut flipped = bytes.clone();
        flipped[index] = byte ^ 0x5a;
        assert!(
            decode(&flipped, SegmentRole::Fact).is_err(),
            "index {index}"
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn arbitrary_bytes_do_not_panic(bytes in proptest::collection::vec(any::<u8>(), 0..2048)) {
        prop_assert!(checked(&bytes, SegmentRole::Fact));
        prop_assert!(checked(&bytes, SegmentRole::View));
    }
}

fn checked(bytes: &[u8], role: SegmentRole) -> bool {
    match decode(bytes, role) {
        Ok(segment) => {
            if segment.bytes() != bytes || segment.role() != role {
                return false;
            }
            let dictionary = segment.dictionary();
            let _ = dictionary.iter().map(|text| text.len()).sum::<usize>();
            for entry in segment.prefixes() {
                let start = usize::try_from(entry.first).unwrap_or(usize::MAX);
                let last = usize::try_from(entry.last).unwrap_or(usize::MAX);
                if start > last || last >= dictionary.len() {
                    return false;
                }
                if !dictionary[start].starts_with(entry.prefix)
                    || !dictionary[last].starts_with(entry.prefix)
                {
                    return false;
                }
            }
            let _ = segment.facts().len() + segment.nodes().len() + segment.forward().len();
            let _ = segment.postings().lengths.len();
            let _ = segment.string_range("a").len();
            let _ = segment.degree(0);
            let _ = segment.forward_from(0);
            let _ = segment.section_payload(SectionKind::Dictionary);
            true
        }
        Err(error) => !error.to_string().is_empty(),
    }
}

fn empty_fact() -> Vec<u8> {
    SegmentBuilder::fact().build().unwrap()
}

fn accent_segment() -> Vec<u8> {
    let mut builder = SegmentBuilder::fact();
    builder.set_lengths(vec![2]).unwrap();
    builder
        .add_term(TermDraft {
            field: Field::Identifier,
            term: "é".to_owned(),
            hits: vec![PostingDraft {
                doc: DocId::Index(0),
                positions: vec![1],
            }],
        })
        .unwrap();
    builder.build().unwrap()
}

fn resign(bytes: &mut [u8]) {
    let sum = envelope_checksum(bytes).unwrap();
    bytes[48..80].copy_from_slice(&sum);
}

fn entry(bytes: &[u8], kind: u16) -> (usize, usize, u64) {
    let count = u32::from_le_bytes(bytes[24..28].try_into().unwrap());
    for index in 0..count {
        let at = 80 + usize::try_from(index).unwrap() * 64;
        let found = u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap());
        if found == kind {
            let offset = u64::from_le_bytes(bytes[at + 8..at + 16].try_into().unwrap());
            let len = u64::from_le_bytes(bytes[at + 16..at + 24].try_into().unwrap());
            return (at, usize::try_from(offset).unwrap(), len);
        }
    }
    panic!("missing section {kind}");
}
