//! In-memory map generation. SQL, tree-sitter nodes and storage paths stay out of this index.
//! Implements the strict half of SPEC-MAP-MODE over an explicit [`crate::ports`] source.
//! Depends on the domain types and dctx-map-model ids. The session owns each [`Generation`].
//! Callers see an [`crate::domain::Answer`], never this module.

mod concept;
mod extract;
pub(crate) mod export;
mod generic;
pub(crate) mod hubs;
pub(crate) mod path;
mod grep;
mod lex_dispatch;
pub(crate) mod profile;
mod query;
mod refs;
mod refs_names;
#[cfg(test)]
mod refs_tests;
mod segment;
pub(crate) mod sites;
mod skeleton;
mod span_index;
mod text_docs;
#[cfg(feature = "syntax-rust")]
mod syn_extract;
mod walk;

pub(crate) use grep::{grep_records, text_records};
pub(crate) use query::{collect, Collected};
pub(crate) use segment::canonical_bytes;
pub(crate) use walk::{walk_source, WalkArgs};

use dctx_core::FileHandlingDecision;
use dctx_hash::ContentDigest;

use crate::domain::FreshnessMode;

/// Byte range in a [`TextBlob`]. Empty text is `0..0`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TextSpan {
    start: u32,
    end: u32,
}

impl TextSpan {
    /// Span of the empty string.
    pub(crate) const EMPTY: Self = Self { start: 0, end: 0 };
}

/// One buffer of ids and labels for a generation. Nodes store [`TextSpan`]s into it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct TextBlob {
    buf: String,
}

impl TextBlob {
    /// Appends `text` and returns its span. A blob past 4 GiB yields [`TextSpan::EMPTY`].
    pub(crate) fn push(&mut self, text: &str) -> TextSpan {
        if text.is_empty() {
            return TextSpan::EMPTY;
        }
        let Ok(start) = u32::try_from(self.buf.len()) else {
            return TextSpan::EMPTY;
        };
        self.buf.push_str(text);
        let Ok(end) = u32::try_from(self.buf.len()) else {
            let keep = usize::try_from(start).unwrap_or(self.buf.len());
            self.buf.truncate(keep);
            return TextSpan::EMPTY;
        };
        TextSpan { start, end }
    }

    /// Borrows the text for `span`. A broken span is empty.
    pub(crate) fn get(&self, span: TextSpan) -> &str {
        if span.end <= span.start {
            return "";
        }
        let start = usize::try_from(span.start).unwrap_or(usize::MAX);
        let end = usize::try_from(span.end).unwrap_or(start);
        self.buf.get(start..end).unwrap_or("")
    }
}

/// Appends `{letter}:{path}#{name}` and `~{ordinal}` when `ordinal` is non-zero.
///
/// A module passes an empty name and ordinal `0`, which yields `m:{path}`.
pub(crate) fn write_stable_id(out: &mut String, letter: u8, path: &str, name: &str, ordinal: u32) {
    out.push(char::from(letter));
    out.push(':');
    out.push_str(path);
    if !name.is_empty() {
        out.push('#');
        out.push_str(name);
    }
    if ordinal > 0 {
        out.push('~');
        push_decimal(out, ordinal);
    }
}

/// Writes the stable id of `node` into `out`.
pub(crate) fn write_node_id(out: &mut String, text: &TextBlob, node: &IndexedNode) {
    write_stable_id(out, node.letter, text.get(node.path), &node.name, node.ordinal);
}

/// Writes the stable id of node `index`, or nothing when the index is absent.
pub(crate) fn write_endpoint(out: &mut String, nodes: &[IndexedNode], text: &TextBlob, index: u32) {
    let Some(index) = usize::try_from(index).ok() else {
        return;
    };
    let Some(node) = nodes.get(index) else {
        return;
    };
    write_node_id(out, text, node);
}

/// True when `target` is the stable id `node` would write.
pub(crate) fn id_matches(text: &TextBlob, node: &IndexedNode, target: &str) -> bool {
    if target.as_bytes().first().copied() != Some(node.letter) {
        return false;
    }
    let Some(after_colon) = target[1..].strip_prefix(':') else {
        return false;
    };
    let Some(after_path) = after_colon.strip_prefix(text.get(node.path)) else {
        return false;
    };
    if node.name.is_empty() {
        return node.ordinal == 0 && after_path.is_empty();
    }
    let Some(after_hash) = after_path.strip_prefix('#') else {
        return false;
    };
    let Some(after_name) = after_hash.strip_prefix(node.name.as_str()) else {
        return false;
    };
    if node.ordinal == 0 {
        return after_name.is_empty();
    }
    let Some(digits) = after_name.strip_prefix('~') else {
        return false;
    };
    parse_ordinal(digits) == Some(node.ordinal)
}

