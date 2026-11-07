// SPDX-License-Identifier: Apache-2.0
//! Legacy writer byte parity and roundtrip tests (D3-LEGACY-REMOVAL §5 D3-02).
//! Golden little-endian fields are pinned independently of either implementation.
//! Exercises caller sinks, omitted bodies, UTF-8 paths and encoder preflight bounds.

#![forbid(unsafe_code)]

use dctx_core::datatypes::NodeType;
use dctx_core::version::Version;
use dctx_pack::{
    decode_legacy, encode_legacy, encode_legacy_to, LegacyBodies, LegacyError, LegacyLimits,
    LegacyNode,
};

// V9.2; root time 1; a time 2 (abc); d time 3 containing d/é time 4 (empty);
// z time 5 (two omitted bytes). Data offsets are 0, 3, 3; data begins at 134.
const GOLDEN: &[u8] = b"DIRCTXTV\x09\0\0\0\x02\0\0\0\
    \x01\0\0\x01\0\0\0\0\0\0\0\x03\0\0\0\
    \0\x01\0a\x02\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\x03\0\0\0\0\0\0\0\
    \x01\x01\0d\x03\0\0\0\0\0\0\0\x01\0\0\0\
    \0\x04\0d/\xc3\xa9\x04\0\0\0\0\0\0\0\x03\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\
    \0\x01\0z\x05\0\0\0\0\0\0\0\x03\0\0\0\0\0\0\0\x02\0\0\0\0\0\0\0abc\0\0";

fn node(kind: NodeType, path: &str, time: u64) -> LegacyNode {
    let mut node = LegacyNode::new(kind, path, std::path::PathBuf::new());
    node.last_modified_timestamp = time;
    node
}

fn tree() -> LegacyNode {
    let mut root = node(NodeType::Directory, "", 1);
    let mut file = node(NodeType::File, "a", 2);
    file.file_content = Some(b"abc".to_vec());
    file.content_size = 999;
    file.content_offset_in_data_section = 999;
    let mut directory = node(NodeType::Directory, "d", 3);
    let mut empty = node(NodeType::File, "d/é", 4);
    empty.file_content = Some(Vec::new());
    directory.children.push(empty);
    let mut omitted = node(NodeType::File, "z", 5);
    omitted.content_size = 2;
    root.children = vec![file, directory, omitted];
    root
}

#[test]
fn writer_matches_fixed_bytes_without_mutating_facts() {
    let tree = tree();
    let version = Version { major: 9, minor: 2 };
    let bytes = encode_legacy(&tree, &version, LegacyLimits::default()).unwrap();
    assert_eq!(bytes, GOLDEN);
    assert_eq!(tree.children[0].content_size, 999);
    assert_eq!(tree.children[0].content_offset_in_data_section, 999);
    assert!(tree.children[2].file_content.is_none());
    let mut sink = Vec::new();
    encode_legacy_to(&tree, &version, LegacyLimits::default(), &mut sink).unwrap();
    assert_eq!(sink, GOLDEN);
}

#[test]
fn loaded_roundtrip_is_identical_and_metadata_export_zero_fills() {
    let document = decode_legacy(GOLDEN, LegacyBodies::Include, LegacyLimits::default()).unwrap();
    assert_eq!(document.header.data_offset, 134);
    assert_eq!(document.root.children[1].children[0].relative_path, "d/é");
    assert_eq!(
        document.root.children[1].children[0].file_content,
        Some(Vec::new())
    );
    assert_eq!(document.root.children[2].file_content, Some(vec![0, 0]));
    assert_eq!(
        encode_legacy(
            &document.root,
            &document.header.version,
            LegacyLimits::default()
        )
        .unwrap(),
        GOLDEN
    );
    let metadata =
        decode_legacy(GOLDEN, LegacyBodies::MetadataOnly, LegacyLimits::default()).unwrap();
    let output = encode_legacy(
        &metadata.root,
        &metadata.header.version,
        LegacyLimits::default(),
    )
    .unwrap();
    assert_eq!(&output[..134], &GOLDEN[..134]);
    assert_eq!(&output[134..], &[0; 5]);
}

#[test]
fn all_truncated_nested_and_empty_file_prefixes_fail() {
    for end in 0..GOLDEN.len() {
        assert!(
            decode_legacy(
                &GOLDEN[..end],
                LegacyBodies::MetadataOnly,
                LegacyLimits::default()
            )
            .is_err(),
            "{end}"
        );
    }
    assert!(decode_legacy(GOLDEN, LegacyBodies::MetadataOnly, LegacyLimits::default()).is_ok());
}

