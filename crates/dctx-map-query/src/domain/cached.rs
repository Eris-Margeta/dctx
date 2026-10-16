//! Lossless, serializable per-file facts retained between index refreshes.
//! Implements SPEC-MAP-MODE section 7 where store retry metadata is incomplete.
//! Uses public parser/model types; query adapters persist these values.
use super::parse_status::{PARSER_REVISION, ParseStatus};
use dctx_map_model::{Edge, EdgeKind, Node, StableId};
use dctx_map_parse::{ParsedFile, RawReference};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CachedFile {
    pub hash: String,
    pub size: u64,
    pub mtime: i64,
    pub readable: bool,
    #[serde(default)]
    pub parser_revision: u32,
    #[serde(default)]
    pub status: ParseStatus,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub references: Vec<SavedReference>,
}
impl CachedFile {
    pub fn parsed(parsed: ParsedFile, hash: String, size: u64, mtime: i64, readable: bool) -> Self {
        Self {
            hash,
            size,
            mtime,
            readable,
            parser_revision: PARSER_REVISION,
            status: ParseStatus::Parsed,
            nodes: parsed.nodes,
            edges: parsed.edges,
            references: parsed
                .references
                .into_iter()
                .map(SavedReference::from)
                .collect(),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SavedReference {
    pub source: StableId,
    pub kind: EdgeKind,
    pub name: String,
    pub scope: String,
    pub binding: Option<String>,
}
impl From<RawReference> for SavedReference {
    fn from(raw: RawReference) -> Self {
        Self {
            source: raw.src,
            kind: raw.kind,
            name: raw.name,
            scope: raw.scope,
            binding: raw.alias,
        }
    }
}
