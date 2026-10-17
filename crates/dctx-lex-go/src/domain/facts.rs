//! Per-file facts in the dctx-map-parse shape, each carrying a precision label.
//! Implements PARSERS-v1 (completion, diagnostics) and SPEC-MAP-MODE 5.3 and 6.2.
//! Uses dctx-map-model nodes and edges; the walker builds these, callers consume them.
use dctx_map_model::{Edge, EdgeKind, Node, StableId};

/// Source dialect selected by the final path extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    /// `.go`, case-insensitively.
    Go,
}

impl Dialect {
    /// Selects Go when the final extension is `go`.
    pub fn for_path(path: &str) -> Option<Self> {
        let extension = path.rsplit_once('.')?.1;
        extension.eq_ignore_ascii_case("go").then_some(Self::Go)
    }
}

/// How much syntactic evidence stands behind one fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Precision {
    /// A complete token shape inside balanced, terminated syntax.
    Structural,
    /// Recognised by a language convention rather than a unique production.
    Convention,
    /// The fact reaches a diagnostic; treat it as a hint.
    Partial,
}

/// A fact together with its precision label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Labeled<T> {
    /// The extracted fact.
    pub fact: T,
    /// Evidence behind the fact.
    pub precision: Precision,
}

/// A syntactic reference whose destination has not been resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawReference {
    /// Source node, including its duplicate ordinal when needed.
    pub src: StableId,
    /// Relation to resolve; never `contains`.
    pub kind: EdgeKind,
    /// Referenced spelling (import path, callee, selector or type name).
    pub name: String,
    /// Lexical scope qualified with `::`; empty means the file root.
    pub scope: String,
    /// Explicit local binding (`import alias "path"`, `.`, `_`).
    pub alias: Option<String>,
    /// One-based inclusive lines of the reference site.
    pub lines: [u32; 2],
    /// File whose replacement invalidates this reference.
    pub owner_path: String,
}

/// Why a span could not be extracted with full structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticReason {
    /// A block comment has no closing `*/`.
    UnterminatedComment,
    /// A string literal is not closed.
    UnterminatedString,
    /// A rune literal is not closed.
    UnterminatedRune,
    /// A bracket has no partner, or closes a different bracket.
    UnbalancedBracket,
    /// A declaration name cannot form a stable id; the declaration is abstained.
    UnsupportedName,
    /// A top-level span is not a recognised Go declaration.
    Unrecognized,
}

/// One bounded diagnostic with its location.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Diagnostic {
    /// One-based line of the problem.
    pub line: u32,
    /// Byte offset of the problem.
    pub byte: u32,
    /// Problem category.
    pub reason: DiagnosticReason,
}

/// Completion of an accepted file. Refusals are [`crate::LexError`] instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Completion {
    /// Every supported span was understood.
    Complete,
    /// Some spans were unsupported. See the diagnostics and fact labels.
    Partial,
}

/// Structural facts for one file, in source traversal order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexedFile {
    /// Validated, place-relative file path.
    pub path: String,
    /// Dialect used for extraction.
    pub dialect: Dialect,
    /// Whether all supported input was understood.
    pub completion: Completion,
    /// At most [`LexedFile::MAX_DIAGNOSTICS`] problems, in source order.
    pub diagnostics: Vec<Diagnostic>,
    /// Module, package clause and symbols.
    pub nodes: Vec<Labeled<Node>>,
    /// Containment relations owned by this file.
    pub edges: Vec<Labeled<Edge>>,
    /// Unresolved references. Repeated sites remain separate records.
    pub references: Vec<Labeled<RawReference>>,
}

impl LexedFile {
    /// Bound on retained diagnostics. Later problems still mark the file partial.
    pub const MAX_DIAGNOSTICS: usize = 64;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn go_dialect_follows_the_final_extension() {
        assert_eq!(Dialect::for_path("a/b.GO"), Some(Dialect::Go));
        assert_eq!(Dialect::for_path("cmd/main.go"), Some(Dialect::Go));
        assert_eq!(Dialect::for_path("notes.md"), None);
        assert_eq!(Dialect::for_path("Makefile"), None);
        assert!(Precision::Partial > Precision::Structural);
    }
}
