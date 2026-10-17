//! Exact legacy writer layout over fact trees and caller sinks (D3-LEGACY-REMOVAL §5 D3-02).
//! Writes little-endian metadata in tree order, then bodies or legacy zero padding.
//! Depends on legacy_codec and std::io::Write; never opens a file or changes the tree.

#![forbid(unsafe_code)]

use crate::legacy_codec::{
    LEGACY_HEADER_BYTES, LEGACY_MAGIC, LEGACY_MAX_DEPTH, LegacyError, LegacyLimits, LegacyNode, add, bounded,
    check_version,
};
use dctx_core::datatypes::NodeType;
use dctx_core::version::Version;
use std::io::Write;

/// Encode snapshot facts to the exact layout of the legacy writer.
/// Loaded bytes override content_size; absent bytes produce content_size zeros.
/// Offsets are recalculated in child order without mutating the input tree.
pub fn encode_legacy(root: &LegacyNode, version: &Version, limits: LegacyLimits) -> Result<Vec<u8>, LegacyError> {
    let total = validate(root, version, limits)?;
    let mut bytes = Vec::with_capacity(total);
    write_document(root, version, &mut bytes)?;
    Ok(bytes)
}

/// Validate the whole tree before writing, then encode into the caller's sink.
/// A sink error may leave a partial document; flushing/commit belongs to the caller.
pub fn encode_legacy_to<W: Write + ?Sized>(
    root: &LegacyNode,
    version: &Version,
    limits: LegacyLimits,
    sink: &mut W,
) -> Result<(), LegacyError> {
    validate(root, version, limits)?;
    write_document(root, version, sink)
}

fn size(node: &LegacyNode) -> Result<usize, LegacyError> {
    match &node.file_content {
        Some(bytes) => Ok(bytes.len()),
        None => usize::try_from(node.content_size).map_err(|_| LegacyError::Overflow),
    }
}

fn validate(root: &LegacyNode, version: &Version, limits: LegacyLimits) -> Result<usize, LegacyError> {
    check_version(version)?;
    let mut stack = vec![(root, 0)];
    let mut nodes = 0;
    let mut total = LEGACY_HEADER_BYTES;
    bounded(total, limits.max_total_bytes, "total bytes")?;
    while let Some((node, depth)) = stack.pop() {
        bounded(depth, limits.max_depth.min(LEGACY_MAX_DEPTH), "depth")?;
        nodes = add(nodes, 1)?;
        bounded(nodes, limits.max_nodes, "nodes")?;
        bounded(node.relative_path.len(), limits.max_name_bytes.min(usize::from(u16::MAX)), "name bytes")?;
        let metadata = match node.node_type {
            NodeType::File => {
                if !node.children.is_empty() {
                    return Err(LegacyError::InvalidTree);
                }
                total = add(total, size(node)?)?;
                27
            }
            NodeType::Directory => {
                if node.file_content.is_some() {
                    return Err(LegacyError::InvalidTree);
                }
                u32::try_from(node.children.len()).map_err(|_| LegacyError::Overflow)?;
                bounded(add(add(nodes, stack.len())?, node.children.len())?, limits.max_nodes, "nodes")?;
                let next_depth = add(depth, 1)?;
                stack.extend(node.children.iter().rev().map(|child| (child, next_depth)));
                15
            }
        };
        total = add(total, add(metadata, node.relative_path.len())?)?;
        bounded(total, limits.max_total_bytes, "total bytes")?;
    }
    u64::try_from(total).map_err(|_| LegacyError::Overflow)?;
    Ok(total)
}

fn write<W: Write + ?Sized>(sink: &mut W, bytes: &[u8]) -> Result<(), LegacyError> {
    sink.write_all(bytes).map_err(LegacyError::Sink)
}

fn write_document<W: Write + ?Sized>(root: &LegacyNode, version: &Version, sink: &mut W) -> Result<(), LegacyError> {
    write(sink, LEGACY_MAGIC)?;
    write(sink, &version.major.to_le_bytes())?;
    write(sink, &version.minor.to_le_bytes())?;
    let mut offset = 0;
    write_node(root, sink, &mut offset)?;
    write_bodies(root, sink)
}

fn write_node<W: Write + ?Sized>(node: &LegacyNode, sink: &mut W, offset: &mut u64) -> Result<(), LegacyError> {
    let kind = if node.node_type == NodeType::File { 0 } else { 1 };
    write(sink, &[kind])?;
    let name_len = u16::try_from(node.relative_path.len()).map_err(|_| LegacyError::Overflow)?;
    write(sink, &name_len.to_le_bytes())?;
    write(sink, node.relative_path.as_bytes())?;
    write(sink, &node.last_modified_timestamp.to_le_bytes())?;
    if node.node_type == NodeType::File {
        let size = u64::try_from(size(node)?).map_err(|_| LegacyError::Overflow)?;
        write(sink, &offset.to_le_bytes())?;
        write(sink, &size.to_le_bytes())?;
        *offset = offset.checked_add(size).ok_or(LegacyError::Overflow)?;
    } else {
        let count = u32::try_from(node.children.len()).map_err(|_| LegacyError::Overflow)?;
        write(sink, &count.to_le_bytes())?;
        for child in &node.children {
            write_node(child, sink, offset)?;
        }
    }
    Ok(())
}

fn write_bodies<W: Write + ?Sized>(node: &LegacyNode, sink: &mut W) -> Result<(), LegacyError> {
    if node.node_type == NodeType::File {
        if let Some(bytes) = &node.file_content {
            write(sink, bytes)?;
        } else {
            // Old writer zero-filled omitted bodies; use bounded chunks, not a size-sized allocation.
            let zeros = [0; 8192];
            let mut remaining = size(node)?;
            while remaining != 0 {
                let count = remaining.min(zeros.len());
                write(sink, &zeros[..count])?;
                remaining -= count;
            }
        }
    }
    for child in &node.children {
        write_bodies(child, sink)?;
    }
    Ok(())
}
