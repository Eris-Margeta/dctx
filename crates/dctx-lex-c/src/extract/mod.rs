//! Structural walker: scopes, fact emission and source order.
//! Implements SPEC-MAP-MODE 5.1–5.4 for C and C++. A declaration owns the spans
//! lexically inside it. Macros are recorded and never expanded.
//! Declaration, type, function and reference scans are sibling modules.
mod declarations;
mod declarations_types;
mod functions;
mod references;

use declarations_types::{Draft, DraftRef};

use crate::domain::{
    body_fingerprint, DiagnosticReason, Dialect, LexError, LexedFile, Limits, Precision,
};
use crate::lexer::{slice, Comment, Kind, Lexed, Token, NONE};
use dctx_map_model::{EdgeKind, Node, NodeKind, StableId, Summary, SummarySource, SymbolKind};
use std::collections::BTreeMap;

/// Extracts facts from an already-lexed C or C++ file.
pub(crate) fn extract(
    path: &str,
    original: &str,
    dialect: Dialect,
    lexed: &Lexed,
    limits: &Limits,
) -> Result<LexedFile, LexError> {
    let mut draft = Draft::new(path, original, limits.max_facts)?;
    if let Some(text) = leading_doc(&lexed.logical, &lexed.comments, lexed.tokens.first()) {
        draft.set_module_summary(text);
    }
    let mut walker = Walker {
        logical: &lexed.logical,
        original,
        toks: &lexed.tokens,
        partner: &lexed.partner,
        lexed,
        draft,
        macros: BTreeMap::new(),
        cond: 0,
        cond_open: Vec::new(),
        max_depth: limits.max_depth,
    };
    let root = Scope::file();
    walker.walk(0, walker.toks.len(), &root);
    if let Some(&at) = walker.cond_open.first() {
        walker.problem(at, DiagnosticReason::UnbalancedConditional);
    }
    walker.draft.finish(lexed, original, dialect, limits.max_facts)
}

/// Where a declaration is allowed to introduce names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Region {
    /// File scope.
    File,
    /// Named or anonymous namespace.
    Namespace,
    /// `struct`, `class` or `union` body.
    Type,
    /// Function body: references and directives only.
    Body,
}

/// Lexical owner of nested facts. `name` is `::`-qualified and empty at the root.
#[derive(Debug, Clone)]
pub(crate) struct Scope {
    /// Index of the owning draft node.
    pub owner: usize,
    /// Qualified name, empty for the file root.
    pub name: String,
    /// What this scope may contain.
    pub region: Region,
    /// Default visibility for declarations opened here.
    pub public: bool,
}

impl Scope {
    fn file() -> Self {
        Self {
            owner: 0,
            name: String::new(),
            region: Region::File,
            public: true,
        }
    }
}

/// A recognised declaration, in token and logical-byte coordinates.
pub(crate) struct Item {
    /// Parsed symbol category. Namespaces and fields have none.
    pub symbol: Option<SymbolKind>,
    /// Name relative to the parent scope. May itself contain `::`.
    pub local: String,
    /// First token of the declaration.
    pub first: usize,
    /// Last token of the declaration.
    pub last: usize,
    /// Logical byte where the signature ends, exclusive.
    pub sig_end: u32,
    /// Logical body span used for the fingerprint.
    pub body: (u32, u32),
    /// Public visibility.
    pub public: bool,
}

pub(crate) struct Walker<'a> {
    pub(crate) logical: &'a str,
    original: &'a str,
    pub(crate) toks: &'a [Token],
    partner: &'a [u32],
    lexed: &'a Lexed,
    draft: Draft,
    pub(crate) macros: BTreeMap<String, bool>,
    pub(crate) cond: u32,
    cond_open: Vec<u32>,
    pub(crate) max_depth: u32,
}

impl<'a> Walker<'a> {
    pub(crate) fn stopped(&self) -> bool {
        self.draft.overflow
    }

