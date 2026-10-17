//! Structural walker: scopes, fact emission and source order.
//! Implements SPEC-MAP-MODE 5.1–5.4 for Java with the same ownership rules as dctx-lex-js:
//! a declaration owns what is lexically inside it. Callers use [`extract`].
//! The token cursor and draft assembly live in sibling modules.
mod declarations;
mod declarations_generics;
mod declarations_members;
mod declarations_modifiers;
mod declarations_types;
mod mod_cursor;
mod mod_helpers;
mod references;

pub(crate) use mod_helpers::{is_modifier_word, is_non_callee, is_primitive};

use crate::domain::{DiagnosticReason, LexError, LexedFile, Limits, Precision};
use crate::lexer::{Lexed, Token};
use dctx_map_model::{EdgeKind, Node, NodeKind, SymbolKind};

/// Extracts facts from an already-lexed Java file.
pub(crate) fn extract(
    path: &str,
    source: &str,
    lexed: &Lexed,
    limits: &Limits,
) -> Result<LexedFile, LexError> {
    let draft = Draft::new(path, source, lexed, limits.max_facts)?;
    let mut walker = Walker {
        src: source,
        toks: &lexed.tokens,
        partner: &lexed.partner,
        comments: &lexed.comments,
        line_starts: &lexed.line_starts,
        draft,
        depth: 0,
        max_depth: limits.max_depth,
        refused: None,
    };
    walker.compilation_unit();
    if let Some(error) = walker.refused.take() {
        return Err(error);
    }
    walker.draft.finish(lexed)
}

/// Kind of type declaration the prefix just reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeclKind {
    /// `class`
    Class,
    /// `interface`
    Interface,
    /// `enum`
    Enum,
    /// `record`
    Record,
    /// `@interface`
    Annotation,
}

/// Enclosing declaration kind. Methods keep the enclosing type's kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScopeKind {
    /// File root.
    File,
    /// Class or anonymous class.
    Class,
    /// Interface or annotation type.
    Interface,
    /// Enum.
    Enum,
    /// Record.
    Record,
}

/// Lexical owner of nested facts. `name` is `::`-qualified and empty at the root.
#[derive(Debug, Clone)]
pub(crate) struct Scope {
    /// Index into the draft node list.
    pub owner: usize,
    /// Qualified name, empty at the file root.
    pub name: String,
    /// Simple name of the enclosing type, used to recognise constructors.
    pub simple: String,
    /// Type parameters visible in this scope.
    pub type_params: Vec<String>,
    /// Enclosing type kind.
    pub kind: ScopeKind,
    /// Inside a `@Test` method.
    pub in_test: bool,
}

impl Scope {
    fn file() -> Self {
        Self {
            owner: 0,
            name: String::new(),
            simple: String::new(),
            type_params: Vec::new(),
            kind: ScopeKind::File,
            in_test: false,
        }
    }
}

/// Modifiers and annotations in front of a declaration.
pub(crate) struct Prefix {
    /// First token that is not a modifier or annotation.
    pub next: usize,
    /// `public` was present.
    pub public: bool,
    /// `private` was present.
    pub private: bool,
    /// Annotations in source order.
    pub annotations: Vec<Ann>,
}

/// One annotation, including an optional argument list.
pub(crate) struct Ann {
    /// Qualified annotation type.
    pub name: String,
    /// Token index of `@`.
    pub at: usize,
    /// Token index of `(` when arguments are present.
    pub args: Option<usize>,
}

/// Fields describing one symbol, borrowed from the caller for the duration of [`Walker::emit`].
pub(crate) struct SymbolDraft<'a> {
    /// Graph category.
    pub node_kind: NodeKind,
    /// Parsed symbol category.
    pub symbol: SymbolKind,
    /// Unqualified name appended to the parent.
    pub local: &'a str,
    /// Simple name stored on the new scope.
    pub simple: &'a str,
    /// Scope kind stored on the new scope.
    pub scope_kind: ScopeKind,
    /// Type parameters introduced by this declaration.
    pub type_params: Vec<String>,
    /// First token of the declaration, annotations included.
    pub first: usize,
    /// Last token of the declaration.
    pub last: usize,
    /// Exclusive byte where the signature ends.
    pub sig_end: u32,
    /// Body byte span used for the fingerprint.
    pub body: (u32, u32),
    /// Public visibility.
    pub is_public: bool,
    /// Precision before partial downgrade.
    pub precision: Precision,
}

struct DraftNode {
    node: Node,
    parent: usize,
    start: u32,
    end: u32,
    precision: Precision,
}

struct DraftRef {
    src: usize,
    kind: EdgeKind,
    name: String,
    scope: String,
    alias: Option<String>,
    lines: [u32; 2],
    start: u32,
    end: u32,
    precision: Precision,
}

struct Draft {
    path: String,
    nodes: Vec<DraftNode>,
    refs: Vec<DraftRef>,
    problems: Vec<(u32, DiagnosticReason)>,
    max_facts: usize,
    overflow: bool,
}

pub(crate) struct Walker<'a> {
    src: &'a str,
    toks: &'a [Token],
    partner: &'a [u32],
    comments: &'a [crate::lexer::Comment],
    line_starts: &'a [u32],
    draft: Draft,
    depth: u32,
    max_depth: u32,
    refused: Option<LexError>,
}

#[cfg(test)]
mod tests {
    use crate::{Completion, JavaLexer, Precision};

    #[test]
    fn package_class_and_call_survive_a_string() {
        let source = "package p; class A { int x; void m() { run(); } String s = \"class B { }\"; }";
        let facts = JavaLexer::default()
            .extract("A.java", source.as_bytes())
            .expect("extract");
        assert_eq!(facts.completion, Completion::Complete);
        assert_eq!(facts.nodes[0].fact.signature, "p");
        let names: Vec<_> = facts.nodes.iter().skip(1).map(|n| n.fact.name.as_str()).collect();
        assert_eq!(names, ["A", "A::x", "A::m", "A::s"]);
        assert_eq!(facts.nodes[1].precision, Precision::Structural);
        let calls: Vec<_> = facts
            .references
            .iter()
            .filter(|reference| reference.fact.kind.as_str() == "calls")
            .map(|reference| reference.fact.name.as_str())
            .collect();
        assert_eq!(calls, ["run"]);
        assert!(!facts.nodes.iter().any(|n| n.fact.name.contains('B')));
    }
}
