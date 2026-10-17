//! Structural walker: scopes, fact emission and source order.
//! Implements SPEC-MAP-MODE 5.1–5.4 for C#. A declaration owns what is lexically inside it.
//! The token cursor, type grammar and reference scan live in sibling modules.
mod declarations;
mod declarations_types;
mod members;
mod references;

use crate::domain::{
    body_fingerprint, slice, Completion, Diagnostic, DiagnosticReason, Dialect, Labeled, LexError,
    LexedFile, Limits, Precision, RawReference, normalize,
};
use crate::lexer::{Lexed, Token};
use dctx_map_model::{
    Confidence, Edge, EdgeKind, Node, NodeKind, StableId, Summary, SummarySource, SymbolKind,
};
use std::collections::BTreeMap;

/// Extracts facts from an already-lexed C# file.
pub(crate) fn extract(
    path: &str,
    source: &str,
    lexed: &Lexed,
    limits: &Limits,
) -> Result<LexedFile, LexError> {
    let file_end = if source.is_empty() {
        1
    } else {
        lexed.line((source.len() as u32).saturating_sub(1)).max(1)
    };
    let mut walker = Walker {
        src: source,
        toks: &lexed.tokens,
        partner: &lexed.partner,
        comments: &lexed.comments,
        line_starts: &lexed.line_starts,
        draft: Draft::new(path, source, file_end, limits.max_facts)?,
        file_end,
    };
    walker.apply_file_doc();
    walker.body(0, walker.toks.len(), &Scope::root(), Body::Namespace);
    walker.draft.finish(lexed, file_end)
}

/// Where a brace-delimited region is allowed to declare names.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Body {
    /// File or namespace: usings, namespaces, types and loose statements.
    Namespace,
    /// Type: nested types and members.
    Type,
}

/// Enclosing declaration kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScopeKind {
    /// File root.
    File,
    /// `namespace` block or file-scoped namespace.
    Namespace,
    /// `class`
    Class,
    /// `struct` or `record struct`
    Struct,
    /// `interface`
    Interface,
    /// `enum`
    Enum,
    /// `record` or `record class`
    Record,
    /// Method, constructor, field, property or enum constant.
    Member,
}

/// Kind of type declaration the prefix just reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeclKind {
    /// `class`
    Class,
    /// `struct`
    Struct,
    /// `interface`
    Interface,
    /// `enum`
    Enum,
    /// `record`
    Record,
}

/// Lexical owner of nested facts. `name` is `::`-qualified and empty at the root.
#[derive(Debug, Clone)]
pub(crate) struct Scope {
    /// Index into the draft node list.
    pub owner: usize,
    /// Qualified name. Empty at the file root.
    pub name: String,
    /// Simple name of the enclosing type, used to recognise constructors.
    pub simple: String,
    /// Enclosing kind.
    pub kind: ScopeKind,
}

impl Scope {
    fn root() -> Self {
        Self {
            owner: 0,
            name: String::new(),
            simple: String::new(),
            kind: ScopeKind::File,
        }
    }
}

/// One symbol about to be emitted. `local` is the unqualified name.
pub(crate) struct Emit<'a> {
    /// Parsed symbol category. Namespaces have none.
    pub symbol: Option<SymbolKind>,
    /// Unqualified name appended to the parent.
    pub local: &'a str,
    /// Simple name stored on the new scope.
    pub simple: &'a str,
    /// Scope kind stored on the new scope.
    pub scope_kind: ScopeKind,
    /// First token of the declaration, attributes included.
    pub first: usize,
    /// Last token of the declaration.
    pub last: usize,
    /// Exclusive byte where the signature ends.
    pub sig_end: u32,
    /// Body byte span used for the fingerprint.
    pub body: (u32, u32),
    /// Public visibility.
    pub is_public: bool,
    /// When set, the first such name becomes the module signature.
    pub namespace: bool,
}

