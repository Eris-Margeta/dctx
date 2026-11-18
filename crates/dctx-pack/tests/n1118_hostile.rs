// SPDX-License-Identifier: Apache-2.0

use dctx_core::budget::PackLimits;
use dctx_core::datatypes::NodeType;
use dctx_core::error::DctxError;
use dctx_core::facts::{Diagnostic, EntryMeta, ReadGrant, SourceEntry};
use dctx_core::ids::{RelativePath, Timestamp, ViewKey};
use dctx_core::selection::{RuleTexts, SelectionPolicy};
use dctx_pack::{
    decode_legacy, pack, LegacyBodies, LegacyError, LegacyLimits, PackPorts, PackRequest,
};
use dctx_ports::memory::{CancelFlag, FixedClock, MemoryCache, MemoryFactStore};
use dctx_ports::Source;

const MODES: [LegacyBodies; 2] = [LegacyBodies::MetadataOnly, LegacyBodies::Include];

fn pack_bytes(declared_count: u32, members: &[(&str, &[u8], u64)]) -> Vec<u8> {
    let mut bytes = b"DIRCTXTV\x01\0\0\0\0\0\0\0".to_vec();
    bytes.push(1); // Directory with an empty root name.
    bytes.extend([0; 10]); // Root name length and timestamp.
    bytes.extend(declared_count.to_le_bytes());

    let mut data = Vec::new();
    for &(name, body, declared_size) in members {
        bytes.push(0); // File.
        bytes.extend(u16::try_from(name.len()).unwrap().to_le_bytes());
        bytes.extend(name.as_bytes());
        bytes.extend([0; 8]); // Timestamp.
        bytes.extend(u64::try_from(data.len()).unwrap().to_le_bytes());
        bytes.extend(declared_size.to_le_bytes());
        data.extend_from_slice(body);
    }
    bytes.extend(data);
    bytes
}

#[test]
fn zero_byte_input_is_truncated_and_zero_length_member_is_valid() {
    let bytes = pack_bytes(1, &[("empty", b"", 0)]);
    for mode in MODES {
        assert!(matches!(
            decode_legacy(b"", mode, LegacyLimits::default()),
            Err(LegacyError::Truncated)
        ));
        let decoded = decode_legacy(&bytes, mode, LegacyLimits::default()).unwrap();
        let [member] = decoded.root.children.as_slice() else {
            panic!("expected one empty member");
        };
        assert_eq!(decoded.header.data_offset, bytes.len());
        assert_eq!(member.content_size, 0);
        assert_eq!(
            member.file_content.as_deref(),
            (mode == LegacyBodies::Include).then_some(b"".as_slice())
        );
    }
}

#[test]
fn nonempty_truncated_packs_are_rejected_across_metadata_and_body() {
    let bytes = pack_bytes(2, &[("a", b"x\0y", 3), ("b", b"z", 1)]);
    for mode in MODES {
        let complete = decode_legacy(&bytes, mode, LegacyLimits::default()).unwrap();
        let data_start = complete.header.data_offset;
        for end in 1..bytes.len() {
            let error = decode_legacy(&bytes[..end], mode, LegacyLimits::default()).unwrap_err();
            // With complete metadata and fewer than three data bytes, the second
            // file's start offset itself falls outside the available data section.
            let expected = if (data_start..data_start + 3).contains(&end) {
                matches!(error, LegacyError::InvalidOffset)
            } else {
                matches!(error, LegacyError::Truncated)
            };
            assert!(expected, "{mode:?}, prefix length {end}: {error:?}");
        }
    }
}

#[test]
fn exact_pack_byte_limit_is_inclusive() {
    let bytes = pack_bytes(1, &[("a", b"x", 1)]);
    let limits = LegacyLimits {
        max_total_bytes: bytes.len(),
        ..LegacyLimits::default()
    };
    let mut oversized = bytes.clone();
    oversized.push(0);
    for mode in MODES {
        assert!(decode_legacy(&bytes, mode, limits).is_ok());
        assert!(matches!(
            decode_legacy(&oversized, mode, limits),
            Err(LegacyError::LimitExceeded("total bytes"))
        ));
    }
}

