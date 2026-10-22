//! File-local Rust facts: declarations, raw imports and raw call references.
//! Implements PARSERS-v1 `ParseResult` and the ARCHITECTURE-PROPOSAL §3 precision labels.
//! Pure data without paths or resolved targets; resolvers and indexers consume it.
use super::Refusal;

/// How a fact was obtained; consumers must never upgrade it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Precision {
    /// Read from a complete `syn` syntax tree with exact byte spans.
    Syntax,
    /// Recovered by the conservative line scan of a refused file; low precision.
    LineScan,
}

/// Extraction coverage of the whole file (PARSERS-v1 `coverage`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Coverage {
    /// Every fact comes from a complete syntax tree.
    Syntax,
    /// No syntax tree; only line-scan facts, if any.
    Opaque,
}

/// Completion status layered over coverage (PARSERS-v1 `completion`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Completion {
    /// All supported facts of the file were extracted.
    Complete,
    /// Facts are incomplete for the stated reason.
    Partial(Refusal),
}

/// Declaration families recognised in Rust source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DeclarationKind {
    /// Inline `mod name { ... }`; `mod name;` is a [`ImportKind::ModFile`] import.
    Module,
    /// `struct`.
    Struct,
    /// `union`.
    Union,
    /// `enum`.
    Enum,
    /// `trait`.
    Trait,
    /// `trait Name = Bounds;`.
    TraitAlias,
    /// `impl` block, named `Type::impl` or `Type::impl<Trait>`.
    Impl,
    /// Free or nested function, including foreign functions.
    Function,
    /// Function directly inside an `impl` or `trait`.
    Method,
    /// `const`, including associated constants.
    Const,
    /// `static`, including foreign statics.
    Static,
    /// `type` alias or associated type.
    TypeAlias,
    /// `macro_rules!` definition.
    Macro,
}

/// Byte span `[start, end)` in the input and its 1-based inclusive lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceSpan {
    /// First byte.
    pub start: usize,
    /// One past the last byte.
    pub end: usize,
    /// First and last line, 1-based inclusive.
    pub lines: [u32; 2],
}

/// The self type and optional trait of an `impl` block, as compact source text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Implementation {
    /// Self type, whitespace removed (`Vec<T>`).
    pub self_type: String,
    /// Implemented trait path, whitespace removed, if any.
    pub trait_path: Option<String>,
}

/// One declaration, in source pre-order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declaration {
    /// Declaration family.
    pub kind: DeclarationKind,
    /// Name qualified by its lexical scope with `::` (`tests::checks`).
    pub name: String,
    /// Index of the lexically enclosing declaration; `None` is the file root.
    pub parent: Option<usize>,
    /// Item bytes without outer attributes and doc comments.
    pub span: SourceSpan,
    /// Has a `pub` visibility, or is an item of a public trait.
    pub public: bool,
    /// Function marked `#[test]` or `#[<path>::test]`.
    pub test: bool,
    /// First non-empty line of the doc comments, if any.
    pub doc: Option<String>,
    /// Self type and trait, for [`DeclarationKind::Impl`] only.
    pub implementation: Option<Implementation>,
    /// How this fact was obtained.
    pub precision: Precision,
}

/// Source construct behind a raw import.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportKind {
    /// One leaf of a `use` tree.
    Use,
    /// Out-of-line module `mod name;`.
    ModFile,
    /// `extern crate name;`.
    ExternCrate,
}

/// An unresolved import as written; `self` leaves collapse onto their prefix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawImport {
    /// Source construct.
    pub kind: ImportKind,
    /// Flattened path (`crate::a::B`, `crate::a::*`).
    pub path: String,
    /// Local binding from `as`, if any.
    pub alias: Option<String>,
    /// Re-exported (`pub use`, `pub mod`, `pub extern crate`).
    pub public: bool,
    /// Lexical scope qualified with `::`; empty is the file root.
    pub scope: String,
    /// Bytes of this leaf.
    pub span: SourceSpan,
    /// How this fact was obtained.
    pub precision: Precision,
}

/// Syntactic shape of a call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceKind {
    /// `path(args)`, including tuple-struct constructors.
    Call,
    /// `receiver.method(args)`.
    MethodCall,
}

/// Why a reference cannot be named from syntax alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnresolvedReason {
    /// Target depends on the receiver's type.
    MethodReceiver,
    /// `<T as Trait>::f` style call.
    QualifiedSelf,
    /// Receiver text exceeded the name bound; only the method is kept.
    Truncated,
}

/// Edge confidence, independent of parser coverage. Syntax never yields `exact` calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confidence {
    /// A path the resolver can match by name and scope.
    Name,
    /// Must not become an edge without further evidence.
    Unresolved(UnresolvedReason),
}

/// An unresolved call site; repeated sites stay separate records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawReference {
    /// Call shape.
    pub kind: ReferenceKind,
    /// Index of the enclosing declaration; `None` is the file root.
    pub from: Option<usize>,
    /// Spelling: `a::b` for paths, `receiver.method` for method calls.
    pub target: String,
    /// Lexical scope qualified with `::`; empty is the file root.
    pub scope: String,
    /// The enclosing declaration is a test function.
    pub in_test: bool,
    /// Bytes of the whole call expression.
    pub span: SourceSpan,
    /// Edge confidence label.
    pub confidence: Confidence,
}

/// All facts extracted from one file, in source order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extraction {
    /// Whether the facts are complete.
    pub completion: Completion,
    /// Whether a syntax tree backed them.
    pub coverage: Coverage,
    /// First non-empty line of the file's inner doc comments (`//!`).
    pub module_doc: Option<String>,
    /// Declarations in pre-order.
    pub declarations: Vec<Declaration>,
    /// Raw imports in source order.
    pub imports: Vec<RawImport>,
    /// Raw call references in traversal order.
    pub references: Vec<RawReference>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_are_independent_axes() {
        let partial = Completion::Partial(Refusal::Syntax { line: 3 });
        assert_ne!(partial, Completion::Complete);
        assert_ne!(Coverage::Syntax, Coverage::Opaque);
        assert_ne!(
            Confidence::Name,
            Confidence::Unresolved(UnresolvedReason::MethodReceiver)
        );
        assert!(DeclarationKind::Module < DeclarationKind::Macro);
    }
}
