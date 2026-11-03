// SPDX-License-Identifier: Apache-2.0
//! Typed refusal for a segment that is truncated, corrupt, or outside v1.
//! Implements SEGMENTS-v1: corruption is an error, never a panic or a partial slice.
//! Depends on no other module. Every decoder in this crate returns [`SegmentError`].
//! The text in each variant is part of the test contract for that refusal.

/// Why a segment was not decoded or a ranking input was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SegmentError {
    /// The buffer ended before a required field.
    #[error("truncated at {at}, need {need} bytes")]
    Truncated {
        /// Offset where the read failed.
        at: u64,
        /// Bytes required from that offset, or the declared file length.
        need: u64,
    },
    /// Bytes 0..8 were not `DCTXSEG` followed by NUL.
    #[error("bad magic")]
    BadMagic,
    /// Major version is not 1. Minor versions of major 1 stay readable.
    #[error("unknown segment version {major}.{minor}")]
    UnknownVersion {
        /// Declared major version.
        major: u16,
        /// Declared minor version.
        minor: u16,
    },
    /// A fixed v1 header field had the wrong value.
    #[error("bad header: {0}")]
    BadHeader(&'static str),
    /// The required-feature bitset names a capability this decoder does not implement.
    #[error("unknown required feature {0:#x}")]
    UnknownFeature(u64),
    /// An offset plus a length overflowed, or did not fit in the file.
    #[error("offset overflow")]
    OffsetOverflow,
    /// The buffer is longer than the length declared in the header.
    #[error("trailing bytes")]
    TrailingBytes,
    /// SHA-256 of the file with the checksum field zeroed did not match.
    #[error("bad envelope checksum")]
    BadEnvelopeChecksum,
    /// SHA-256 of one section payload did not match its directory entry.
    #[error("bad payload checksum for section {kind}")]
    BadPayloadChecksum {
        /// Section kind whose payload hash failed.
        kind: u16,
    },
    /// The section directory is not canonical v1.
    #[error("directory: {0}")]
    Directory(&'static str),
    /// A decoder ceiling was exceeded. A tighter caller cap wins over the default.
    #[error("limit: {0}")]
    Limit(&'static str),
    /// A length-prefixed byte string was not UTF-8.
    #[error("utf-8 in section {kind}")]
    Utf8 {
        /// Section kind that held the bytes.
        kind: u16,
    },
    /// A base-128 integer was overlong, overflowing, or unfinished.
    #[error("varint: {0}")]
    Varint(&'static str),
    /// The string dictionary is not unique, sorted, or in range.
    #[error("dictionary: {0}")]
    Dictionary(&'static str),
    /// A sparse prefix range does not match the dictionary.
    #[error("prefix: {0}")]
    Prefix(&'static str),
    /// Postings are unsorted, out of range, or not canonical deltas.
    #[error("postings: {0}")]
    Postings(&'static str),
    /// A fact key, span, or record is not canonical.
    #[error("facts: {0}")]
    Facts(&'static str),
    /// A view node or view key is not canonical.
    #[error("nodes: {0}")]
    Nodes(&'static str),
    /// Forward and reverse adjacency disagree, or an edge is illegal.
    #[error("adjacency: {0}")]
    Adjacency(&'static str),
    /// The section set does not match the fact or view role.
    #[error("role: {0}")]
    Role(&'static str),
    /// A ranking input is empty or otherwise unusable.
    #[error("ranking: {0}")]
    Ranking(&'static str),
    /// A checked add or multiply overflowed.
    #[error("arithmetic overflow")]
    Arithmetic,
    /// An unknown section kind was marked required.
    #[error("unknown required section {kind}")]
    UnknownSection {
        /// Directory kind that this decoder does not implement.
        kind: u16,
    },
    /// A known section kind used a schema other than 1.
    #[error("unknown schema {schema} for section {kind}")]
    UnknownSchema {
        /// Section kind.
        kind: u16,
        /// Schema number that was refused.
        schema: u16,
    },
    /// A count does not match the bytes or the directory.
    #[error("cardinality: {0}")]
    Cardinality(&'static str),
}