/// Modifiers and attribute names in front of a declaration.
pub(crate) struct Prefix {
    /// First token that is not a modifier or attribute.
    pub next: usize,
    /// `public` was present.
    pub public: bool,
    /// `private` was present.
    pub private: bool,
    /// Attribute type token ranges, exclusive end.
    pub attrs: Vec<(usize, usize)>,
}

struct DraftNode {
    node: Node,
    parent: usize,
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
    file_end: u32,
}

impl Draft {
    fn new(path: &str, source: &str, file_end: u32, max_facts: usize) -> Result<Self, LexError> {
        let id = StableId::new(NodeKind::Module, path, None, None)?;
        let module = Node {
            id,
            kind: NodeKind::Module,
            symbol_kind: None,
            path: path.to_string(),
            lines: [1, file_end.max(1)],
            name: path.to_string(),
            signature: String::new(),
            fingerprint: body_fingerprint(source),
            public: false,
            summary: Summary {
                text: String::new(),
                source: SummarySource::Signature,
            },
        };
        Ok(Self {
            path: path.to_string(),
            nodes: vec![DraftNode {
                node: module,
                parent: 0,
                end: u32::MAX,
                precision: Precision::Structural,
            }],
            refs: Vec::new(),
            problems: Vec::new(),
            max_facts,
            overflow: false,
        })
    }

    fn full(&mut self) -> bool {
        if self.nodes.len() + self.refs.len() >= self.max_facts {
            self.overflow = true;
            true
        } else {
            false
        }
    }

    fn finish(mut self, lexed: &Lexed, file_end: u32) -> Result<LexedFile, LexError> {
        if self.overflow || self.nodes.len() + self.refs.len() > self.max_facts {
            return Err(LexError::TooManyFacts { limit: self.max_facts });
        }
        let module = &mut self.nodes[0].node;
        if module.signature.is_empty() {
            module.signature = module.name.clone();
        }
        if module.summary.text.is_empty() {
            module.summary.text = module.signature.clone();
            module.summary.source = SummarySource::Signature;
        }
        module.lines = [1, file_end.max(1)];
        assign_ordinals(&mut self.nodes)?;
        let mut diagnostics = lexed.diagnostics.clone();
        diagnostics.extend(self.problems.iter().map(|&(byte, reason)| Diagnostic {
            line: lexed.line(byte).max(1),
            byte,
            reason,
        }));
        diagnostics.sort_by_key(|diagnostic| (diagnostic.byte, diagnostic.reason as u8));
        let first = diagnostics.first().map(|diagnostic| diagnostic.byte).unwrap_or(u32::MAX);
        let completion = if diagnostics.is_empty() {
            Completion::Complete
        } else {
            Completion::Partial
        };
        diagnostics.truncate(LexedFile::MAX_DIAGNOSTICS);
        let path = self.path.clone();
        let ids: Vec<StableId> = self.nodes.iter().map(|node| node.node.id.clone()).collect();
        let edges = self
            .nodes
            .iter()
            .enumerate()
            .skip(1)
            .map(|(index, child)| Labeled {
                fact: Edge {
                    src: ids[child.parent].clone(),
                    dst: ids[index].clone(),
                    kind: EdgeKind::Contains,
                    conf: Confidence::Exact,
                    owner_path: path.clone(),
                },
                precision: partial_precision(child.end, child.precision, first),
            })
            .collect();
        let references = self
            .refs
            .into_iter()
            .filter(|reference| reference.src < ids.len())
            .map(|reference| Labeled {
                precision: partial_precision(reference.end, reference.precision, first),
                fact: RawReference {
                    src: ids[reference.src].clone(),
                    kind: reference.kind,
                    name: reference.name,
                    scope: reference.scope,
                    alias: reference.alias,
                    lines: reference.lines,
                    owner_path: path.clone(),
                },
            })
            .collect();
        let nodes = self
            .nodes
            .into_iter()
            .map(|draft| Labeled {
                precision: partial_precision(draft.end, draft.precision, first),
                fact: draft.node,
            })
            .collect();
        Ok(LexedFile {
            path,
            dialect: Dialect::CSharp,
            completion,
            diagnostics,
            nodes,
            edges,
            references,
        })
    }
}

