// SPDX-License-Identifier: Apache-2.0
//! Bounded DIRCTXTV decoding over immutable bytes (D3-LEGACY-REMOVAL §5 D3-02).
//! Ports the old reader's metadata layout and validates the whole data section.
//! Depends on `legacy_codec` and core fact nodes; used by explicit facade imports.

#![forbid(unsafe_code)]

use crate::legacy_codec::{
    add, bounded, check_version, LegacyBodies, LegacyDocument, LegacyError, LegacyHeader,
    LegacyLimits, LegacyNode, LEGACY_MAGIC, LEGACY_MAX_DEPTH,
};
use dctx_core::datatypes::NodeType;
use dctx_core::version::Version;

/// Decode a complete legacy file without touching its input or consulting a host.
/// Both body modes validate all offsets and sizes. Extra trailing bytes are refused.
/// Snapshot revisions are not format versions: any positive major is supported.
///
/// # Errors
/// Refuses malformed or truncated documents, invalid versions, names, offsets or bodies, arithmetic overflow and exceeded decode limits.
pub fn decode_legacy(
    bytes: &[u8],
    bodies: LegacyBodies,
    limits: LegacyLimits,
) -> Result<LegacyDocument, LegacyError> {
    bounded(bytes.len(), limits.max_total_bytes, "total bytes")?;
    let mut reader = Reader {
        bytes,
        position: 0,
        nodes: 0,
        limits,
    };
    if reader.take(8)? != LEGACY_MAGIC {
        return Err(LegacyError::BadMagic);
    }
    let version = Version {
        major: u32::from_le_bytes(reader.array()?),
        minor: u32::from_le_bytes(reader.array()?),
    };
    check_version(&version)?;
    let mut root = reader.node(0)?;
    let data_offset = reader.position;
    let data = bytes.get(data_offset..).ok_or(LegacyError::Truncated)?;
    validate_ranges(&root, data, limits)?;
    if bodies == LegacyBodies::Include {
        load_bodies(&mut root, data)?;
    }
    Ok(LegacyDocument {
        header: LegacyHeader {
            version,
            data_offset,
        },
        root,
    })
}

struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
    nodes: usize,
    limits: LegacyLimits,
}

impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], LegacyError> {
        let end = add(self.position, count)?;
        let value = self
            .bytes
            .get(self.position..end)
            .ok_or(LegacyError::Truncated)?;
        self.position = end;
        Ok(value)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], LegacyError> {
        self.take(N)?.try_into().map_err(|_| LegacyError::Truncated)
    }

    fn node(&mut self, depth: usize) -> Result<LegacyNode, LegacyError> {
        bounded(depth, self.limits.max_depth.min(LEGACY_MAX_DEPTH), "depth")?;
        self.nodes = add(self.nodes, 1)?;
        bounded(self.nodes, self.limits.max_nodes, "nodes")?;
        let kind = match self.array::<1>()?[0] {
            0 => NodeType::File,
            1 => NodeType::Directory,
            other => return Err(LegacyError::UnknownNodeType(other)),
        };
        let name_len = usize::from(u16::from_le_bytes(self.array()?));
        bounded(name_len, self.limits.max_name_bytes, "name bytes")?;
        let name =
            std::str::from_utf8(self.take(name_len)?).map_err(|_| LegacyError::InvalidUtf8)?;
        let mut node = LegacyNode::new(kind, name, std::path::PathBuf::new());
        node.last_modified_timestamp = u64::from_le_bytes(self.array()?);
        match kind {
            NodeType::File => {
                node.content_offset_in_data_section = u64::from_le_bytes(self.array()?);
                node.content_size = u64::from_le_bytes(self.array()?);
            }
            NodeType::Directory => {
                let count = usize::try_from(u32::from_le_bytes(self.array()?))
                    .map_err(|_| LegacyError::Overflow)?;
                bounded(add(self.nodes, count)?, self.limits.max_nodes, "nodes")?;
                // Even an empty directory requires 15 metadata bytes. Do not reserve
                // from a declared child count before checking the available buffer.
                if count > self.bytes.len().saturating_sub(self.position) / 15 {
                    return Err(LegacyError::Truncated);
                }
                for _ in 0..count {
                    node.children.push(self.node(add(depth, 1)?)?);
                }
            }
        }
        Ok(node)
    }
}

fn range(node: &LegacyNode, data_len: usize) -> Result<std::ops::Range<usize>, LegacyError> {
    let end = node
        .content_offset_in_data_section
        .checked_add(node.content_size)
        .ok_or(LegacyError::Overflow)?;
    let start =
        usize::try_from(node.content_offset_in_data_section).map_err(|_| LegacyError::Overflow)?;
    let end = usize::try_from(end).map_err(|_| LegacyError::Overflow)?;
    if start > data_len {
        return Err(LegacyError::InvalidOffset);
    }
    if end > data_len {
        return Err(LegacyError::Truncated);
    }
    Ok(start..end)
}

fn validate_ranges(
    root: &LegacyNode,
    data: &[u8],
    limits: LegacyLimits,
) -> Result<(), LegacyError> {
    let mut stack = vec![root];
    let mut extent = 0;
    let mut body_bytes = 0;
    while let Some(node) = stack.pop() {
        if node.node_type == NodeType::File {
            let span = range(node, data.len())?;
            extent = extent.max(span.end);
            body_bytes = add(body_bytes, span.len())?;
            // Overlapping ranges must not amplify a small buffer into unbounded copies.
            bounded(body_bytes, limits.max_total_bytes, "body bytes")?;
        }
        stack.extend(&node.children);
    }
    if extent != data.len() {
        return Err(LegacyError::TrailingBytes);
    }
    Ok(())
}

fn load_bodies(node: &mut LegacyNode, data: &[u8]) -> Result<(), LegacyError> {
    if node.node_type == NodeType::File {
        let span = range(node, data.len())?;
        node.file_content = Some(data.get(span).ok_or(LegacyError::Truncated)?.to_vec());
    }
    for child in &mut node.children {
        load_bodies(child, data)?;
    }
    Ok(())
}