    pub(crate) fn text(&self, index: usize) -> &'a str {
        self.toks.get(index).map_or("", |token| {
            slice(self.logical, token.start, token.end)
        })
    }

    pub(crate) fn kind(&self, index: usize) -> Option<Kind> {
        self.toks.get(index).map(|token| token.kind)
    }

    pub(crate) fn is(&self, index: usize, spelling: &str) -> bool {
        self.text(index) == spelling
    }

    pub(crate) fn ident(&self, index: usize) -> bool {
        self.kind(index) == Some(Kind::Ident)
    }

    pub(crate) fn start_of(&self, index: usize) -> u32 {
        self.toks.get(index).map_or(self.logical.len() as u32, |token| token.start)
    }

    pub(crate) fn end_of(&self, index: usize) -> u32 {
        self.toks.get(index).map_or(self.logical.len() as u32, |token| token.end)
    }

    pub(crate) fn close(&self, index: usize) -> Option<usize> {
        match self.partner.get(index).copied() {
            Some(partner) if partner != NONE && (partner as usize) > index => Some(partner as usize),
            _ => None,
        }
    }

    pub(crate) fn after(&self, index: usize) -> usize {
        self.close(index).map_or(index + 1, |close| close + 1)
    }

    pub(crate) fn known_macro(&self, name: &str) -> bool {
        self.macros.contains_key(name)
    }

    pub(crate) fn problem(&mut self, logical: u32, reason: DiagnosticReason) {
        self.draft.problems.push((logical, reason));
    }

    pub(crate) fn open_cond(&mut self, at: u32) {
        self.cond += 1;
        self.cond_open.push(at);
    }

    pub(crate) fn close_cond(&mut self, at: u32) {
        if self.cond_open.pop().is_none() {
            self.problem(at, DiagnosticReason::UnbalancedConditional);
        }
        self.cond = self.cond.saturating_sub(1);
    }

    pub(crate) fn soften(&self, precision: Precision) -> Precision {
        if self.cond > 0 && precision == Precision::Structural {
            Precision::Convention
        } else {
            precision
        }
    }

    /// Emits a declaration and returns the scope its contents belong to.
    pub(crate) fn emit(&mut self, item: Item, scope: &Scope) -> Option<Scope> {
        if self.draft.full() {
            return None;
        }
        let name = if scope.name.is_empty() {
            item.local.clone()
        } else {
            format!("{}::{}", scope.name, item.local)
        };
        if !valid_name(&name) {
            self.problem(self.start_of(item.first), DiagnosticReason::UnsupportedName);
            return None;
        }
        let path = self.draft.path.clone();
        let Ok(id) = StableId::new(NodeKind::Symbol, &path, Some(&name), None) else {
            self.problem(self.start_of(item.first), DiagnosticReason::UnsupportedName);
            return None;
        };
        let start_l = self.start_of(item.first);
        let end_l = self.end_of(item.last).max(start_l);
        let sig_end = item.sig_end.clamp(start_l, end_l);
        let mut signature = normalize(slice(self.logical, start_l, sig_end));
        if signature.is_empty() {
            signature = name.clone();
        }
        let summary = match self.doc_before(item.first) {
            Some(text) => Summary {
                text,
                source: SummarySource::Doc,
            },
            None => Summary {
                text: signature.clone(),
                source: SummarySource::Signature,
            },
        };
        let body = self.physical_slice(item.body.0, item.body.1.max(item.body.0));
        let node = Node {
            id,
            kind: NodeKind::Symbol,
            symbol_kind: item.symbol,
            path,
            lines: self.span_lines(start_l, end_l),
            name: name.clone(),
            signature,
            fingerprint: body_fingerprint(body),
            public: item.public,
            summary,
        };
        let start_p = self.lexed.map.physical(start_l);
        let end_p = self.lexed.map.physical(end_l).max(start_p);
        let owner = self.draft.node(node, scope.owner, (start_p, end_p), self.soften(Precision::Structural))?;
        Some(Scope {
            owner,
            name,
            region: scope.region,
            public: scope.public,
        })
    }

    pub(crate) fn refer(
        &mut self,
        owner: usize,
        scope: &Scope,
        kind: EdgeKind,
        name: String,
        span: (usize, usize),
        precision: Precision,
    ) {
        let (first, last) = span;
        if name.is_empty() || name.len() > 512 || self.draft.full() {
            return;
        }
        let start_l = self.start_of(first);
        let end_l = self.end_of(last).max(start_l);
        self.draft.reference(DraftRef {
            src: owner,
            kind,
            name,
            scope: scope.name.clone(),
            alias: None,
            lines: self.span_lines(start_l, end_l),
            start: self.lexed.map.physical(start_l),
            end: self.lexed.map.physical(end_l).max(self.lexed.map.physical(start_l)),
            precision: self.soften(precision),
        });
    }

    pub(crate) fn span_lines(&self, start: u32, end: u32) -> [u32; 2] {
        let file_end = self.lexed.map.physical_lines.max(1);
        let left = self.lexed.map.line(start).clamp(1, file_end);
        let right = self
            .lexed
            .map
            .line(end.saturating_sub(1).max(start))
            .clamp(1, file_end);
        [left, right.max(left)]
    }

    fn physical_slice(&self, start: u32, end: u32) -> &str {
        let from = self.lexed.map.physical(start) as usize;
        let to = self.lexed.map.physical(end).min(self.original.len() as u32) as usize;
        self.original.get(from.min(to)..to).unwrap_or("")
    }

    pub(crate) fn doc_before(&self, token: usize) -> Option<String> {
        let start = self.toks.get(token)?.start;
        let prev_end = token.checked_sub(1).map(|index| self.toks[index].end).unwrap_or(0);
        let mut limit = start;
        let mut group = Vec::new();
        for comment in self.lexed.comments.iter().rev() {
            if !comment.doc || comment.end > start {
                continue;
            }
            if comment.start < prev_end || !gap_ok(self.logical, comment.end, limit) {
                break;
            }
            group.push((comment.start, comment.end));
            limit = comment.start;
        }
        if group.is_empty() {
            return None;
        }
        group.reverse();
        let text = group
            .iter()
            .map(|&(from, to)| comment_body(slice(self.logical, from, to)))
            .collect::<Vec<_>>()
            .join(" ");
        let text = sentence(&text);
        (!text.is_empty()).then_some(text)
    }

    pub(crate) fn skip_angles(&self, lt: usize, end: usize) -> Option<usize> {
        if !self.is(lt, "<") {
            return None;
        }
        let mut depth = 0i32;
        let mut index = lt;
        while index < end {
            if self.is(index, "<") {
                depth += 1;
                if depth as u32 > self.max_depth {
                    return None;
                }
            } else if self.is(index, ">") {
                depth -= 1;
                if depth == 0 {
                    return Some(index + 1);
                }
            } else if self.is(index, ">>") {
                depth -= 2;
                if depth <= 0 {
                    return Some(index + 1);
                }
            } else if self.is(index, "(") || self.is(index, "[") || self.is(index, "{") {
                let next = self.after(index);
                index = if next > index { next } else { index + 1 };
                continue;
            }
            index += 1;
        }
        None
    }

    pub(crate) fn find_lt_back(&self, gt: usize) -> Option<usize> {
        let mut depth = 0i32;
        let mut index = gt;
        loop {
            if self.is(index, ">>") {
                depth += 2;
            } else if self.is(index, ">") {
                depth += 1;
            } else if self.is(index, "<") {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            } else if matches!(self.text(index), ")" | "]" | "}")
                && let Some(open) = self.close_from_closer(index)
            {
                if open == 0 {
                    return None;
                }
                index = open - 1;
                continue;
            }
            if index == 0 {
                return None;
            }
            index -= 1;
        }
    }

    fn close_from_closer(&self, index: usize) -> Option<usize> {
        match self.partner.get(index).copied() {
            Some(open) if open != NONE && (open as usize) < index => Some(open as usize),
            _ => None,
        }
    }
}