#[test]
fn bounds_and_invalid_facts_leave_the_sink_untouched() {
    let tree = tree();
    let version = Version { major: 9, minor: 2 };
    let exact = LegacyLimits {
        max_total_bytes: GOLDEN.len(),
        max_nodes: 5,
        max_name_bytes: 4,
        max_depth: 2,
    };
    assert_eq!(encode_legacy(&tree, &version, exact).unwrap(), GOLDEN);
    for limits in [
        LegacyLimits {
            max_total_bytes: GOLDEN.len() - 1,
            ..exact
        },
        LegacyLimits {
            max_nodes: 4,
            ..exact
        },
        LegacyLimits {
            max_name_bytes: 3,
            ..exact
        },
        LegacyLimits {
            max_depth: 1,
            ..exact
        },
    ] {
        let mut sink = b"untouched".to_vec();
        assert!(matches!(
            encode_legacy_to(&tree, &version, limits, &mut sink),
            Err(LegacyError::LimitExceeded(_))
        ));
        assert_eq!(sink, b"untouched");
    }
    let mut bad = tree.clone();
    bad.children[2].content_size = u64::MAX;
    let mut sink = Vec::new();
    assert!(encode_legacy_to(&bad, &version, LegacyLimits::default(), &mut sink).is_err());
    assert_eq!(sink, []);
    bad = tree.clone();
    bad.children[0]
        .children
        .push(node(NodeType::Directory, "bad", 0));
    assert!(matches!(
        encode_legacy_to(&bad, &version, exact, &mut sink),
        Err(LegacyError::InvalidTree)
    ));
    assert_eq!(sink, []);
    let zero = Version { major: 0, minor: 0 };
    assert!(matches!(
        encode_legacy_to(&tree, &zero, exact, &mut sink),
        Err(LegacyError::UnknownVersion { .. })
    ));
    bad = tree.clone();
    bad.file_content = Some(Vec::new());
    assert!(matches!(
        encode_legacy_to(&bad, &version, exact, &mut sink),
        Err(LegacyError::InvalidTree)
    ));
    assert_eq!(sink, []);
}

#[test]
fn wire_name_limit_and_hard_depth_are_enforced() {
    let version = Version::new_v1();
    let limits = LegacyLimits {
        max_name_bytes: usize::MAX,
        max_depth: usize::MAX,
        ..LegacyLimits::default()
    };
    let mut root = node(NodeType::File, &"é".repeat(32767), u64::MAX);
    let bytes = encode_legacy(&root, &version, limits).unwrap();
    assert_eq!(
        decode_legacy(&bytes, LegacyBodies::Include, limits)
            .unwrap()
            .root
            .last_modified_timestamp,
        u64::MAX
    );
    root.relative_path.push('é');
    assert!(matches!(
        encode_legacy(&root, &version, limits),
        Err(LegacyError::LimitExceeded("name bytes"))
    ));
    root = node(NodeType::Directory, "", 0);
    for _ in 0..129 {
        let mut parent = node(NodeType::Directory, "", 0);
        parent.children.push(root);
        root = parent;
    }
    assert!(matches!(
        encode_legacy(&root, &version, limits),
        Err(LegacyError::LimitExceeded("depth"))
    ));
}

#[test]
fn absent_body_chunks_are_bounded_and_sink_errors_propagate() {
    struct Sink {
        largest: usize,
        bytes: usize,
        fail: bool,
    }
    impl std::io::Write for Sink {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if self.fail {
                return Err(std::io::Error::other("refused"));
            }
            self.largest = self.largest.max(bytes.len());
            self.bytes += bytes.len();
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            panic!("caller owns flushing")
        }
    }
    let mut file = node(NodeType::File, "a", 0);
    file.content_size = 20_000;
    let mut sink = Sink {
        largest: 0,
        bytes: 0,
        fail: false,
    };
    encode_legacy_to(
        &file,
        &Version::new_v1(),
        LegacyLimits::default(),
        &mut sink,
    )
    .unwrap();
    assert_eq!(sink.bytes, 16 + 28 + 20_000);
    assert_eq!(sink.largest, 8192);
    sink.fail = true;
    assert!(matches!(
        encode_legacy_to(
            &file,
            &Version::new_v1(),
            LegacyLimits::default(),
            &mut sink
        ),
        Err(LegacyError::Sink(_))
    ));
}

#[test]
fn varied_body_lengths_roundtrip() {
    for len in 0..257 {
        let mut root = tree();
        root.children[0].file_content = Some(
            (0..len)
                .map(|n| u8::try_from(n % 256).expect("nonnegative byte remainder"))
                .collect(),
        );
        let bytes = encode_legacy(&root, &Version::new_v1(), LegacyLimits::default()).unwrap();
        let decoded =
            decode_legacy(&bytes, LegacyBodies::Include, LegacyLimits::default()).unwrap();
        assert_eq!(
            encode_legacy(
                &decoded.root,
                &decoded.header.version,
                LegacyLimits::default()
            )
            .unwrap(),
            bytes
        );
    }
}