#[test]
fn declared_member_count_must_match_the_metadata() {
    let limits = LegacyLimits::default();
    // This count alone fits the limit, but the root is also a node.
    let over_limit = u32::try_from(limits.max_nodes).unwrap();
    for mode in MODES {
        let decode = |count| decode_legacy(&pack_bytes(count, &[("a", b"x", 1)]), mode, limits);
        assert!(decode(1).is_ok());
        assert!(matches!(
            decode(0),
            Err(LegacyError::CorruptData { offset: 0 })
        ));
        assert!(matches!(decode(2), Err(LegacyError::Truncated)));
        assert!(matches!(
            decode(over_limit),
            Err(LegacyError::LimitExceeded("nodes"))
        ));
    }
}

#[test]
fn declared_member_size_must_match_the_data() {
    for mode in MODES {
        let decode = |size| {
            decode_legacy(
                &pack_bytes(2, &[("a", b"x", 1), ("b", b"yz", size)]),
                mode,
                LegacyLimits::default(),
            )
        };
        assert!(decode(2).is_ok());
        assert!(matches!(
            decode(1),
            Err(LegacyError::CorruptData { offset: 2 })
        ));
        assert!(matches!(decode(3), Err(LegacyError::Truncated)));
        assert!(matches!(decode(u64::MAX), Err(LegacyError::Overflow)));
    }
}

#[test]
fn duplicate_wire_names_keep_distinct_member_bodies_in_order() {
    let bytes = pack_bytes(2, &[("same", b"x", 1), ("same", b"yz", 2)]);
    for mode in MODES {
        let decoded = decode_legacy(&bytes, mode, LegacyLimits::default()).unwrap();
        let [first, second] = decoded.root.children.as_slice() else {
            panic!("expected both wire records");
        };
        assert_eq!(
            (first.relative_path.as_str(), second.relative_path.as_str()),
            ("same", "same")
        );
        assert_eq!(first.content_offset_in_data_section, 0);
        assert_eq!(second.content_offset_in_data_section, 1);
        for (node, body) in [(first, b"x".as_slice()), (second, b"yz".as_slice())] {
            assert_eq!(
                node.file_content.as_deref(),
                (mode == LegacyBodies::Include).then_some(body)
            );
        }
    }
}

struct UntrustedListing<'a>(&'a [&'a str]);

impl Source for UntrustedListing<'_> {
    fn root_kind(&self) -> Result<NodeType, DctxError> {
        Ok(NodeType::Directory)
    }

    fn entries(&self, dir: &RelativePath) -> Result<Vec<SourceEntry>, DctxError> {
        assert!(dir.is_root());
        Ok(self.0.iter().copied().map(SourceEntry::file).collect())
    }

    fn metadata(&self, path: &RelativePath) -> Result<EntryMeta, DctxError> {
        assert!(path.is_root() || path.as_str() == "ok");
        Ok(EntryMeta {
            size: u64::from(!path.is_root()),
            modified: Timestamp(0),
        })
    }

    fn read(&self, grant: &ReadGrant, max_bytes: u64) -> Result<Vec<u8>, DctxError> {
        assert_eq!(grant.path().as_str(), "ok");
        assert!(max_bytes >= 1);
        Ok(vec![b'x'])
    }
}

#[test]
fn pack_skips_unsafe_member_names_and_deduplicates_valid_names() {
    let policy = SelectionPolicy::from_texts(&RuleTexts::default());
    for name in ["../escape", "/absolute", "nul\0byte", "sub\\file"] {
        let result = pack(
            PackPorts {
                source: &UntrustedListing(&["ok", name, "ok"]),
                store: &mut MemoryFactStore::new(),
                cache: &mut MemoryCache::new(u64::MAX),
                clock: &FixedClock(Timestamp(0)),
                cancel: &CancelFlag::new(),
            },
            &PackRequest {
                view: ViewKey::parse("hostile").unwrap(),
                policy: &policy,
                debug_ignored: false,
                max_diff_files: u32::MAX,
                limits: PackLimits::unbounded(),
            },
        )
        .unwrap();
        assert_eq!(
            result.diagnostics,
            [Diagnostic::UnrepresentableName {
                parent: String::new()
            }],
            "{name:?}"
        );
        assert!(result.published);
        let [member] = result.snapshot.tree().children.as_slice() else {
            panic!("expected one valid member for {name:?}");
        };
        assert_eq!(member.relative_path, "ok");
        assert_eq!(member.file_content.as_deref(), Some(b"x".as_slice()));
        assert_eq!(result.snapshot.digests().len(), 1);
    }
}
