//! Adversarial DIRCTXTV reader checks for D3-LEGACY-REMOVAL §5 D3-02.
//! Uses fixed wire fields and the D3-01 PNG bytes, never the legacy IO implementation.
//! Exercises bounds independently of the encoder, including every truncated prefix.

#![forbid(unsafe_code)]

use dctx_pack::legacy_codec::{revision_can_advance, LEGACY_MAX_DEPTH};
use dctx_pack::{LegacyBodies, LegacyError, LegacyLimits, decode_legacy};

// V3.7; directory "" at time 1; file logo.png at time 2, offset 0, size 7.
const GOLDEN: &[u8] = b"DIRCTXTV\x03\0\0\0\x07\0\0\0\
    \x01\0\0\x01\0\0\0\0\0\0\0\x01\0\0\0\
    \0\x08\0logo.png\x02\0\0\0\0\0\0\0\
    \0\0\0\0\0\0\0\0\x07\0\0\0\0\0\0\0\x89PNG\0\0\x01";

fn decode(bytes: &[u8]) -> Result<dctx_pack::LegacyDocument, LegacyError> {
    decode_legacy(bytes, LegacyBodies::Include, LegacyLimits::default())
}

#[test]
fn fixed_header_metadata_and_pinned_body() {
    let png: Vec<u8> = include_str!("fixtures/legacy-binary.hex")
        .lines()
        .flat_map(|line| line.split('#').next().unwrap().split_whitespace())
        .map(|byte| u8::from_str_radix(byte, 16).unwrap())
        .collect();
    let loaded = decode(GOLDEN).unwrap();
    assert_eq!(loaded.header.data_offset, 66);
    assert_eq!((loaded.header.version.major, loaded.header.version.minor), (3, 7));
    assert_eq!(loaded.root.last_modified_timestamp, 1);
    let file = &loaded.root.children[0];
    assert_eq!(file.relative_path, "logo.png");
    assert_eq!(file.last_modified_timestamp, 2);
    assert_eq!(file.content_offset_in_data_section, 0);
    assert_eq!(file.content_size, 7);
    assert_eq!(file.file_content.as_deref(), Some(png.as_slice()));
    assert!(file.disk_path.as_os_str().is_empty());
    let metadata = decode_legacy(GOLDEN, LegacyBodies::MetadataOnly, LegacyLimits::default()).unwrap();
    assert!(metadata.root.children[0].file_content.is_none());
    assert_eq!(metadata.root.children[0].content_size, 7);
}

#[test]
fn every_truncated_prefix_is_refused_in_both_modes() {
    for mode in [LegacyBodies::MetadataOnly, LegacyBodies::Include] {
        assert!(decode_legacy(GOLDEN, mode, LegacyLimits::default()).is_ok());
        for end in 0..GOLDEN.len() {
            assert!(decode_legacy(&GOLDEN[..end], mode, LegacyLimits::default()).is_err(), "prefix {end}");
        }
    }
    assert!(matches!(decode(&GOLDEN[..7]), Err(LegacyError::Truncated)));
}

#[test]
fn typed_magic_version_kind_utf8_and_trailing_errors() {
    let mut bytes = GOLDEN.to_vec();
    bytes[0] = 0;
    assert!(matches!(decode(&bytes), Err(LegacyError::BadMagic)));
    bytes = GOLDEN.to_vec();
    bytes[8..12].fill(0);
    assert!(matches!(decode(&bytes), Err(LegacyError::UnknownVersion { major: 0, minor: 7 })));
    bytes[8..16].fill(255);
    assert_eq!(decode(&bytes).unwrap().header.version.major, u32::MAX);
    bytes = GOLDEN.to_vec();
    bytes[16] = 2;
    assert!(matches!(decode(&bytes), Err(LegacyError::UnknownNodeType(2))));
    bytes = GOLDEN.to_vec();
    bytes[34] = 255;
    assert!(matches!(decode(&bytes), Err(LegacyError::InvalidUtf8)));
    bytes = GOLDEN.to_vec();
    bytes.push(0);
    assert!(matches!(decode(&bytes), Err(LegacyError::TrailingBytes)));
    assert!(decode(GOLDEN).is_ok());
}

