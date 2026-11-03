// SPDX-License-Identifier: Apache-2.0
//! View key prelude and dense node records.
//! Implements SEGMENTS-v1 kind 5. Dense ids are assigned in public-id order and are not portable.
//! Depends on [`super::facts`] for the fact-key bytes. View adjacency addresses these ids.
//! Zero nodes still store the 116-byte view key. An empty section has no view key.

use super::cursor::{push_bool, push_bytes, push_record, push_u32, push_u64, Cursor};
use super::dictionary::dict_str;
use super::error::SegmentError;
use super::facts::{encode_key, parse_key, FactKey};
use super::limits::{bounded_count, DecoderLimits};

/// View identity stored in front of the node records.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewKey {
    /// Dictionary id of the trust namespace. The string is non-empty.
    pub trust: u32,
    /// Dictionary id of the root. The string is non-empty.
    pub root: u32,
    /// Source snapshot digest, raw SHA-256.
    pub snapshot: [u8; 32],
    /// Overlay generation chosen by the caller.
    pub overlay_generation: u64,
    /// Selection policy digest, raw SHA-256.
    pub policy: [u8; 32],
    /// Dictionary id of the resolver revision. The string is non-empty.
    pub resolver_revision: u32,
    /// Resolver environment digest, raw SHA-256.
    pub environment: [u8; 32],
}

/// One manifest-local node. `dense_id` equals the node's index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewNode {
    /// Index of this node, from zero in public-id order.
    pub dense_id: u32,
    /// Dictionary id of the root-relative path.
    pub path: u32,
    /// Dictionary id of the public id.
    pub public_id: u32,
    /// Fact key for the bytes this node projects.
    pub fact: FactKey,
    /// Caller-supplied ordinal of the fact inside that key.
    pub local_fact_id: u32,
    /// Whether ranking treats the node as public.
    pub public: bool,
    /// Whether ranking applies the tier-0 boost.
    pub tier0: bool,
}

pub(crate) fn encode<S: AsRef<str>>(
    key: &ViewKey,
    nodes: &[ViewNode],
    dictionary: &[S],
    limits: &DecoderLimits,
) -> Result<(Vec<u8>, u64), SegmentError> {
    if u64::try_from(nodes.len()).unwrap_or(u64::MAX) > limits.max_records {
        return Err(SegmentError::Limit("records"));
    }
    validate_key(key, dictionary)?;
    let mut out = Vec::new();
    write_key(&mut out, key);
    let mut previous: Option<&str> = None;
    let mut paths = std::collections::BTreeSet::new();
    for (index, node) in nodes.iter().enumerate() {
        let dense = u32::try_from(index).map_err(|_| SegmentError::Arithmetic)?;
        if node.dense_id != dense {
            return Err(SegmentError::Nodes("order"));
        }
        let public_text = require_text(dictionary, node.public_id, "empty public")?;
        match previous {
            Some(prior) if public_text == prior => {
                return Err(SegmentError::Nodes("duplicate public"));
            }
            Some(prior) if public_text < prior => return Err(SegmentError::Nodes("order")),
            _ => {}
        }
        previous = Some(public_text);
        let path_text = require_text(dictionary, node.path, "empty path")?;
        if !paths.insert(path_text) {
            return Err(SegmentError::Nodes("duplicate path"));
        }
        require_key(&node.fact, dictionary)?;
        let mut body = Vec::new();
        push_u32(&mut body, node.dense_id);
        push_u32(&mut body, node.path);
        push_u32(&mut body, node.public_id);
        push_bytes(&mut body, &encode_key(&node.fact));
        push_u32(&mut body, node.local_fact_id);
        push_bool(&mut body, node.public);
        push_bool(&mut body, node.tier0);
        push_record(&mut out, &body, limits.max_record_bytes)?;
    }
    Ok((out, u64::try_from(nodes.len()).map_err(|_| SegmentError::Arithmetic)?))
}

pub(crate) fn decode<S: AsRef<str>>(
    payload: &[u8],
    record_count: u64,
    dictionary: &[S],
    limits: &DecoderLimits,
) -> Result<(ViewKey, Vec<ViewNode>), SegmentError> {
    if payload.is_empty() {
        return Err(SegmentError::Role("view key"));
    }
    if payload.len() < 116 {
        return Err(SegmentError::Truncated { at: 0, need: 116 });
    }
    let mut cursor = Cursor::new(payload, 0);
    let key = read_key(&mut cursor, dictionary)?;
    let rest = u64::try_from(cursor.rest().len()).unwrap_or(u64::MAX);
    let room = bounded_count(record_count, 4, rest, limits.max_records)?;
    let mut nodes = Vec::with_capacity(room);
    let mut previous: Option<&str> = None;
    let mut paths = std::collections::BTreeSet::new();
    for _ in 0..record_count {
        let rec_len = cursor.u32()?;
        if rec_len > limits.max_record_bytes {
            return Err(SegmentError::Limit("record"));
        }
        let record = cursor.take(usize::try_from(rec_len).map_err(|_| SegmentError::Arithmetic)?)?;
        let node = read_node(record, dictionary)?;
        let dense = u32::try_from(nodes.len()).map_err(|_| SegmentError::Arithmetic)?;
        if node.dense_id != dense {
            return Err(SegmentError::Nodes("order"));
        }
        let public_text = require_text(dictionary, node.public_id, "empty public")?;
        match previous {
            Some(prior) if public_text == prior => {
                return Err(SegmentError::Nodes("duplicate public"));
            }
            Some(prior) if public_text < prior => return Err(SegmentError::Nodes("order")),
            _ => {}
        }
        previous = Some(public_text);
        let path_text = require_text(dictionary, node.path, "empty path")?;
        if !paths.insert(path_text) {
            return Err(SegmentError::Nodes("duplicate path"));
        }
        nodes.push(node);
    }
    if !cursor.rest().is_empty() {
        return Err(SegmentError::Nodes("trailing"));
    }
    Ok((key, nodes))
}

