// SPDX-License-Identifier: Apache-2.0
//! Per-file facts in the dctx-map-parse shape, each carrying a precision label.
//! Implements PARSERS-v1 (completion, diagnostics) and SPEC-MAP-MODE 5.3 and 6.2.
//! Uses dctx-map-model nodes and edges; the walker builds these, callers consume them.
use dctx_map_model::{Edge, EdgeKind, Node, StableId};

/// Source dialect selected by the final path extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    /// `.c` and `.h`, case-insensitively. The exact suffix `.C` is C++.
    C,
    /// C++ suffixes, case-insensitively, plus the exact suffix `.C`.
    Cpp,
}

impl Dialect {
    /// Selects C or C++ from the final extension. Other extensions are unsupported.
    ///
    /// GCC treats the exact suffix `.C` as C++. `.c` and `.h` stay C.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn for_path(path: &str) -> Option<Self> {
        let extension = path.rsplit_once('.')?.1;
        if extension == "C" {
            return Some(Self::Cpp);
        }
        if extension.eq_ignore_ascii_case("c") || extension.eq_ignore_ascii_case("h") {
            return Some(Self::C);
        }
        let lower = extension.to_ascii_lowercase();
        if matches!(
            lower.as_str(),
            "cc" | "cpp" | "cxx" | "c++" | "hpp" | "hh" | "hxx" | "ixx" | "cppm"
        ) {
            Some(Self::Cpp)
        } else {
            None
        }
    }
}

/// How much syntactic evidence stands behind one fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Precision {
    /// A complete token shape inside balanced, terminated syntax.
    Structural,
    /// A conditional branch, or a member read the model has no edge kind for.
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
    /// Referenced spelling (header, callee, member or type name).
    pub name: String,
    /// Lexical scope qualified with `::`; empty means the file root.
    pub scope: String,
    /// Explicit local binding. C and C++ includes have none.
    pub alias: Option<String>,
    /// One-based inclusive lines of the reference site.
    pub lines: [u32; 2],
    /// File whose replacement invalidates this reference.
    pub owner_path: String,
}

/// Why a span could not be extracted with full structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DiagnosticReason {
    /// A block comment has no closing `*/`.
    UnterminatedComment,
    /// A string literal is not closed.
    UnterminatedString,
    /// A character literal is not closed.
    UnterminatedChar,
    /// A raw string literal is not closed.
    UnterminatedRawString,
    /// A bracket has no partner, or closes a different bracket.
    UnbalancedBracket,
    /// An `#if` has no `#endif`, or an `#endif` has no opener.
    UnbalancedConditional,
    /// A declaration name cannot form a stable id; the declaration is abstained.
    UnsupportedName,
    /// A macro occupies the declaration or the include path, so the fact is abstained.
    HiddenByMacro,
    /// A declaration keyword was not followed by a recognizable shape.
    Malformed,
    /// A top-level span is not a recognised declaration.
    Unrecognized,
}

/// One bounded diagnostic with its location.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Diagnostic {
    /// One-based line of the problem.
    pub line: u32,
    /// Byte offset of the problem in the original file.
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
    /// Module and symbols.
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
    fn dialect_follows_the_final_extension() {
        assert_eq!(Dialect::for_path("a/b.c"), Some(Dialect::C));
        assert_eq!(Dialect::for_path("a/b.C"), Some(Dialect::Cpp));
        assert_eq!(Dialect::for_path("include/box.H"), Some(Dialect::C));
        assert_eq!(Dialect::for_path("src/box.cc"), Some(Dialect::Cpp));
        assert_eq!(Dialect::for_path("src/box.CPP"), Some(Dialect::Cpp));
        assert_eq!(Dialect::for_path("src/box.cxx"), Some(Dialect::Cpp));
        assert_eq!(Dialect::for_path("include/box.hpp"), Some(Dialect::Cpp));
        assert_eq!(Dialect::for_path("include/box.hh"), Some(Dialect::Cpp));
        assert_eq!(Dialect::for_path("include/Widget.hxx"), Some(Dialect::Cpp));
        assert_eq!(Dialect::for_path("unit.c++"), Some(Dialect::Cpp));
        assert_eq!(Dialect::for_path("mod.ixx"), Some(Dialect::Cpp));
        assert_eq!(Dialect::for_path("part.cppm"), Some(Dialect::Cpp));
        assert_eq!(Dialect::for_path("notes.md"), None);
        assert_eq!(Dialect::for_path("Makefile"), None);
        assert!(Precision::Partial > Precision::Convention);
        assert!(Precision::Convention > Precision::Structural);
    }
}
