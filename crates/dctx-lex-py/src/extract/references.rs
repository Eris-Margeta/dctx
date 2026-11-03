// SPDX-License-Identifier: Apache-2.0
//! Calls, module-level constants and the nodes and references they emit.
//! Implements SPEC-MAP-MODE 5.1 constants and 5.3 call references for Python.
//! Declarations and imports call these while they walk a span.
use super::{DraftNode, Scope, Walker, qualify};
use crate::domain::{DiagnosticReason, Precision, body_fingerprint, collapse};
use crate::lexer::{Kind, Token};
use dctx_map_model::{EdgeKind, Node, NodeKind, StableId, Summary, SummarySource, SymbolKind};

impl Walker<'_> {
    pub(super) fn constants(&mut self, lo: usize, hi: usize, scope: &Scope) {
        for (start, end) in assignment_targets(self, lo, hi) {
            let Some(names) = const_pattern(self, start, end) else {
                continue;
            };
            for name in names {
                self.emit_const(scope, &name, lo, hi);
            }
        }
    }

    pub(super) fn calls(&mut self, lo: usize, hi: usize, scope: &Scope) {
        let mut j = lo;
        while j < hi {
            if let Some(paren) = chain_paren(self, j, lo, hi) {
                let name = dotted(self, j, paren);
                let end = group_end(self, paren, hi);
                if !name.is_empty() {
                    self.emit_ref(scope, EdgeKind::Calls, name, None, j, end, Precision::Structural);
                }
                j = paren;
                continue;
            }
            j += 1;
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn emit_symbol(
        &mut self,
        scope: &Scope,
        qualified: &str,
        local: &str,
        symbol: SymbolKind,
        sig_at: usize,
        colon: usize,
        doc: Option<String>,
    ) -> Option<usize> {
        if !name_ok(qualified) {
            self.problem_here(DiagnosticReason::UnsupportedName);
            return None;
        }
        let start = self.tok_start(sig_at);
        let end = self.tok_end(colon);
        let signature = collapse(self.slice(start, end));
        let summary = match doc {
            Some(text) => Summary {
                text,
                source: SummarySource::Doc,
            },
            None => Summary {
                text: signature.clone(),
                source: SummarySource::Signature,
            },
        };
        let id = StableId::new(NodeKind::Symbol, &self.path, Some(qualified), None).ok()?;
        let node = Node {
            id,
            kind: NodeKind::Symbol,
            symbol_kind: Some(symbol),
            path: self.path.clone(),
            lines: [self.line_of(start), self.line_of(end.saturating_sub(1).max(start))],
            name: qualified.to_owned(),
            signature,
            fingerprint: body_fingerprint(""),
            public: is_public(local),
            summary,
        };
        self.push_node(node, scope.owner, start, end, Precision::Structural)
    }

    pub(super) fn finish_body(&mut self, owner: usize, colon: usize) {
        let Some(node) = self.draft.nodes.get(owner) else {
            return;
        };
        let colon_end = node.end;
        let started = node.start;
        let (body, end_byte) = match content_span(self.toks, colon.saturating_add(1), self.i) {
            Some((start, end)) => (self.slice(start, end).to_owned(), end),
            None => (String::new(), colon_end),
        };
        let line = self.line_of(end_byte.saturating_sub(1).max(started));
        let Some(slot) = self.draft.nodes.get_mut(owner) else {
            return;
        };
        slot.node.fingerprint = body_fingerprint(&body);
        slot.node.lines[1] = line;
        slot.end = end_byte.max(slot.end);
    }

    fn emit_const(&mut self, scope: &Scope, local: &str, lo: usize, hi: usize) {
        if !name_ok(local) {
            self.problem_here(DiagnosticReason::UnsupportedName);
            return;
        }
        let (start, end) = self.span_bytes(lo, hi);
        let signature = collapse(self.slice(start, end));
        let qualified = qualify(scope, local);
        let Ok(id) = StableId::new(NodeKind::Symbol, &self.path, Some(&qualified), None) else {
            self.problem_here(DiagnosticReason::UnsupportedName);
            return;
        };
        let node = Node {
            id,
            kind: NodeKind::Symbol,
            symbol_kind: Some(SymbolKind::Const),
            path: self.path.clone(),
            lines: [self.line_of(start), self.line_of(end.saturating_sub(1).max(start))],
            name: qualified,
            signature: signature.clone(),
            fingerprint: body_fingerprint(self.slice(start, end)),
            public: is_public(local),
            summary: Summary {
                text: signature,
                source: SummarySource::Signature,
            },
        };
        let _ = self.push_node(node, scope.owner, start, end, Precision::Convention);
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn emit_ref(
        &mut self,
        scope: &Scope,
        kind: EdgeKind,
        name: String,
        alias: Option<String>,
        lo: usize,
        hi: usize,
        precision: Precision,
    ) {
        if name.is_empty() || self.refused.is_some() {
            return;
        }
        if self.draft.nodes.len() + self.draft.refs.len() >= self.max_facts {
            self.refused = Some(crate::domain::LexError::TooManyFacts {
                limit: self.max_facts,
            });
            self.i = self.toks.len();
            return;
        }
        let start = self.tok_start(lo);
        let end = self.tok_end(hi).max(start);
        self.draft.refs.push(super::DraftRef {
            src: scope.owner,
            kind,
            name,
            scope: scope.name.clone(),
            alias,
            lines: [self.line_of(start), self.line_of(end.saturating_sub(1).max(start))],
            start,
            end,
            precision,
        });
    }

    fn push_node(
        &mut self,
        node: Node,
        parent: usize,
        start: u32,
        end: u32,
        precision: Precision,
    ) -> Option<usize> {
        if self.draft.nodes.len() + self.draft.refs.len() >= self.max_facts {
            self.refused = Some(crate::domain::LexError::TooManyFacts {
                limit: self.max_facts,
            });
            self.i = self.toks.len();
            return None;
        }
        self.draft.nodes.push(DraftNode {
            node,
            parent,
            start,
            end,
            precision,
        });
        Some(self.draft.nodes.len() - 1)
    }

    pub(super) fn header_follows(&self) -> bool {
        let mut j = self.i;
        let mut depth = 0i32;
        while j < self.toks.len() {
            if depth <= 0 && self.op_at(j) == Some(":") {
                return true;
            }
            if depth <= 0
                && matches!(
                    self.kind_at(j),
                    Some(Kind::Newline | Kind::Dedent | Kind::Indent)
                )
            {
                return false;
            }
            depth += self.delta(j);
            j += 1;
        }
        false
    }
}

fn assignment_targets(walker: &Walker<'_>, lo: usize, hi: usize) -> Vec<(usize, usize)> {
    let mut depth = 0i32;
    let mut colon = None;
    let mut eqs = Vec::new();
    let mut index = lo;
    while index < hi {
        if depth <= 0 && walker.op_at(index) == Some(":") && colon.is_none() && eqs.is_empty()
        {
            colon = Some(index);
        }
        if depth <= 0 && walker.op_at(index) == Some("=") {
            eqs.push(index);
        }
        depth += walker.delta(index);
        index += 1;
    }
    if let Some(colon_at) = colon
        && eqs.first().is_none_or(|eq| colon_at < *eq)
    {
        return vec![(lo, colon_at)];
    }
    let mut targets = Vec::new();
    let mut from = lo;
    for eq in eqs {
        targets.push((from, eq));
        from = eq + 1;
    }
    targets
}

fn const_pattern(walker: &Walker<'_>, lo: usize, hi: usize) -> Option<Vec<String>> {
    if lo >= hi {
        return None;
    }
    let mut names = Vec::new();
    for index in lo..hi {
        if walker.kind_at(index) == Some(Kind::Ident) {
            let text = walker.text_at(index);
            if is_keyword(text) || !is_constant(text) {
                return None;
            }
            names.push(text.to_owned());
        } else if !matches!(walker.op_at(index), Some("(" | ")" | "[" | "]" | "," | "*")) {
            return None;
        }
    }
    if names.is_empty() { None } else { Some(names) }
}

fn chain_paren(walker: &Walker<'_>, start: usize, lo: usize, hi: usize) -> Option<usize> {
    if start > lo && walker.op_at(start - 1) == Some(".") {
        return None;
    }
    if !is_call_name(walker, start) {
        return None;
    }
    let mut index = start + 1;
    while index + 1 < hi && walker.op_at(index) == Some(".") && is_call_name(walker, index + 1)
    {
        index += 2;
    }
    if index < hi && walker.op_at(index) == Some("(") {
        Some(index)
    } else {
        None
    }
}

fn is_call_name(walker: &Walker<'_>, index: usize) -> bool {
    walker.kind_at(index) == Some(Kind::Ident)
        && !is_keyword(walker.text_at(index))
        && !walker.text_at(index).is_empty()
}

fn dotted(walker: &Walker<'_>, start: usize, paren: usize) -> String {
    let mut name = String::new();
    for index in start..paren {
        if walker.kind_at(index) == Some(Kind::Ident) {
            if !name.is_empty() {
                name.push('.');
            }
            name.push_str(walker.text_at(index));
        }
    }
    name
}

fn group_end(walker: &Walker<'_>, open: usize, hi: usize) -> usize {
    let mut depth = 0i32;
    let mut index = open;
    while index < hi {
        depth += walker.delta(index);
        if depth == 0 {
            return index;
        }
        index += 1;
    }
    hi.saturating_sub(1).max(open)
}

fn content_span(tokens: &[Token], lo: usize, hi: usize) -> Option<(u32, u32)> {
    let hi = hi.min(tokens.len());
    let lo = lo.min(hi);
    let mut start = None;
    let mut end = 0u32;
    for token in &tokens[lo..hi] {
        if matches!(token.kind, Kind::Newline | Kind::Indent | Kind::Dedent) {
            continue;
        }
        if start.is_none() {
            start = Some(token.start);
        }
        end = token.end;
    }
    start.map(|start| (start, end.max(start)))
}

fn is_public(name: &str) -> bool {
    if name.starts_with("__") && name.ends_with("__") && name.len() > 4 {
        return true;
    }
    !name.starts_with('_')
}

fn is_constant(name: &str) -> bool {
    let mut chars = name.chars();
    if !chars.next().is_some_and(|ch| ch.is_ascii_uppercase()) {
        return false;
    }
    name.chars()
        .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_')
}

fn name_ok(name: &str) -> bool {
    !name.is_empty()
        && !name.contains(['~', '#'])
        && !name.chars().any(char::is_control)
}

pub(super) fn is_keyword(text: &str) -> bool {
    matches!(
        text,
        "False"
            | "None"
            | "True"
            | "and"
            | "as"
            | "assert"
            | "async"
            | "await"
            | "break"
            | "class"
            | "continue"
            | "def"
            | "del"
            | "elif"
            | "else"
            | "except"
            | "finally"
            | "for"
            | "from"
            | "global"
            | "if"
            | "import"
            | "in"
            | "is"
            | "lambda"
            | "nonlocal"
            | "not"
            | "or"
            | "pass"
            | "raise"
            | "return"
            | "try"
            | "while"
            | "with"
            | "yield"
    )
}