fn assign_ordinals(nodes: &mut [DraftNode]) -> Result<(), LexError> {
    let mut total = BTreeMap::<(NodeKind, String), u32>::new();
    for draft in nodes.iter().skip(1).filter(|draft| named(draft.node.kind)) {
        *total.entry((draft.node.kind, draft.node.name.clone())).or_insert(0) += 1;
    }
    let mut seen = BTreeMap::<(NodeKind, String), u32>::new();
    for draft in nodes.iter_mut().skip(1).filter(|draft| named(draft.node.kind)) {
        let key = (draft.node.kind, draft.node.name.clone());
        let count = total[&key];
        let next = seen.entry(key).or_insert(0);
        *next += 1;
        let ordinal = (count > 1).then_some(*next);
        draft.node.id = StableId::new(draft.node.kind, &draft.node.path, Some(&draft.node.name), ordinal)?;
    }
    Ok(())
}

fn named(kind: NodeKind) -> bool {
    matches!(kind, NodeKind::Symbol | NodeKind::Test | NodeKind::Doc)
}

fn partial_precision(end: u32, base: Precision, first: u32) -> Precision {
    if end != u32::MAX && end > first {
        Precision::Partial
    } else {
        base
    }
}

impl<'a> Walker<'a> {
    /// Emits a declaration and returns the scope its contents belong to.
    pub(crate) fn emit(&mut self, item: &Emit<'_>, parent: &Scope) -> Option<Scope> {
        if self.draft.full() {
            return None;
        }
        let local = item.local.trim_start_matches('@');
        if local.is_empty() || local.contains(['~', '#']) {
            self.problem(self.start_byte(item.first), DiagnosticReason::UnsupportedName);
            return None;
        }
        let parent_name = &parent.name;
        let name = if parent_name.is_empty() {
            local.to_string()
        } else {
            format!("{parent_name}::{local}")
        };
        let path = self.draft.path.clone();
        let Ok(id) = StableId::new(NodeKind::Symbol, &path, Some(&name), None) else {
            self.problem(self.start_byte(item.first), DiagnosticReason::UnsupportedName);
            return None;
        };
        let start = self.start_byte(item.first);
        let end = self.end_byte(item.last).max(start);
        let sig_end = item.sig_end.clamp(start, end);
        let signature = normalize(slice(self.src, start, sig_end));
        let summary = match self.doc_before(item.first) {
            Some(text) => Summary { text, source: SummarySource::Doc },
            None => Summary {
                text: if signature.is_empty() { name.clone() } else { signature.clone() },
                source: SummarySource::Signature,
            },
        };
        let lines = self.span_lines(start, end);
        let node = Node {
            id,
            kind: NodeKind::Symbol,
            symbol_kind: item.symbol,
            path,
            lines,
            name: name.clone(),
            signature,
            fingerprint: body_fingerprint(slice(self.src, item.body.0, item.body.1)),
            public: item.is_public,
            summary,
        };
        self.draft.nodes.push(DraftNode {
            node,
            parent: parent.owner,
            end,
            precision: Precision::Structural,
        });
        let owner = self.draft.nodes.len() - 1;
        if item.namespace {
            self.note_namespace(local);
        }
        Some(Scope {
            owner,
            name,
            simple: item.simple.to_string(),
            kind: item.scope_kind,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::CsLexer;

    #[test]
    fn duplicate_methods_receive_ordinals_and_the_module_stays_structural() {
        let facts = CsLexer::default()
            .extract("p.cs", b"class A { void M() {} void M() {} }\n")
            .expect("extract");
        let ids: Vec<_> = facts.nodes.iter().map(|node| node.fact.id.as_str()).collect();
        assert!(ids.contains(&"s:p.cs#A::M~1"), "{ids:?}");
        assert!(ids.contains(&"s:p.cs#A::M~2"), "{ids:?}");
        assert_eq!(facts.edges.len(), facts.nodes.len() - 1);
        assert_eq!(facts.nodes[0].precision, crate::Precision::Structural);
    }
}