#[test]
fn malicious_lengths_counts_offsets_and_overflow() {
    let mut bytes = GOLDEN.to_vec();
    bytes[32..34].copy_from_slice(&u16::MAX.to_le_bytes());
    assert!(matches!(decode(&bytes), Err(LegacyError::LimitExceeded("name bytes"))));
    bytes = GOLDEN.to_vec();
    bytes[27..31].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(matches!(decode(&bytes), Err(LegacyError::LimitExceeded("nodes"))));
    bytes = GOLDEN.to_vec();
    bytes[50..58].copy_from_slice(&8u64.to_le_bytes());
    assert!(matches!(decode(&bytes), Err(LegacyError::InvalidOffset)));
    bytes[50..58].copy_from_slice(&u64::MAX.to_le_bytes());
    assert!(matches!(decode(&bytes), Err(LegacyError::Overflow)));
    bytes = GOLDEN.to_vec();
    bytes[58..66].copy_from_slice(&u64::MAX.to_le_bytes());
    assert!(matches!(decode(&bytes), Err(LegacyError::Truncated | LegacyError::Overflow)));
    assert!(decode(GOLDEN).is_ok());
}

#[test]
fn exact_limits_pass_and_one_less_fails() {
    let exact = LegacyLimits { max_total_bytes: GOLDEN.len(), max_nodes: 2, max_name_bytes: 8, max_depth: 1 };
    assert!(decode_legacy(GOLDEN, LegacyBodies::Include, exact).is_ok());
    for limits in [
        LegacyLimits { max_total_bytes: GOLDEN.len() - 1, ..exact },
        LegacyLimits { max_nodes: 1, ..exact },
        LegacyLimits { max_name_bytes: 7, ..exact },
        LegacyLimits { max_depth: 0, ..exact },
    ] {
        assert!(matches!(decode_legacy(GOLDEN, LegacyBodies::Include, limits), Err(LegacyError::LimitExceeded(_))));
    }
}

fn directory_chain(depth: usize) -> Vec<u8> {
    let mut bytes = b"DIRCTXTV\x01\0\0\0\0\0\0\0".to_vec();
    for level in 0..=depth {
        bytes.extend_from_slice(&[1, 0, 0]);
        bytes.extend_from_slice(&0u64.to_le_bytes());
        bytes.extend_from_slice(&u32::from(level < depth).to_le_bytes());
    }
    bytes
}

#[test]
fn depth_bomb_is_refused_even_if_caller_removes_its_cap() {
    assert!(decode(&directory_chain(LEGACY_MAX_DEPTH)).is_ok());
    let bomb = directory_chain(10_000);
    let limits = LegacyLimits { max_depth: usize::MAX, ..LegacyLimits::default() };
    assert!(matches!(decode_legacy(&bomb, LegacyBodies::Include, limits), Err(LegacyError::LimitExceeded("depth"))));
}

#[test]
fn overlapping_ranges_cannot_amplify_copies_past_the_bound() {
    let mut bytes = b"DIRCTXTV\x01\0\0\0\0\0\0\0\x01\0\0\0\0\0\0\0\0\0\0\x04\0\0\0".to_vec();
    for _ in 0..4 {
        bytes.extend_from_slice(&[0, 0, 0]);
        bytes.extend_from_slice(&0u64.to_le_bytes());
        bytes.extend_from_slice(&0u64.to_le_bytes());
        bytes.extend_from_slice(&100u64.to_le_bytes());
    }
    bytes.extend_from_slice(&[42; 100]);
    let limits = LegacyLimits { max_total_bytes: bytes.len(), ..LegacyLimits::default() };
    assert!(matches!(
        decode_legacy(&bytes, LegacyBodies::Include, limits),
        Err(LegacyError::LimitExceeded("body bytes"))
    ));
    assert!(decode(&bytes).is_ok());
}

#[test]
fn arbitrary_bytes_never_panic() {
    let mut seed = 17u32;
    for len in 0..512 {
        let mut bytes = Vec::new();
        for _ in 0..len {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            bytes.push((seed >> 24) as u8);
        }
        if len >= 16 {
            bytes[..16].copy_from_slice(&GOLDEN[..16]);
        }
        let _ = decode(&bytes);
    }
}

#[test]
fn maximum_major_decodes_and_the_next_revision_is_a_typed_refusal() {
    let mut bytes = GOLDEN.to_vec();
    bytes[8..16].fill(255);
    let document = decode(&bytes).expect("historical maximum still decodes");
    assert_eq!(document.header.version.major, u32::MAX);
    assert!(matches!(revision_can_advance(&document.header.version), Err(LegacyError::Overflow)));
    let mut prior = document.header.version;
    prior.major = u32::MAX - 1;
    assert!(revision_can_advance(&prior).is_ok());
    assert!(revision_can_advance(&decode(GOLDEN).expect("golden").header.version).is_ok());
}
