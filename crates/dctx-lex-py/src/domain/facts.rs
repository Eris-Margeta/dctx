//! Per-file facts in the dctx-map-parse shape, each carrying a precision label.
//! Implements PARSERS-v1 (completion, diagnostics) and SPEC-MAP-MODE 5.3 and 6.2.
//! Uses dctx-map-model nodes and edges; the walker builds these, callers consume them.
use dctx_map_model::{Edge, EdgeKind, Node, StableId};

/// Source dialect selected by the final path extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    /// `.py` and `.pyw`.
    Python,
    /// `.pyi` stub.
    Stub,
}

impl Dialect {
    /// Selects a dialect case-insensitively; other extensions are unsupported.
    pub fn for_path(path: &str) -> Option<Self> {
        let file = path.rsplit(['/', '\\']).next().unwrap_or(path);
        let ext = file.rsplit_once('.')?.1.to_ascii_lowercase();
        match ext.as_str() {
            "py" | "pyw" => Some(Self::Python),
            "pyi" => Some(Self::Stub),
            _ => None,
        }
    }
}

/// How much syntactic evidence stands behind one fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Precision {
    /// A complete dotted name or declaration in balanced, terminated syntax.
    Structural,
    /// A module-level constant recognised by the `ALL_CAPS` convention.
    Convention,
    /// The fact ends after the first damaged span; treat it as a hint.
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
    /// `imports` or `calls`. Never `contains`.
    pub kind: EdgeKind,
    /// Referenced spelling (`pkg.mod`, `pkg::name`, `obj.method`).
    pub name: String,
    /// Lexical scope qualified with `::`; empty means the file root.
    pub scope: String,
    /// `import x as y` / `from m import x as y`.
    pub alias: Option<String>,
    /// One-based inclusive lines of the reference site.
    pub lines: [u32; 2],
    /// File whose replacement invalidates this reference.
    pub owner_path: String,
}

/// Why a span could not be extracted with full structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticReason {
    /// A quote that never closes, including an open f-string field.
    UnterminatedString,
    /// A bracket has no partner, or closes a different bracket.
    UnbalancedBracket,
    /// A dedent lands between indent levels.
    InconsistentIndent,
    /// Tabs and spaces disagree about indent versus dedent.
    MixedTabs,
    /// A statement is not a form this extractor understands.
    UnexpectedToken,
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

/// Completion of an accepted file; refusals are [`super::LexError`] instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Completion {
    /// Every token was understood.
    Complete,
    /// Some spans were damaged; see the diagnostics and fact labels.
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
    /// Module, symbols and constants.
    pub nodes: Vec<Labeled<Node>>,
    /// Containment relations owned by this file.
    pub edges: Vec<Labeled<Edge>>,
    /// Unresolved imports and calls; repeated sites remain separate records.
    pub references: Vec<Labeled<RawReference>>,
}

impl LexedFile {
    /// Bound on retained diagnostics; later problems still mark the file partial.
    pub const MAX_DIAGNOSTICS: usize = 64;
}
