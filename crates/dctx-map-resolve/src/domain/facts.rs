// SPDX-License-Identifier: Apache-2.0
//! Parser-neutral per-file definitions and raw references.
//! Implements SPEC-MAP-MODE sections 6.2 and 6.3 for CM-6 adapters.
//! Uses model nodes and identities; consumed by the pure resolver.
use dctx_map_model::{Edge, EdgeKind, Node, StableId};

/// Complete parser output for one file, independent of any grammar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts {
    /// Place-relative source path; owns every resulting edge and unresolved item.
    pub path: String,
    /// Package identity, normally the manifest directory; isolates bare names.
    pub package: String,
    /// Nodes and their canonical names within this package.
    pub definitions: Vec<Definition>,
    /// Syntactic references, including explicit containment relationships.
    pub references: Vec<Reference>,
}

/// A model node bound to its package-relative lexical name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Definition {
    /// Node, including its parser-supplied body fingerprint.
    pub node: Node,
    /// Canonical name such as `api::Client::send`; root module uses an empty name.
    /// Independent of `node.name`, which may be local or display-oriented.
    pub qualified_name: String,
}

/// A reference spelling or a parser-proven identity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Target {
    /// A fully known node identity; required for exact containment.
    Id(StableId),
    /// Rust-style lexical name (`crate::`, `self::`, `super::` supported).
    /// Documents may also name a place-relative source path.
    Name(String),
}

/// One syntactic reference; importing adapters expand grouped `use` trees.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Reference {
    /// Node whose text contains the reference, or the parent for containment.
    pub source: StableId,
    /// Package-relative lexical scope, empty at the crate root.
    pub scope: String,
    /// Relation requested by the parser.
    pub kind: EdgeKind,
    /// Raw name or known identity.
    pub target: Target,
    /// Local name introduced by imports/reexports; absent means the final segment.
    /// Glob imports use a name target ending in `::*` and no explicit binding.
    pub binding: Option<String>,
}

/// Why a syntactically valid reference could not produce one reliable edge.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum UnresolvedReason {
    /// No compatible definition was found, including unsupported spellings.
    Missing,
    /// Multiple compatible nodes remain; identities are sorted and deduplicated.
    Ambiguous(Vec<StableId>),
}

/// Lossless retry input, still owned by its producing file.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Unresolved {
    /// File whose reindex replaces this reference.
    pub owner_path: String,
    /// Original reference, preserving source, scope, spelling and import binding.
    pub reference: Reference,
    /// Missing or ambiguous resolution.
    pub reason: UnresolvedReason,
}

/// Deterministically sorted results for a complete facts snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Resolution {
    /// Unique edges, sorted by owner, source, relation, destination and confidence.
    pub edges: Vec<Edge>,
    /// Unique unresolved references in canonical order, ready for retry.
    pub unresolved: Vec<Unresolved>,
}

/// Invalid input contracts, distinct from ordinary unresolved source references.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ResolveError {
    /// Each source file must occur once per snapshot.
    #[error("duplicate facts for file {0}")]
    DuplicateFile(String),
    /// Stable identities must have one definition.
    #[error("duplicate node {0}")]
    DuplicateNode(StableId),
    /// A node's model path, identity or category disagrees with its facts.
    #[error("invalid node ownership or identity: {0}")]
    InvalidNode(StableId),
    /// Reference sources must exist in the supplied snapshot.
    #[error("unknown reference source: {0}")]
    UnknownSource(StableId),
    /// Non-containment references must come from their owning file.
    #[error("reference source belongs to another file: {0}")]
    ForeignSource(StableId),
    /// Definition names and scopes must be canonical package-relative names.
    #[error("noncanonical name or scope: {0}")]
    InvalidScope(String),
    /// Binding names belong only to imports/reexports and must be one segment.
    #[error("invalid import binding: {0}")]
    InvalidBinding(String),
    /// Containment must be proven by the parser, never guessed by spelling.
    #[error("containment requires an identity target")]
    UnprovenContainment,
    /// Two local symbols in one shared fact use the same name.
    #[error("duplicate local symbol {0}")]
    DuplicateSymbol(String),
    /// A retained fact key already stores a different payload.
    #[error("fact key retains a different payload")]
    ConflictingFact,
    /// The view placed a path whose fact key is not retained.
    #[error("no retained fact for {0}")]
    MissingFact(String),
    /// The shared fact names a local symbol that is not in its payload.
    #[error("shared fact is missing symbol {0}")]
    MissingSymbol(String),
    /// Root, namespace or revision is empty or has a git-shaped identity.
    #[error("invalid view identity: {0}")]
    InvalidRoot(String),
    /// A placement path is not a portable relative path.
    #[error("invalid placement path: {0}")]
    InvalidPath(String),
    /// Shared references carry names only; resolved ids belong to a view.
    #[error("shared reference carries a resolved identity")]
    LayoutDependentTarget,
}

pub(crate) fn canonical(value: &str) -> bool {
    value.is_empty()
        || value.split("::").all(|part| {
            !part.is_empty()
                && !matches!(part, "crate" | "self" | "super" | "*")
                && !part.contains([':', '/', '\\'])
                && !part.chars().any(|c| c.is_control() || c.is_whitespace())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_package_relative_and_scopes_can_be_root() {
        for name in ["", "api", "api::Client::send"] {
            assert!(canonical(name));
        }
        for name in ["crate::api", "::api", "api::", "self", "a/b", "a::*"] {
            assert!(!canonical(name));
        }
    }
}
