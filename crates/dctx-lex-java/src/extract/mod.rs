//! Structural walker: scopes, fact emission and source order.
//! Implements SPEC-MAP-MODE 5.1–5.4 for Java with the same ownership rules as dctx-lex-js:
//! a declaration owns what is lexically inside it. Callers use [`extract`].
mod declarations;
mod references;

use crate::domain::{
    body_fingerprint, javadoc_summary, normalize, Completion, Diagnostic, DiagnosticReason,
    Dialect, Labeled, LexError, LexedFile, Limits, Precision, RawReference,
};
use crate::lexer::{line_of, Kind, Lexed, Token, NONE};
use dctx_map_model::{
    Confidence, Edge, EdgeKind, Node, NodeKind, StableId, Summary, SummarySource, SymbolKind,
};
use std::cmp::Reverse;
use std::collections::BTreeMap;

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

impl Draft {
    fn new(path: &str, source: &str, lexed: &Lexed, max_facts: usize) -> Result<Self, LexError> {
        let id = StableId::new(NodeKind::Module, path, None, None)?;
        let lines = source.lines().count().max(1) as u32;
        let summary = match leading_javadoc(source, lexed) {
            Some(text) => Summary {
                text,
                source: SummarySource::Doc,
            },
            None => Summary {
                text: String::new(),
                source: SummarySource::Signature,
            },
        };
        let module = Node {
            id,
            kind: NodeKind::Module,
            symbol_kind: None,
            path: path.into(),
            lines: [1, lines],
            name: path.into(),
            signature: String::new(),
            fingerprint: body_fingerprint(source),
            public: false,
            summary,
        };
        Ok(Self {
            path: path.into(),
            nodes: vec![DraftNode {
                node: module,
                parent: 0,
                start: 0,
                end: u32::MAX,
                precision: Precision::Structural,
            }],
            refs: Vec::new(),
            problems: Vec::new(),
            max_facts,
            overflow: max_facts == 0,
        })
    }

    fn set_package(&mut self, name: &str) {
        if let Some(module) = self.nodes.first_mut() {
            module.node.signature = name.to_string();
            if module.node.summary.source == SummarySource::Signature {
                module.node.summary.text = name.to_string();
            }
        }
    }

    fn set_fingerprint(&mut self, index: usize, body: &str) {
        if let Some(node) = self.nodes.get_mut(index) {
            node.node.fingerprint = body_fingerprint(body);
        }
    }

    /// Returns false when another fact would pass the limit. The file is then refused.
    fn accept(&mut self) -> bool {
        if self.overflow || self.nodes.len() + self.refs.len() >= self.max_facts {
            self.overflow = true;
            false
        } else {
            true
        }
    }

    fn finish(mut self, lexed: &Lexed) -> Result<LexedFile, LexError> {
        if self.overflow {
            return Err(LexError::TooManyFacts {
                limit: self.max_facts,
            });
        }
        let mut order: Vec<usize> = (1..self.nodes.len()).collect();
        order.sort_by_key(|&index| (self.nodes[index].start, Reverse(self.nodes[index].end)));
        let mut remap = vec![0; self.nodes.len()];
        for (position, &old) in order.iter().enumerate() {
            remap[old] = position + 1;
        }
        let mut slots: Vec<Option<DraftNode>> = self.nodes.drain(..).map(Some).collect();
        let mut nodes = Vec::with_capacity(slots.len());
        nodes.extend(slots[0].take());
        for &old in &order {
            nodes.extend(slots[old].take());
        }
        for node in nodes.iter_mut().skip(1) {
            node.parent = remap[node.parent];
        }
        assign_ordinals(&mut nodes)?;
        self.refs.sort_by_key(|reference| (reference.start, Reverse(reference.end)));
        let mut diagnostics = lexed.diagnostics.clone();
        diagnostics.extend(self.problems.iter().map(|&(byte, reason)| Diagnostic {
            line: lexed.line(byte),
            byte,
            reason,
        }));
        diagnostics.sort_by_key(|diagnostic| diagnostic.byte);
        let first_problem = diagnostics.first().map(|d| d.byte).unwrap_or(u32::MAX);
        let completion = if diagnostics.is_empty() {
            Completion::Complete
        } else {
            Completion::Partial
        };
        diagnostics.truncate(LexedFile::MAX_DIAGNOSTICS);
        let path = self.path.clone();
        let label = |end: u32, base: Precision| {
            if end > first_problem {
                Precision::Partial
            } else {
                base
            }
        };
        let edges = nodes
            .iter()
            .skip(1)
            .map(|child| Labeled {
                fact: Edge {
                    src: nodes[child.parent].node.id.clone(),
                    dst: child.node.id.clone(),
                    kind: EdgeKind::Contains,
                    conf: Confidence::Exact,
                    owner_path: path.clone(),
                },
                precision: label(child.end, child.precision),
            })
            .collect();
        let references = self
            .refs
            .into_iter()
            .map(|reference| Labeled {
                precision: label(reference.end, reference.precision),
                fact: RawReference {
                    src: nodes[remap[reference.src]].node.id.clone(),
                    kind: reference.kind,
                    name: reference.name,
                    scope: reference.scope,
                    alias: reference.alias,
                    lines: reference.lines,
                    owner_path: path.clone(),
                },
            })
            .collect();
        let nodes = nodes
            .into_iter()
            .map(|node| Labeled {
                precision: if node.end == u32::MAX {
                    node.precision
                } else {
                    label(node.end, node.precision)
                },
                fact: node.node,
            })
            .collect();
        Ok(LexedFile {
            path,
            dialect: Dialect::Java,
            completion,
            diagnostics,
            nodes,
            edges,
            references,
        })
    }
}

