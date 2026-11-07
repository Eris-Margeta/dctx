// SPDX-License-Identifier: Apache-2.0
use super::bytes::{decision_byte, freshness_byte, push_count, push_str, push_u32, push_u64};
use super::{MAGIC, VERSION};
use crate::domain::{DctxError, ScanLimits};
use crate::index::Generation;
use std::collections::BTreeMap;

pub(in crate::session) fn encode_generation(
    source_key: &str,
    view: &str,
    limits: &ScanLimits,
    generation: &Generation,
) -> Result<Vec<u8>, DctxError> {
    let mut out = Vec::new();
    let mut strings = Strings::default();
    strings.push(&mut out, source_key)?;
    strings.push(&mut out, view)?;
    out.push(freshness_byte(generation.freshness));
    push_u64(&mut out, generation.epoch);
    strings.push(&mut out, &generation.digest)?;
    strings.push(&mut out, &generation.coverage)?;
    strings.push(&mut out, &generation.completion)?;
    strings.push(&mut out, &generation.parser)?;
    push_u64(&mut out, generation.taken_at);
    push_u64(&mut out, generation.id);
    push_u64(&mut out, limits.entries);
    push_u64(&mut out, limits.depth);
    push_u64(&mut out, limits.file_bytes);
    push_u64(&mut out, limits.source_bytes);
    push_count(&mut out, generation.files.len())?;
    for file in &generation.files {
        strings.push(&mut out, &file.path)?;
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
    for node in &generation.nodes {
        out.push(node.letter);
        push_u32(&mut out, node.ordinal);
        strings.push(&mut out, &node.kind)?;
        strings.push(&mut out, &node.name)?;
        strings.push(&mut out, generation.text.get(node.path))?;
        strings.push(&mut out, &node.summary)?;
        strings.push(&mut out, &node.summary_source)?;
        strings.push(&mut out, &node.signature)?;
        push_u32(&mut out, node.lines[0]);
        push_u32(&mut out, node.lines[1]);
        push_u64(&mut out, node.span[0]);
        push_u64(&mut out, node.span[1]);
    }
    push_count(&mut out, generation.edges.len())?;
    for edge in &generation.edges {
        if edge.src as usize >= generation.nodes.len()
            || edge.dst as usize >= generation.nodes.len()
        {
            return Err(DctxError::CorruptIndex {
                reason: "generation edge",
            });
        }
        push_u32(&mut out, edge.src);
        push_u32(&mut out, edge.dst);
        strings.push(&mut out, generation.text.get(edge.edge))?;
        strings.push(&mut out, generation.text.get(edge.confidence))?;
    }
    let mut encoded = Vec::new();
    encoded.extend_from_slice(MAGIC);
    encoded.extend_from_slice(&VERSION.to_le_bytes());
    push_u64(&mut encoded, strings.values.len() as u64);
    for value in strings.values {
        push_str(&mut encoded, value)?;
    }
    encoded.extend_from_slice(&out);
    Ok(encoded)
}

/// One copy of each string, including equal heading names, summaries and signatures.
#[derive(Default)]
struct Strings<'a> {
    ids: BTreeMap<&'a str, u32>,
    values: Vec<&'a str>,
}

impl<'a> Strings<'a> {
    fn push(&mut self, out: &mut Vec<u8>, value: &'a str) -> Result<(), DctxError> {
        let id = if let Some(id) = self.ids.get(value) {
            *id
        } else {
            let id = u32::try_from(self.values.len()).map_err(|_| DctxError::CorruptIndex {
                reason: "generation strings",
            })?;
            self.values.push(value);
            self.ids.insert(value, id);
            id
        };
        push_u32(out, id);
        Ok(())
    }
}
