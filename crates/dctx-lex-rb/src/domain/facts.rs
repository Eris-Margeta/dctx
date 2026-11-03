// SPDX-License-Identifier: Apache-2.0
//! Per-file facts in the dctx-map-parse shape, each carrying a precision label.
//! Implements PARSERS-v1 (completion, diagnostics) and SPEC-MAP-MODE 5.3 and 6.2.
//! Uses dctx-map-model nodes and edges; the walker builds these, callers consume them.
use dctx_map_model::{Edge, EdgeKind, Node, StableId};

/// Source dialect selected by extension or a known Ruby basename.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    /// `.rb`, `.rake`, `.gemspec`, `Rakefile`, or `Gemfile`.
    Ruby,
}

impl Dialect {
    /// Selects Ruby from the path. Other files are unsupported.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn for_path(path: &str) -> Option<Self> {
        let base = path.rsplit(['/', '\\']).next().unwrap_or(path);
        if base.eq_ignore_ascii_case("Rakefile") || base.eq_ignore_ascii_case("Gemfile") {
            return Some(Self::Ruby);
        }
        let ext = base.rsplit_once('.')?.1;
        matches!(ext.to_ascii_lowercase().as_str(), "rb" | "rake" | "gemspec").then_some(Self::Ruby)
    }
}

/// How much syntactic evidence stands behind one fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Precision {
    /// A complete token shape inside balanced, terminated syntax.
    Structural,
    /// Recognised by a Ruby convention (`attr_*`, `alias`) rather than `def`.
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
    /// Referenced spelling (load path, callee, or member chain).
    pub name: String,
    /// Lexical scope qualified with `::`; empty means the file root.
    pub scope: String,
    /// Explicit local binding. Ruby load edges have none.
    pub alias: Option<String>,
    /// One-based inclusive lines of the reference site.
    pub lines: [u32; 2],
    /// File whose replacement invalidates this reference.
    pub owner_path: String,
}

/// Why a span could not be extracted with full structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticReason {
    /// `=begin` has no closing `=end`.
    UnterminatedComment,
    /// A string, heredoc, regex or `%`-literal is not closed.
    UnterminatedString,
    /// A bracket has no partner, or closes a different bracket.
    UnbalancedBracket,
    /// `class`, `module`, `def` or `do` has no `end`.
    UnterminatedBlock,
    /// A declaration name cannot form a stable id; the declaration is abstained.
    UnsupportedName,
    /// A declaration keyword was not followed by a recognizable shape.
    Malformed,
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
    /// Module node and symbols. The module owns every other node.
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
    fn ruby_dialect_follows_extension_and_basename() {
        assert_eq!(Dialect::for_path("lib/a.RB"), Some(Dialect::Ruby));
        assert_eq!(Dialect::for_path("tasks/x.rake"), Some(Dialect::Ruby));
        assert_eq!(Dialect::for_path("demo.gemspec"), Some(Dialect::Ruby));
        assert_eq!(Dialect::for_path("app/Rakefile"), Some(Dialect::Ruby));
        assert_eq!(Dialect::for_path("Gemfile"), Some(Dialect::Ruby));
        assert_eq!(Dialect::for_path("notes.md"), None);
        assert_eq!(Dialect::for_path("Gemfile.lock"), None);
        assert!(Precision::Structural < Precision::Convention);
        assert!(Precision::Convention < Precision::Partial);
    }
}
