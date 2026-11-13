// SPDX-License-Identifier: Apache-2.0
//! Per-file facts in the dctx-map-parse shape, each carrying a precision label.
//! Implements PARSERS-v1 (completion, diagnostics) and SPEC-MAP-MODE 5.3 and 6.2.
//! Uses dctx-map-model nodes and edges; the walker builds these, callers consume them.
use dctx_map_model::{Edge, EdgeKind, Node, StableId};

/// Source dialect selected by the final path extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    /// `.js`, `.mjs`, `.cjs` and `.jsx`, with JSX.
    JavaScript,
    /// `.ts`, `.mts` and `.cts`, without JSX.
    TypeScript,
    /// `.tsx`: TypeScript with JSX.
    Tsx,
}

impl Dialect {
    /// Selects a dialect case-insensitively; other extensions are unsupported.
    /// This lexical helper does not validate path identities or enforce a size limit.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn for_path(path: &str) -> Option<Self> {
        let extension = path.rsplit_once('.')?.1.to_ascii_lowercase();
        match extension.as_str() {
            "js" | "mjs" | "cjs" | "jsx" => Some(Self::JavaScript),
            "ts" | "mts" | "cts" => Some(Self::TypeScript),
            "tsx" => Some(Self::Tsx),
            _ => None,
        }
    }

    /// Whether type annotations and declarations are recognised.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub const fn typed(self) -> bool {
        matches!(self, Self::TypeScript | Self::Tsx)
    }

    /// Whether `<` in expression position may open a JSX element.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub const fn jsx(self) -> bool {
        matches!(self, Self::JavaScript | Self::Tsx)
    }
}

/// How much syntactic evidence stands behind one fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Precision {
    /// A complete token shape inside balanced, terminated syntax.
    Structural,
    /// Recognised by convention: test blocks, `require` bindings, test edges.
    Convention,
    /// The fact reaches unterminated or unbalanced input; treat as a hint.
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
    /// Referenced spelling (module path, receiver.method or type name).
    pub name: String,
    /// Lexical scope qualified with `::`; empty means the file root.
    pub scope: String,
    /// Explicit local binding (`import { a as b }`, `const b = require(..)`).
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
    /// A quoted string reaches a line end or the end of input.
    UnterminatedString,
    /// A template literal has no closing backtick.
    UnterminatedTemplate,
    /// A JSX element is not closed before the end of input.
    UnterminatedJsx,
    /// A bracket has no partner, or closes a different bracket.
    UnbalancedBracket,
    /// A declaration name cannot form a stable id; the declaration is abstained.
    UnsupportedName,
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

/// Completion of an accepted file; refusals are [`crate::LexError`] instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Completion {
    /// Every token was understood.
    Complete,
    /// Some spans were unsupported; see the diagnostics and fact labels.
    Partial,
}

/// Structural facts for one file, in source traversal order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexedFile {
    /// Validated, place-relative file path.
    pub path: String,
    /// Dialect used for extraction.
    pub dialect: Dialect,
    /// Whether all input was understood.
    pub completion: Completion,
    /// At most [`LexedFile::MAX_DIAGNOSTICS`] problems, in source order.
    pub diagnostics: Vec<Diagnostic>,
    /// Module, symbols and tests.
    pub nodes: Vec<Labeled<Node>>,
    /// Containment relations owned by this file.
    pub edges: Vec<Labeled<Edge>>,
    /// Unresolved references; repeated sites remain separate records.
    pub references: Vec<Labeled<RawReference>>,
}

impl LexedFile {
    /// Bound on retained diagnostics; later problems still mark the file partial.
    pub const MAX_DIAGNOSTICS: usize = 64;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialects_follow_the_final_extension() {
        assert_eq!(Dialect::for_path("a/b.MJS"), Some(Dialect::JavaScript));
        assert_eq!(Dialect::for_path("view.jsx"), Some(Dialect::JavaScript));
        assert_eq!(Dialect::for_path("types.d.ts"), Some(Dialect::TypeScript));
        assert_eq!(Dialect::for_path("view.tsx"), Some(Dialect::Tsx));
        assert_eq!(Dialect::for_path("notes.md"), None);
        assert_eq!(Dialect::for_path("Makefile"), None);
        assert!(Dialect::Tsx.typed() && Dialect::Tsx.jsx());
        assert!(!Dialect::TypeScript.jsx() && !Dialect::JavaScript.typed());
    }
}