fn assign_ordinals(nodes: &mut [DraftNode]) -> Result<(), LexError> {
    let mut counts = BTreeMap::<(NodeKind, String), u32>::new();
    for draft in nodes.iter().skip(1) {
        *counts
            .entry((draft.node.kind, draft.node.name.clone()))
            .or_insert(0) += 1;
    }
    let mut ordinals = BTreeMap::<(NodeKind, String), u32>::new();
    for draft in nodes.iter_mut().skip(1) {
        let node = &mut draft.node;
        let key = (node.kind, node.name.clone());
        let next = ordinals.entry(key.clone()).or_insert(0);
        *next += 1;
        let ordinal = (counts[&key] > 1).then_some(*next);
        node.id = StableId::new(node.kind, &node.path, Some(&node.name), ordinal)?;
    }
    Ok(())
}

fn leading_javadoc(source: &str, lexed: &Lexed) -> Option<String> {
    let comment = lexed.comments.iter().find(|comment| comment.javadoc)?;
    let before = source
        .get(..comment.start as usize)?
        .trim_start_matches('\u{feff}');
    if !before.chars().all(char::is_whitespace) {
        return None;
    }
    let text = source.get(comment.start as usize..comment.end as usize)?;
    javadoc_summary(text)
}

fn valid_name(name: &str) -> bool {
    !name.is_empty() && !name.contains(['#', '~']) && !name.chars().any(char::is_control)
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

impl<'a> Walker<'a> {
    fn enter(&mut self, at: usize) -> bool {
        if self.depth >= self.max_depth {
            self.refuse_depth(at);
            return false;
        }
        self.depth += 1;
        true
    }

    fn leave(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    pub(crate) fn refuse_depth(&mut self, at: usize) {
        if self.refused.is_none() {
            self.refused = Some(LexError::TooDeep {
                line: self.line_of_token(at),
                limit: self.max_depth,
            });
        }
    }

    fn ok(&self) -> bool {
        self.refused.is_none()
    }

    pub(crate) fn kind(&self, index: usize) -> Option<Kind> {
        self.toks.get(index).map(|token| token.kind)
    }

    pub(crate) fn text(&self, index: usize) -> &'a str {
        let Some(token) = self.toks.get(index) else {
            return "";
        };
        self.src
            .get(token.start as usize..token.end as usize)
            .unwrap_or("")
    }

    pub(crate) fn word(&self, index: usize) -> bool {
        self.kind(index) == Some(Kind::Word)
    }

    pub(crate) fn is_word(&self, index: usize, spelling: &str) -> bool {
        self.word(index) && self.text(index) == spelling
    }

    pub(crate) fn is_punct(&self, index: usize, spelling: &str) -> bool {
        self.kind(index) == Some(Kind::Punct) && self.text(index) == spelling
    }

    pub(crate) fn close(&self, index: usize) -> Option<usize> {
        let partner = *self.partner.get(index)?;
        if partner == NONE {
            return None;
        }
        let partner = partner as usize;
        (partner > index).then_some(partner)
    }

    pub(crate) fn start_byte(&self, index: usize) -> u32 {
        self.toks
            .get(index)
            .map(|token| token.start)
            .unwrap_or_else(|| self.src.len() as u32)
    }

    pub(crate) fn end_byte(&self, index: usize) -> u32 {
        self.toks
            .get(index)
            .map(|token| token.end)
            .unwrap_or_else(|| self.src.len() as u32)
    }

    fn line_of_byte(&self, byte: u32) -> u32 {
        line_of(self.line_starts, byte)
    }

    pub(crate) fn line_of_token(&self, index: usize) -> u32 {
        self.line_of_byte(self.start_byte(index))
    }

    fn src_span(&self, start: u32, end: u32) -> &str {
        if end < start {
            return "";
        }
        self.src.get(start as usize..end as usize).unwrap_or("")
    }

    pub(crate) fn qualified(&self, from: usize, to: usize) -> String {
        if to < from {
            return String::new();
        }
        self.src_span(self.start_byte(from), self.end_byte(to))
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect()
    }

    pub(crate) fn signature(&self, first: usize, sig_end: u32) -> String {
        normalize(self.src_span(self.start_byte(first), sig_end))
    }

    fn doc_text(&self, token: usize) -> Option<String> {
        let doc = self.toks.get(token)?.doc;
        if doc == 0 {
            return None;
        }
        let comment = self.comments.get(doc as usize - 1)?;
        if !comment.javadoc {
            return None;
        }
        let text = self.src.get(comment.start as usize..comment.end as usize)?;
        javadoc_summary(text)
    }

    fn comment_start(&self, token: usize) -> Option<u32> {
        let doc = self.toks.get(token)?.doc;
        if doc == 0 {
            return None;
        }
        Some(self.comments.get(doc as usize - 1)?.start)
    }

    pub(crate) fn problem_at(&mut self, token: usize, reason: DiagnosticReason) {
        self.draft.problems.push((self.start_byte(token), reason));
    }

    pub(crate) fn is_annotation(&self, index: usize) -> bool {
        self.is_punct(index, "@") && !self.is_word(index + 1, "interface")
    }

    pub(crate) fn annotation_name(&self, index: usize) -> String {
        let mut cursor = index + 1;
        if !self.word(cursor) {
            return String::new();
        }
        let mut name = self.text(cursor).to_string();
        cursor += 1;
        while self.is_punct(cursor, ".") && self.word(cursor + 1) {
            name.push('.');
            name.push_str(self.text(cursor + 1));
            cursor += 2;
        }
        name
    }

    pub(crate) fn skip_annotation_tokens(&self, index: usize, end: usize) -> usize {
        if !self.is_punct(index, "@") {
            return index;
        }
        let mut cursor = index + 1;
        if cursor < end && self.word(cursor) {
            cursor += 1;
            while cursor + 1 < end && self.is_punct(cursor, ".") && self.word(cursor + 1) {
                cursor += 2;
            }
        }
        if cursor < end
            && self.is_punct(cursor, "(")
            && let Some(close) = self.close(cursor)
        {
            return (close + 1).min(end);
        }
        cursor.min(end)
    }

    pub(crate) fn prefix(&self, mut index: usize, end: usize) -> Prefix {
        let mut annotations = Vec::new();
        let mut public = false;
        let mut private = false;
        while index < end {
            if self.is_annotation(index) {
                let args = self.is_punct(self.annotation_args(index), "(").then(|| self.annotation_args(index));
                annotations.push(Ann {
                    name: self.annotation_name(index),
                    at: index,
                    args,
                });
                let next = self.skip_annotation_tokens(index, end);
                index = if next > index { next } else { index + 1 };
                continue;
            }
            if self.is_word(index, "non")
                && self.is_punct(index + 1, "-")
                && self.is_word(index + 2, "sealed")
            {
                index += 3;
                continue;
            }
            if self.word(index) && is_modifier_word(self.text(index)) {
                public |= self.text(index) == "public";
                private |= self.text(index) == "private";
                index += 1;
                continue;
            }
            break;
        }
        Prefix {
            next: index,
            public,
            private,
            annotations,
        }
    }

    fn annotation_args(&self, index: usize) -> usize {
        let mut cursor = index + 1;
        if self.word(cursor) {
            cursor += 1;
            while self.is_punct(cursor, ".") && self.word(cursor + 1) {
                cursor += 2;
            }
        }
        cursor
    }

    pub(crate) fn decl_kind(&self, index: usize) -> Option<DeclKind> {
        if self.is_punct(index, "@") && self.is_word(index + 1, "interface") {
            return Some(DeclKind::Annotation);
        }
        if !self.word(index) {
            return None;
        }
        match self.text(index) {
            "class" => Some(DeclKind::Class),
            "interface" => Some(DeclKind::Interface),
            "enum" => Some(DeclKind::Enum),
            "record" => Some(DeclKind::Record),
            _ => None,
        }
    }

    pub(crate) fn name_at(&self, keyword: usize, kind: DeclKind) -> usize {
        match kind {
            DeclKind::Annotation => keyword + 2,
            _ => keyword + 1,
        }
    }

    /// Emits a symbol and returns the scope of its body. `None` abstains.
    pub(crate) fn emit(&mut self, draft: SymbolDraft<'_>, parent: &Scope) -> Option<Scope> {
        if self.draft.overflow || !self.ok() {
            return None;
        }
        let qualified = if parent.name.is_empty() {
            draft.local.to_string()
        } else {
            format!("{}::{}", parent.name, draft.local)
        };
        if !valid_name(&qualified) {
            self.problem_at(draft.first, DiagnosticReason::UnsupportedName);
            return None;
        }
        if !self.draft.accept() {
            return None;
        }
        let path = self.draft.path.clone();
        let id = match StableId::new(draft.node_kind, &path, Some(&qualified), None) {
            Ok(id) => id,
            Err(_) => {
                self.problem_at(draft.first, DiagnosticReason::UnsupportedName);
                return None;
            }
        };
        let signature = self.signature(draft.first, draft.sig_end);
        let summary = match self.doc_text(draft.first) {
            Some(text) => Summary {
                text,
                source: SummarySource::Doc,
            },
            None => Summary {
                text: signature.clone(),
                source: SummarySource::Signature,
            },
        };
        let fingerprint = body_fingerprint(self.src_span(draft.body.0, draft.body.1));
        let start = self
            .comment_start(draft.first)
            .unwrap_or_else(|| self.start_byte(draft.first));
        let end = self.end_byte(draft.last);
        let lines = [
            self.line_of_byte(start),
            self.line_of_byte(end.saturating_sub(1).max(start)),
        ];
        let node = Node {
            id,
            kind: draft.node_kind,
            symbol_kind: Some(draft.symbol),
            path,
            lines,
            name: qualified.clone(),
            signature,
            fingerprint,
            public: draft.is_public,
            summary,
        };
        self.draft.nodes.push(DraftNode {
            node,
            parent: parent.owner,
            start,
            end,
            precision: draft.precision,
        });
        let mut type_params = parent.type_params.clone();
        type_params.extend(draft.type_params);
        Some(Scope {
            owner: self.draft.nodes.len() - 1,
            name: qualified,
            simple: draft.simple.to_string(),
            type_params,
            kind: draft.scope_kind,
            in_test: parent.in_test || draft.node_kind == NodeKind::Test,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn refer(
        &mut self,
        src: usize,
        scope: &Scope,
        kind: EdgeKind,
        name: String,
        alias: Option<String>,
        from: usize,
        to: usize,
        precision: Precision,
    ) {
        if name.is_empty() || !self.ok() || !self.draft.accept() {
            return;
        }
        let start = self.start_byte(from);
        let end = self.end_byte(to);
        let lines = [
            self.line_of_byte(start),
            self.line_of_byte(end.saturating_sub(1).max(start)),
        ];
        self.draft.refs.push(DraftRef {
            src,
            kind,
            name,
            scope: scope.name.clone(),
            alias,
            lines,
            start,
            end,
            precision,
        });
    }
}

pub(crate) fn is_modifier_word(word: &str) -> bool {
    matches!(
        word,
        "public"
            | "protected"
            | "private"
            | "static"
            | "final"
            | "abstract"
            | "sealed"
            | "strictfp"
            | "native"
            | "synchronized"
            | "transient"
            | "volatile"
            | "default"
            | "transitive"
    )
}

pub(crate) fn is_primitive(word: &str) -> bool {
    matches!(
        word,
        "byte" | "short" | "int" | "long" | "char" | "float" | "double" | "boolean" | "void"
    )
}

pub(crate) fn is_non_callee(word: &str) -> bool {
    matches!(
        word,
        "if" | "for"
            | "while"
            | "switch"
            | "catch"
            | "synchronized"
            | "try"
            | "do"
            | "else"
            | "return"
            | "throw"
            | "break"
            | "continue"
            | "assert"
            | "case"
            | "default"
            | "new"
            | "class"
            | "interface"
            | "enum"
            | "record"
            | "package"
            | "import"
            | "instanceof"
            | "finally"
    )
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
