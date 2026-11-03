// SPDX-License-Identifier: Apache-2.0
//! Per-file facts in the dctx-map-parse shape, each carrying a precision label.
//! Implements PARSERS-v1 (completion, diagnostics) and SPEC-MAP-MODE 5.3 and 6.2.
//! Uses dctx-map-model nodes and edges; the walkers build these, callers consume them.
use dctx_map_model::{Edge, EdgeKind, Node, StableId};

/// Source dialect selected by the final path extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    /// `.json`, case-insensitively. Comments are diagnostics.
    Json,
    /// `.jsonc`, case-insensitively. Comments and trailing commas are accepted.
    Jsonc,
    /// `.toml`, case-insensitively.
    Toml,
    /// `.yaml` or `.yml`, case-insensitively.
    Yaml,
}

impl Dialect {
    /// Selects a dialect from the final extension. Other extensions are unsupported.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn for_path(path: &str) -> Option<Self> {
        let extension = path.rsplit_once('.')?.1;
        if extension.eq_ignore_ascii_case("json") {
            Some(Self::Json)
        } else if extension.eq_ignore_ascii_case("jsonc") {
            Some(Self::Jsonc)
        } else if extension.eq_ignore_ascii_case("toml") {
            Some(Self::Toml)
        } else if extension.eq_ignore_ascii_case("yaml") || extension.eq_ignore_ascii_case("yml") {
            Some(Self::Yaml)
        } else {
            None
        }
    }
}

/// How much syntactic evidence stands behind one fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Precision {
    /// A complete key inside balanced, terminated syntax.
    Structural,
    /// A dependency name, workspace member or path recognised by convention.
    Convention,
    /// The fact reaches a diagnostic. Treat it as a hint.
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
    /// Relation to resolve. Configuration references use `imports`.
    pub kind: EdgeKind,
    /// Referenced spelling: a path, a workspace member or a dependency name.
    pub name: String,
    /// Dotted table that contains the reference. Empty means the file root.
    pub scope: String,
    /// Unused for configuration references. Always absent.
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
    UnterminatedComment = 0,
    /// A quoted string is not closed.
    UnterminatedString = 1,
    /// A bracket, brace or flow collection is not closed.
    UnbalancedBracket = 2,
    /// A span is not valid configuration syntax.
    InvalidSyntax = 3,
    /// The same key is assigned again. The first assignment is kept.
    DuplicateKey = 4,
    /// A key cannot form a stable id. That key is abstained.
    UnsupportedName = 5,
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

/// Structural facts for one file, in source order.
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
    /// File node, then one symbol per table or key.
    pub nodes: Vec<Labeled<Node>>,
    /// Containment relations owned by this file.
    pub edges: Vec<Labeled<Edge>>,
    /// Unresolved cross-file references. Repeated sites remain separate records.
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
        assert_eq!(Dialect::for_path("a/b.JSON"), Some(Dialect::Json));
        assert_eq!(Dialect::for_path("cfg/app.JSONC"), Some(Dialect::Jsonc));
        assert_eq!(Dialect::for_path("Cargo.TOML"), Some(Dialect::Toml));
        assert_eq!(Dialect::for_path("chart.YML"), Some(Dialect::Yaml));
        assert_eq!(Dialect::for_path("chart.yaml"), Some(Dialect::Yaml));
        assert_eq!(Dialect::for_path("notes.md"), None);
        assert_eq!(Dialect::for_path("Makefile"), None);
        assert!(Precision::Partial > Precision::Structural);
        assert!(Precision::Convention > Precision::Structural);
    }
}