fn write_key(out: &mut Vec<u8>, key: &ViewKey) {
    push_u32(out, key.trust);
    push_u32(out, key.root);
    push_bytes(out, &key.snapshot);
    push_u64(out, key.overlay_generation);
    push_bytes(out, &key.policy);
    push_u32(out, key.resolver_revision);
    push_bytes(out, &key.environment);
}

fn read_key<S: AsRef<str>>(cursor: &mut Cursor<'_>, dictionary: &[S]) -> Result<ViewKey, SegmentError> {
    let key = ViewKey {
        trust: cursor.u32()?,
        root: cursor.u32()?,
        snapshot: cursor.array::<32>()?,
        overlay_generation: cursor.u64()?,
        policy: cursor.array::<32>()?,
        resolver_revision: cursor.u32()?,
        environment: cursor.array::<32>()?,
    };
    validate_key(&key, dictionary)?;
    Ok(key)
}

fn validate_key<S: AsRef<str>>(key: &ViewKey, dictionary: &[S]) -> Result<(), SegmentError> {
    for id in [key.trust, key.root, key.resolver_revision] {
        if dict_str(dictionary, id).map_err(|_| SegmentError::Nodes("key"))?.is_empty() {
            return Err(SegmentError::Nodes("key"));
        }
    }
    Ok(())
}

fn read_node<S: AsRef<str>>(record: &[u8], dictionary: &[S]) -> Result<ViewNode, SegmentError> {
    let mut cursor = Cursor::new(record, 0);
    let node = ViewNode {
        dense_id: cursor.u32()?,
        path: cursor.u32()?,
        public_id: cursor.u32()?,
        fact: parse_key(&mut cursor).map_err(|_| SegmentError::Nodes("key"))?,
        local_fact_id: cursor.u32()?,
        public: cursor.flag()?,
        tier0: cursor.flag()?,
    };
    if !cursor.rest().is_empty() {
        return Err(SegmentError::Nodes("trailing"));
    }
    require_key(&node.fact, dictionary)?;
    Ok(node)
}

fn require_text<'a, S: AsRef<str>>(
    dictionary: &'a [S],
    id: u32,
    empty: &'static str,
) -> Result<&'a str, SegmentError> {
    let text = dict_str(dictionary, id).map_err(|_| SegmentError::Nodes("path"))?;
    if text.is_empty() {
        return Err(SegmentError::Nodes(empty));
    }
    Ok(text)
}

fn require_key<S: AsRef<str>>(key: &FactKey, dictionary: &[S]) -> Result<(), SegmentError> {
    for id in [key.dialect, key.parser_id, key.parser_revision] {
        dict_str(dictionary, id).map_err(|_| SegmentError::Nodes("key"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::kinds::Language;

    fn key() -> ViewKey {
        ViewKey {
            trust: 1,
            root: 2,
            snapshot: [3; 32],
            overlay_generation: 1,
            policy: [4; 32],
            resolver_revision: 3,
            environment: [5; 32],
        }
    }

    fn node() -> ViewNode {
        ViewNode {
            dense_id: 0,
            path: 4,
            public_id: 5,
            fact: FactKey {
                digest: [7; 32],
                byte_length: 1,
                language: Language::Rust,
                dialect: 1,
                parser_id: 2,
                parser_revision: 3,
                extraction_plan: [8; 32],
            },
            local_fact_id: 9,
            public: true,
            tier0: false,
        }
    }

    #[test]
    fn one_node_round_trips_and_the_prelude_is_116_bytes() {
        let dictionary = ["", "ns", "root", "rev", "a.rs", "s:a"];
        let limits = DecoderLimits::standard();
        let (payload, count) = encode(&key(), &[node()], &dictionary, &limits).unwrap();
        assert_eq!(count, 1);
        assert_eq!(payload.len(), 116 + 4 + 118);
        let (decoded_key, decoded) = decode(&payload, count, &dictionary, &limits).unwrap();
        assert_eq!(decoded_key, key());
        assert_eq!(decoded, vec![node()]);
        assert_eq!(
            decode(&[], 0, &dictionary, &limits).unwrap_err(),
            SegmentError::Role("view key")
        );
    }
}
