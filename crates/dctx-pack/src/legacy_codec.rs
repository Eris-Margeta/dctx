//! Types and bounds for the DIRCTXTV compatibility codec (D3-LEGACY-REMOVAL §5 D3-02).
//! Shares the snapshot fact node with dctx-core; carries no host paths or IO capabilities.
//! Used by the pure decoder, encoder and the facade's explicit import operation.

#![forbid(unsafe_code)]

use dctx_core::version::Version;
use std::fmt;

/// Legacy signature. The trailing V identifies the versioned header layout.
pub const LEGACY_MAGIC: &[u8; 8] = b"DIRCTXTV";
/// Signature plus two little-endian snapshot revision integers.
pub const LEGACY_HEADER_BYTES: usize = 16;
/// Hard ceiling also protects recursive clone/drop of the shared fact tree.
pub const LEGACY_MAX_DEPTH: usize = 128;

/// Legacy node facts: paths, kinds, timestamps, offsets, sizes and optional bodies.
/// Non-wire fields retain the core constructor defaults; disk paths stay empty on decode.
pub type LegacyNode = dctx_core::datatypes::DirContextTreeNode;

/// The fixed header and the located start of the data section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyHeader {
    /// Snapshot revision, not a codec version: every positive major is supported.
    pub version: Version,
    /// Absolute byte offset immediately after the tree metadata.
    pub data_offset: usize,
}

/// Decoded legacy facts. Metadata-only decoding never invents file content.
#[derive(Debug, Clone)]
pub struct LegacyDocument {
    /// Header and data section location.
    pub header: LegacyHeader,
    /// Tree in its original child order.
    pub root: LegacyNode,
}

/// Explicit choice of whether to materialize the validated data section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacyBodies {
    /// Match the old reader: retain sizes and offsets, with every body `None`.
    MetadataOnly,
    /// Copy file ranges, including empty bodies. Zero padding cannot be distinguished
    /// from actual zero bytes because the format stores no omission marker.
    Include,
}

/// Limits apply before allocation and before an encoder writes to its sink.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LegacyLimits {
    /// Maximum encoded input/output bytes and aggregate materialized body bytes.
    pub max_total_bytes: usize,
    /// Maximum nesting, with root at depth zero; capped by [`LEGACY_MAX_DEPTH`].
    pub max_depth: usize,
    /// Maximum UTF-8 bytes in one stored relative path (wire maximum is `u16::MAX`).
    pub max_name_bytes: usize,
    /// Maximum nodes including the root.
    pub max_nodes: usize,
}

impl Default for LegacyLimits {
    fn default() -> Self {
        Self { max_total_bytes: 64 * 1024 * 1024, max_depth: 128, max_name_bytes: 4096, max_nodes: 100_000 }
    }
}

/// Typed malformed-input, resource and sink failures; never embeds file bodies.
#[derive(Debug)]
pub enum LegacyError {
    /// Signature differs from DIRCTXTV.
    BadMagic,
    /// A field or required body range is incomplete.
    Truncated,
    /// Snapshot revision has the unsupported zero major.
    UnknownVersion {
        /// Stored major revision.
        major: u32,
        /// Stored minor revision.
        minor: u32,
    },
    /// Unknown one-byte node discriminant.
    UnknownNodeType(u8),
    /// A relative path is not UTF-8.
    InvalidUtf8,
    /// Requested work exceeds a named explicit bound.
    LimitExceeded(&'static str),
    /// Integer addition, conversion or size computation overflowed.
    Overflow,
    /// A file offset is outside the data section.
    InvalidOffset,
    /// Bytes remain outside all declared content ranges.
    TrailingBytes,
    /// A fact tree has children on a file or a body on a directory.
    InvalidTree,
    /// The caller's sink refused a write; partial output may have been written.
    Sink(std::io::Error),
}

impl fmt::Display for LegacyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadMagic => formatter.write_str("invalid DIRCTXTV signature"),
            Self::Truncated => formatter.write_str("truncated legacy snapshot"),
            Self::UnknownVersion { major, minor } => write!(formatter, "unsupported legacy revision {major}.{minor}"),
            Self::UnknownNodeType(kind) => write!(formatter, "unknown legacy node type {kind}"),
            Self::InvalidUtf8 => formatter.write_str("legacy path is not UTF-8"),
            Self::LimitExceeded(bound) => write!(formatter, "legacy {bound} limit exceeded"),
            Self::Overflow => formatter.write_str("legacy size arithmetic overflow"),
            Self::InvalidOffset => formatter.write_str("legacy offset outside data section"),
            Self::TrailingBytes => formatter.write_str("trailing legacy data"),
            Self::InvalidTree => formatter.write_str("invalid legacy fact tree"),
            Self::Sink(error) => write!(formatter, "legacy sink: {error}"),
        }
    }
}

impl std::error::Error for LegacyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Sink(error) => Some(error),
            _ => None,
        }
    }
}

pub(crate) fn check_version(version: &Version) -> Result<(), LegacyError> {
    if version.major == 0 {
        return Err(LegacyError::UnknownVersion { major: version.major, minor: version.minor });
    }
    Ok(())
}

/// Decoding still accepts every positive major, including `u32::MAX`.
/// Publication as the base of another pack requires room for one major reset.
///
/// # Errors
/// Refuses a zero major revision or a major revision that cannot advance without overflow.
pub fn revision_can_advance(version: &Version) -> Result<(), LegacyError> {
    check_version(version)?;
    if version.major.checked_add(1).is_none() {
        return Err(LegacyError::Overflow);
    }
    Ok(())
}

pub(crate) fn add(left: usize, right: usize) -> Result<usize, LegacyError> {
    left.checked_add(right).ok_or(LegacyError::Overflow)
}

pub(crate) fn bounded(value: usize, limit: usize, name: &'static str) -> Result<(), LegacyError> {
    if value > limit {
        return Err(LegacyError::LimitExceeded(name));
    }
    Ok(())
}