fn valid_name(name: &str) -> bool {
    !name.is_empty() && !name.contains(['~', '#']) && !name.chars().any(char::is_control)
}

pub(crate) fn normalize(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn sentence(value: &str) -> String {
    let value = normalize(value);
    match value.find(". ") {
        Some(end) => value[..=end].to_string(),
        None => value,
    }
}

fn comment_body(text: &str) -> String {
    if let Some(rest) = text.strip_prefix("//") {
        return rest.trim().trim_start_matches(['/', '!']).trim().to_string();
    }
    if let Some(inner) = text.strip_prefix("/*").and_then(|body| body.strip_suffix("*/")) {
        let inner = inner.trim().trim_start_matches(['*', '!']);
        return inner
            .lines()
            .map(|line| line.trim().trim_start_matches('*').trim())
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
    }
    text.trim().to_string()
}

fn gap_ok(src: &str, from: u32, to: u32) -> bool {
    if from > to {
        return false;
    }
    let Some(gap) = src.get(from as usize..to as usize) else {
        return false;
    };
    gap.bytes().filter(|byte| *byte == b'\n').count() <= 1
        && gap.bytes().all(|byte| matches!(byte, b' ' | b'\t' | b'\r' | b'\n'))
}

fn leading_doc(src: &str, comments: &[Comment], first: Option<&Token>) -> Option<String> {
    let limit = first.map(|token| token.start).unwrap_or(src.len() as u32);
    let mut parts = Vec::new();
    let mut expect = 0u32;
    for comment in comments {
        if comment.start >= limit {
            break;
        }
        if !comment.doc {
            if parts.is_empty() && gap_ok(src, 0, comment.start) {
                continue;
            }
            break;
        }
        if parts.is_empty() && !gap_ok(src, 0, comment.start) && comment.start != 0 {
            let prefix = src.get(..comment.start as usize).unwrap_or("");
            if !prefix.bytes().all(|byte| matches!(byte, b' ' | b'\t' | b'\r' | b'\n')) {
                break;
            }
        }
        if !parts.is_empty() && !gap_ok(src, expect, comment.start) {
            break;
        }
        parts.push(comment_body(slice(src, comment.start, comment.end)));
        expect = comment.end;
    }
    let text = sentence(&parts.join(" "));
    (!text.is_empty()).then_some(text)
}

#[cfg(test)]
mod tests {
    use crate::CLexer;

    #[test]
    fn duplicate_functions_receive_ordinals() {
        let facts = CLexer::default()
            .extract("p.c", b"int F(void);\nint F(void) { return 1; }\n")
            .unwrap();
        let ids: Vec<_> = facts.nodes.iter().map(|node| node.fact.id.as_str()).collect();
        assert!(ids.contains(&"s:p.c#F~1"), "{ids:?}");
        assert!(ids.contains(&"s:p.c#F~2"), "{ids:?}");
        assert_eq!(facts.edges.len(), facts.nodes.len() - 1);
    }
}
