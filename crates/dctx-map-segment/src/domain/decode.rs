// SPDX-License-Identifier: Apache-2.0
//! Safe decoder for one immutable segment.
//! Implements SEGMENTS-v1 check order: structure, then checksums, then bodies.
//! Depends on the section codecs. Stores and tests call [`decode`] or [`decode_with_limits`].
//! A bad length, checksum, or cardinality is a [`SegmentError`]. The decoder does not panic.

use super::adjacency;
use super::checksum;
use super::dictionary;
use super::directory;
use super::error::SegmentError;
use super::fact_record;
use super::header;
use super::kinds::{SectionKind, SegmentRole};
use super::limits::DecoderLimits;
use super::nodes;
use super::postings::{self, Postings};
use super::prefix;
use super::validated::ValidatedSegment;

/// Decodes `bytes` for `role` with the standard ceilings.
///
/// # Errors
/// Refuses malformed headers, sections, checksums or records, role violations, inconsistent adjacency and exceeded standard ceilings.
pub fn decode(bytes: &[u8], role: SegmentRole) -> Result<ValidatedSegment<'_>, SegmentError> {
    decode_with_limits(bytes, role, DecoderLimits::standard())
}

/// Decodes `bytes` for `role`. `limits` cannot raise the standard ceilings.
///
/// # Errors
/// Refuses malformed headers, sections, checksums or records, role violations, inconsistent adjacency and exceeded effective ceilings.
pub fn decode_with_limits(
    bytes: &[u8],
    role: SegmentRole,
    limits: DecoderLimits,
) -> Result<ValidatedSegment<'_>, SegmentError> {
    let limits = limits.effective();
    let header = header::parse(bytes, &limits)?;
    let entries = directory::parse(bytes, header.file_len, header.section_count, &limits)?;
    checksum::check_envelope(bytes, &header.checksum)?;
    for entry in &entries {
        let body = directory::payload(bytes, entry)?;
        if checksum::sha256(body)? != entry.checksum {
            return Err(SegmentError::BadPayloadChecksum { kind: entry.kind });
        }
    }
    directory::check_role(&entries, role)?;
    let mut dictionary = Vec::new();
    let mut prefixes = Vec::new();
    let mut facts = Vec::new();
    let mut postings = Postings { lengths: Vec::new(), rows: Vec::new() };
    let mut saw_postings = false;
    let mut view_key = None;
    let mut nodes = Vec::new();
    let mut forward = Vec::new();
    let mut reverse_resolved = Vec::new();
    let mut reverse_unresolved = Vec::new();
    for entry in &entries {
        let Some(kind) = SectionKind::from_code(entry.kind) else {
            continue;
        };
        let body = directory::payload(bytes, entry)?;
        match kind {
            SectionKind::Dictionary => {
                dictionary = dictionary::decode(body, entry.record_count, &limits)?;
            }
            SectionKind::Prefix => {
                prefixes = prefix::decode(body, entry.record_count, &dictionary, &limits)?;
            }
            SectionKind::Facts => {
                facts = fact_record::decode(body, entry.record_count, &dictionary, &limits)?;
            }
            SectionKind::Postings => {
                let dict_len = u32::try_from(dictionary.len()).map_err(|_| SegmentError::Arithmetic)?;
                postings = postings::decode(body, entry.record_count, dict_len, role, &limits)?;
                saw_postings = true;
            }
            SectionKind::Nodes => {
                let (key, decoded) = nodes::decode(body, entry.record_count, &dictionary, &limits)?;
                view_key = Some(key);
                nodes = decoded;
            }
            SectionKind::Forward => {
                let node_count = u32::try_from(nodes.len()).map_err(|_| SegmentError::Arithmetic)?;
                forward = adjacency::decode_forward(
                    body,
                    entry.record_count,
                    node_count,
                    &dictionary,
                    &limits,
                )?;
            }
            SectionKind::Reverse => {
                let node_count = u32::try_from(nodes.len()).map_err(|_| SegmentError::Arithmetic)?;
                let (resolved, unresolved) = adjacency::decode_reverse(
                    body,
                    entry.record_count,
                    node_count,
                    &dictionary,
                    &limits,
                )?;
                reverse_resolved = resolved;
                reverse_unresolved = unresolved;
            }
        }
    }
    if role == SegmentRole::View {
        if view_key.is_none() {
            return Err(SegmentError::Role("view key"));
        }
        if saw_postings && postings.lengths.len() != nodes.len() {
            return Err(SegmentError::Role("length"));
        }
    }
    let (expect_resolved, expect_unresolved) = adjacency::reverse_of(&forward);
    if expect_resolved != reverse_resolved || expect_unresolved != reverse_unresolved {
        return Err(SegmentError::Adjacency("reverse mismatch"));
    }
    Ok(ValidatedSegment {
        role,
        minor: header.minor,
        bytes,
        entries,
        dictionary,
        prefixes,
        facts,
        postings,
        view_key,
        nodes,
        forward,
        reverse_resolved,
        reverse_unresolved,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Confidence, EdgeDraft, EdgeEnd, EdgeKind, Language, NodeDraft, SegmentBuilder, ViewKeyDraft,
    };

    fn empty_sections(extra: Option<(u16, u16, u32)>) -> Vec<u8> {
        let mut sections = vec![
            built(SectionKind::Dictionary.code(), 1, 1),
            built(SectionKind::Facts.code(), 1, 1),
        ];
        if let Some((kind, schema, flags)) = extra {
            sections.push(built(kind, schema, flags));
        }
        directory_assemble(&sections)
    }

    fn built(kind: u16, schema: u16, flags: u32) -> super::super::directory::BuiltSection {
        super::super::directory::BuiltSection {
            kind,
            schema,
            flags,
            record_count: 0,
            payload: Vec::new(),
        }
    }

    fn directory_assemble(sections: &[super::super::directory::BuiltSection]) -> Vec<u8> {
        super::super::directory::assemble(sections, &DecoderLimits::standard()).unwrap()
    }

    #[test]
    fn unknown_optional_sections_are_skipped_and_bad_schemas_are_refused() {
        let ok = empty_sections(None);
        assert!(decode(&ok, SegmentRole::Fact).is_ok());
        let skipped = empty_sections(Some((99, 1, 0)));
        assert!(decode(&skipped, SegmentRole::Fact).is_ok());
        let required = empty_sections(Some((99, 1, 1)));
        assert_eq!(
            decode(&required, SegmentRole::Fact).unwrap_err(),
            SegmentError::UnknownSection { kind: 99 }
        );
        let schema = directory_assemble(&[built(1, 2, 1), built(3, 1, 1)]);
        assert_eq!(
            decode(&schema, SegmentRole::Fact).unwrap_err(),
            SegmentError::UnknownSchema { kind: 1, schema: 2 }
        );
        let missing = directory_assemble(&[built(1, 1, 1)]);
        assert_eq!(
            decode(&missing, SegmentRole::Fact).unwrap_err(),
            SegmentError::Role("missing")
        );
    }

    #[test]
    fn a_reverse_section_that_disagrees_with_forward_is_refused() {
        let bytes = one_edge_view();
        assert!(decode(&bytes, SegmentRole::View).is_ok());
        let (entry_at, offset, len) = find_section(&bytes, SectionKind::Reverse.code());
        assert!(len > 15);
        let mut bad = bytes.clone();
        bad[offset + 15] = 1;
        let hash = super::super::checksum::sha256(&bad[offset..offset + len]).unwrap();
        bad[entry_at + 32..entry_at + 64].copy_from_slice(&hash);
        let sum = checksum::envelope_checksum(&bad).unwrap();
        bad[48..80].copy_from_slice(&sum);
        assert_eq!(
            decode(&bad, SegmentRole::View).unwrap_err(),
            SegmentError::Adjacency("reverse mismatch")
        );
    }

    fn one_edge_view() -> Vec<u8> {
        let mut builder = SegmentBuilder::view();
        builder
            .set_view_key(ViewKeyDraft {
                trust: "ns".to_owned(),
                root: "root".to_owned(),
                snapshot: [3; 32],
                overlay_generation: 1,
                policy: [4; 32],
                resolver_revision: "rev".to_owned(),
                environment: [5; 32],
            })
            .unwrap();
        builder.add_node(node("a.rs", "a", [7; 32])).unwrap();
        builder.add_node(node("b.rs", "b", [8; 32])).unwrap();
        builder
            .add_edge(EdgeDraft {
                source: "a".to_owned(),
                kind: EdgeKind::Imports,
                confidence: Confidence::Exact,
                target: EdgeEnd::Resolved("b".to_owned()),
            })
            .unwrap();
        builder.build().unwrap()
    }

    fn node(path: &str, public_id: &str, digest: [u8; 32]) -> NodeDraft {
        NodeDraft {
            path: path.to_owned(),
            public_id: public_id.to_owned(),
            digest,
            byte_length: 1,
            language: Language::Rust,
            dialect: "d".to_owned(),
            parser_id: "p".to_owned(),
            parser_revision: "r".to_owned(),
            extraction_plan: [2; 32],
            local_fact_id: 1,
            public: true,
            tier0: false,
        }
    }

    fn find_section(bytes: &[u8], kind: u16) -> (usize, usize, usize) {
        let count = u32::from_le_bytes(bytes[24..28].try_into().unwrap());
        for index in 0..count {
            let at = 80 + usize::try_from(index).unwrap() * 64;
            let found = u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap());
            if found == kind {
                let offset = usize::try_from(u64::from_le_bytes(bytes[at + 8..at + 16].try_into().unwrap())).unwrap();
                let len = usize::try_from(u64::from_le_bytes(bytes[at + 16..at + 24].try_into().unwrap())).unwrap();
                return (at, offset, len);
            }
        }
        panic!("missing section");
    }
}