/// Ordinal stored in a persisted id. `0` means the id has no `~` suffix.
///
/// A name may itself contain `~`. The suffix is only the digits after the stored name.
pub(crate) fn ordinal_suffix(id: &str, name: &str) -> u32 {
    let Some((_, tail)) = id.rsplit_once('#') else {
        return 0;
    };
    if tail == name {
        return 0;
    }
    let Some(rest) = tail.strip_prefix(name) else {
        return 0;
    };
    let Some(digits) = rest.strip_prefix('~') else {
        return 0;
    };
    parse_ordinal(digits).unwrap_or(0)
}

fn parse_ordinal(digits: &str) -> Option<u32> {
    if digits.is_empty() || (digits.len() > 1 && digits.as_bytes().first().copied() == Some(b'0')) {
        return None;
    }
    let mut value: u32 = 0;
    for byte in digits.bytes() {
        if !byte.is_ascii_digit() {
            return None;
        }
        value = value.checked_mul(10)?.checked_add(u32::from(byte - b'0'))?;
    }
    Some(value)
}

fn push_decimal(out: &mut String, value: u32) {
    let mut buf = [0u8; 10];
    let mut cursor = buf.len();
    let mut rest = value;
    loop {
        cursor = cursor.saturating_sub(1);
        buf[cursor] = b'0'.saturating_add((rest % 10) as u8);
        rest /= 10;
        if rest == 0 {
            break;
        }
    }
    if let Ok(text) = std::str::from_utf8(&buf[cursor..]) {
        out.push_str(text);
    }
}

/// One visited file. Bodies are not retained; reads hash again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IndexedFile {
    /// Root-relative path.
    pub path: String,
    /// Decision at snapshot time. Queries recompute the live decision before emitting bytes.
    pub decision: FileHandlingDecision,
    /// Own content digest. `None` when the file was not read.
    pub digest: Option<ContentDigest>,
    /// Metadata size.
    pub size: u64,
    /// Metadata modification time, in milliseconds.
    pub modified: u64,
}

/// One graph node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IndexedNode {
    /// Stable-id letter (`s`, `t`, `d`, `m`). The wire kind does not carry it.
    pub letter: u8,
    /// Duplicate suffix. `0` means the id has no `~` ordinal.
    pub ordinal: u32,
    /// Wire kind (`module`, `fn`, `heading`, …).
    pub kind: String,
    /// Unqualified or qualified name. Empty for a module.
    pub name: String,
    /// Root-relative path in the generation blob.
    pub path: TextSpan,
    /// Inclusive 1-based lines.
    pub lines: [u32; 2],
    /// Byte span `[start, end)`.
    pub span: [u64; 2],
    /// Summary text.
    pub summary: String,
    /// `doc`, `heading`, `sig` or empty.
    pub summary_source: String,
    /// Signature without the body.
    pub signature: String,
}

/// One edge: `contains` from the walk, `calls` and `imports` from [`refs::link`]. Calls are never `exact`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IndexedEdge {
    /// Index of the source node in the generation.
    pub src: u32,
    /// Index of the destination node in the generation.
    pub dst: u32,
    /// Edge name in the generation blob.
    pub edge: TextSpan,
    /// Confidence label in the generation blob.
    pub confidence: TextSpan,
}

/// One pinned generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Generation {
    /// Generation id. The first snapshot is 1.
    pub id: u64,
    /// Own digest of the canonical payload.
    pub digest: String,
    /// Segment bytes charged to the cache.
    pub encoded_len: u64,
    /// Files that were not excluded.
    pub files: Vec<IndexedFile>,
    /// Modules, symbols and headings.
    pub nodes: Vec<IndexedNode>,
    /// Contains edges, then calls and imports edges.
    pub edges: Vec<IndexedEdge>,
    /// Ids, paths and edge labels addressed by the spans above.
    pub text: TextBlob,
    /// Freshness mode requested for this generation.
    pub freshness: FreshnessMode,
    /// Injected clock at snapshot time, in milliseconds.
    pub taken_at: u64,
    /// Policy epoch at snapshot time.
    pub epoch: u64,
    /// Coverage label.
    pub coverage: String,
    /// Completion label.
    pub completion: String,
    /// Parser id.
    pub parser: String,
}
