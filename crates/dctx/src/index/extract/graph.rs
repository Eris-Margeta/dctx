// SPDX-License-Identifier: Apache-2.0
//! Projection of prepared symbols and complete config lexes into indexed nodes and contains edges.
use super::*;

/// Projects `prepared` into `nodes` and `edges`. Returns the symbol count charged to the file.
pub(crate) fn index_prepared(
    path: &str,
    bytes: &[u8],
    path_span: TextSpan,
    module_index: u32,
    prepared: Prepared,
    nodes: &mut Vec<IndexedNode>,
    edges: &mut Vec<super::IndexedEdge>,
    text: &mut TextBlob,
) -> usize {
    match prepared {
        Prepared::Symbols(symbols) => {
            let count = symbols.len();
            index_symbols(path, path_span, module_index, symbols, nodes, edges, text);
            count
        }
        Prepared::Config(lexed) => append_cfg(
            path,
            bytes,
            path_span,
            module_index,
            lexed,
            nodes,
            edges,
            text,
        ),
    }
}

/// Moves a complete config lex into the graph. Duplicate keys never reach a complete lex.
fn append_cfg(
    path: &str,
    bytes: &[u8],
    path_span: TextSpan,
    module_index: u32,
    mut lexed: dctx_lex_cfg::LexedFile,
    nodes: &mut Vec<IndexedNode>,
    edges: &mut Vec<super::IndexedEdge>,
    text: &mut TextBlob,
) -> usize {
    let Ok(body) = std::str::from_utf8(bytes) else {
        return 0;
    };
    let spans = SpanIndex::build(body);
    let facts = std::mem::take(&mut lexed.nodes);
    drop(lexed);
    let edge = text.push("contains");
    let confidence = text.push("exact");
    let path_ok = StableId::new(NodeKind::Module, path, None, None).is_ok();
    nodes.reserve(facts.len());
    edges.reserve(facts.len());
    let mut kept = 0usize;
    for labeled in facts {
        let node = labeled.fact;
        if matches!(
            node.kind,
            NodeKind::Module | NodeKind::Place | NodeKind::Package
        ) || node.name.is_empty()
        {
            continue;
        }
        let graph = match node.kind {
            NodeKind::Doc => NodeKind::Doc,
            NodeKind::Test => NodeKind::Test,
            _ => NodeKind::Symbol,
        };
        if stored_ordinal(graph, path, &node.name, None, path_ok).is_none() {
            continue;
        }
        let Some(dst) = u32::try_from(nodes.len()).ok() else {
            break;
        };
        let kind_label = if graph == NodeKind::Doc {
            "heading"
        } else {
            node.symbol_kind
                .map_or("fn", dctx_map_model::SymbolKind::as_str)
        };
        let source_label = node.summary.source.as_str();
        let lines = node.lines;
        nodes.push(IndexedNode {
            letter: letter_byte(graph),
            ordinal: 0,
            kind: reuse_label(node.fingerprint, kind_label),
            name: node.name,
            path: path_span,
            lines,
            span: spans.line_span(lines),
            summary: node.summary.text,
            summary_source: reuse_label(node.path, source_label),
            signature: node.signature,
        });
        edges.push(super::IndexedEdge {
            src: module_index,
            dst,
            edge,
            confidence,
        });
        kept = kept.saturating_add(1);
    }
    kept
}

fn reuse_label(mut slot: String, label: &str) -> String {
    slot.clear();
    slot.push_str(label);
    slot
}

/// Assigns stable ids and contains edges for one file's symbols.
///
/// Consumes `symbols` so names, kinds and summaries move into the graph.
/// The id text is not built here. Rows keep the letter, the ordinal and the shared path.
/// `module_index` is the module node already pushed for this file.
pub(crate) fn index_symbols(
    path: &str,
    path_span: TextSpan,
    module_index: u32,
    symbols: Vec<Symbol>,
    nodes: &mut Vec<IndexedNode>,
    edges: &mut Vec<super::IndexedEdge>,
    text: &mut TextBlob,
) {
    let ordinals = duplicate_ordinals(&symbols);
    let path_ok = StableId::new(NodeKind::Module, path, None, None).is_ok();
    let edge = text.push("contains");
    let confidence = text.push("exact");
    nodes.reserve(ordinals.len());
    edges.reserve(ordinals.len());
    for (symbol, ordinal) in symbols.into_iter().zip(ordinals) {
        let Some(ordinal) = stored_ordinal(symbol.graph, path, &symbol.name, ordinal, path_ok)
        else {
            continue;
        };
        let Some(dst) = u32::try_from(nodes.len()).ok() else {
            break;
        };
        nodes.push(IndexedNode {
            letter: letter_byte(symbol.graph),
            ordinal,
            kind: symbol.kind,
            name: symbol.name,
            path: path_span,
            lines: symbol.lines,
            span: symbol.span,
            summary: symbol.summary,
            summary_source: symbol.summary_source,
            signature: symbol.signature,
        });
        edges.push(super::IndexedEdge {
            src: module_index,
            dst,
            edge,
            confidence,
        });
    }
}

