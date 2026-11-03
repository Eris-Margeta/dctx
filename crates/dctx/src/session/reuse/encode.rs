// SPDX-License-Identifier: Apache-2.0
use super::bytes::{decision_byte, freshness_byte, push_count, push_str, push_u32, push_u64};
use super::{MAGIC, VERSION};
use crate::domain::{DctxError, ScanLimits};
use crate::index::{Generation, write_endpoint, write_node_id};

pub(in crate::session) fn encode_generation(
    source_key: &str,
    view: &str,
    limits: &ScanLimits,
    generation: &Generation,
) -> Result<Vec<u8>, DctxError> {
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    push_str(&mut out, source_key)?;
    push_str(&mut out, view)?;
    out.push(freshness_byte(generation.freshness));
    push_u64(&mut out, generation.epoch);
    push_str(&mut out, &generation.digest)?;
    push_str(&mut out, &generation.coverage)?;
    push_str(&mut out, &generation.completion)?;
    push_str(&mut out, &generation.parser)?;
    push_u64(&mut out, generation.taken_at);
    push_u64(&mut out, generation.id);
    push_u64(&mut out, limits.entries);
    push_u64(&mut out, limits.depth);
    push_u64(&mut out, limits.file_bytes);
    push_u64(&mut out, limits.source_bytes);
    push_count(&mut out, generation.files.len())?;
    for file in &generation.files {
        push_str(&mut out, &file.path)?;
        out.push(decision_byte(file.decision));
        match file.digest {
            Some(digest) => {
                out.push(1);
                out.extend_from_slice(digest.as_bytes());
                push_u64(&mut out, digest.byte_length());
            }
            None => out.push(0),
        }
        push_u64(&mut out, file.size);
        push_u64(&mut out, file.modified);
        match file.stamp {
            Some(stamp) => {
                out.push(1);
                out.extend_from_slice(&stamp.modified_ns.to_le_bytes());
                push_u64(&mut out, stamp.inode);
            }
            None => out.push(0),
        }
    }
    push_count(&mut out, generation.nodes.len())?;
    let mut id = String::new();
    for node in &generation.nodes {
        id.clear();
        write_node_id(&mut id, &generation.text, node);
        push_str(&mut out, &id)?;
        push_str(&mut out, &node.kind)?;
        push_str(&mut out, &node.name)?;
        push_str(&mut out, generation.text.get(node.path))?;
        push_str(&mut out, &node.summary)?;
        push_str(&mut out, &node.summary_source)?;
        push_str(&mut out, &node.signature)?;
        push_u32(&mut out, node.lines[0]);
        push_u32(&mut out, node.lines[1]);
        push_u64(&mut out, node.span[0]);
        push_u64(&mut out, node.span[1]);
    }
    push_count(&mut out, generation.edges.len())?;
    let mut src = String::new();
    let mut dst = String::new();
    for edge in &generation.edges {
        src.clear();
        dst.clear();
        write_endpoint(&mut src, &generation.nodes, &generation.text, edge.src);
        write_endpoint(&mut dst, &generation.nodes, &generation.text, edge.dst);
        push_str(&mut out, &src)?;
        push_str(&mut out, &dst)?;
        push_str(&mut out, generation.text.get(edge.edge))?;
        push_str(&mut out, generation.text.get(edge.confidence))?;
    }
    Ok(out)
}