/// First occurrence keeps an empty ordinal. Later copies use 2, 3, … in source order.
#[allow(
    clippy::cast_possible_truncation,
    reason = "The hash is an intentionally native-word table index; truncation keeps its low bits before masking to a power-of-two capacity."
)]
fn duplicate_ordinals(symbols: &[Symbol]) -> Vec<Option<u32>> {
    let capacity = table_len(symbols.len());
    let mask = capacity - 1;
    let mut slots = vec![
        Slot {
            hash: 0,
            index: 0,
            count: 0
        };
        capacity
    ];
    let mut ordinals = Vec::with_capacity(symbols.len());
    for (index, symbol) in symbols.iter().enumerate() {
        let mut hash = fx_hash(symbol.name.as_bytes());
        if hash == 0 {
            hash = 1;
        }
        let mut slot = (hash as usize) & mask;
        let stored = loop {
            let entry = &mut slots[slot];
            if entry.hash == 0 {
                entry.hash = hash;
                entry.index = u32::try_from(index).unwrap_or(u32::MAX);
                entry.count = 1;
                break None;
            }
            if entry.hash == hash {
                let prior = usize::try_from(entry.index).unwrap_or(usize::MAX);
                if symbols
                    .get(prior)
                    .is_some_and(|other| other.name == symbol.name)
                {
                    entry.count = entry.count.saturating_add(1);
                    break (entry.count > 1).then_some(entry.count);
                }
            }
            slot = (slot + 1) & mask;
        };
        ordinals.push(stored);
    }
    ordinals
}

/// `0` when this is the first copy. [`StableId`] still rejects a name that cannot be stored.
pub(super) fn stored_ordinal(
    kind: NodeKind,
    path: &str,
    name: &str,
    ordinal: Option<u32>,
    path_ok: bool,
) -> Option<u32> {
    if ordinal == Some(0) {
        return None;
    }
    let stored = ordinal.unwrap_or(0);
    if path_ok && name_is_plain(name) {
        return Some(stored);
    }
    StableId::new(kind, path, Some(name), ordinal).ok()?;
    Some(stored)
}

pub(super) fn letter_byte(kind: NodeKind) -> u8 {
    match kind {
        NodeKind::Place => b'p',
        NodeKind::Package => b'k',
        NodeKind::Module => b'm',
        NodeKind::Test => b't',
        NodeKind::Doc => b'd',
        NodeKind::Symbol => b's',
    }
}

#[derive(Clone, Copy)]
struct Slot {
    hash: u64,
    index: u32,
    count: u32,
}

fn table_len(count: usize) -> usize {
    count
        .saturating_mul(2)
        .max(16)
        .checked_next_power_of_two()
        .unwrap_or(1_usize << (usize::BITS - 1))
}

fn fx_hash(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0;
    let mut offset = 0;
    while offset + 8 <= bytes.len() {
        let mut chunk = [0u8; 8];
        chunk.copy_from_slice(&bytes[offset..offset + 8]);
        hash =
            (hash.rotate_left(5) ^ u64::from_le_bytes(chunk)).wrapping_mul(0x517c_c1b7_2722_0a95);
        offset += 8;
    }
    if offset < bytes.len() {
        let mut tail: u64 = 0;
        for (shift, byte) in bytes[offset..].iter().copied().enumerate() {
            tail |= u64::from(byte) << (shift * 8);
        }
        hash = (hash.rotate_left(5) ^ tail).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
    hash
}

fn name_is_plain(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let mut ascii = true;
    for byte in name.bytes() {
        if byte < 0x20 || byte == 0x7f || byte == b'#' || byte == b'~' {
            return false;
        }
        if byte >= 0x80 {
            ascii = false;
        }
    }
    ascii || !name.chars().any(char::is_control)
}
