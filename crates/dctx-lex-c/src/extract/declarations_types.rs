// SPDX-License-Identifier: Apache-2.0
//! Type declarations and the draft ledger that finishes a `LexedFile`.
//! Implements SPEC-MAP-MODE 5.1 for struct, class, union and enum containment.
//! Anonymous types contribute no symbol. A friend is a type reference only.
//! The ledger assigns source order, duplicate ordinals and containment edges.
use super::{Item, Region, Scope, Walker};
use crate::domain::{
    Completion, Diagnostic, DiagnosticReason, Dialect, Labeled, LexError, LexedFile, Precision,
    RawReference, body_fingerprint,
};
use crate::lexer::Lexed;
use dctx_map_model::{
    Confidence, Edge, EdgeKind, Node, NodeKind, StableId, Summary, SummarySource, SymbolKind,
};
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};

impl Walker<'_> {
    #[allow(
        clippy::too_many_lines,
        reason = "Keep the ordered C/C++ declaration-kind dispatch together with its shared token boundaries and recovery."
    )]
    pub(crate) fn type_decl(
        &mut self,
        sig: usize,
        intro: usize,
        end: usize,
        scope: &Scope,
        typedef: bool,
    ) -> usize {
        let keyword = self.text(intro).to_string();
        let mut index = intro + 1;
        index = self.skip_attributes(index, end);
        if keyword == "enum" && matches!(self.text(index), "class" | "struct") {
            index += 1;
            index = self.skip_attributes(index, end);
        }
        let tag = if self.can_name(index) && !self.known_macro(self.text(index)) {
            let mut local = self.text(index).to_string();
            index += 1;
            while self.is(index, "::") && self.can_name(index + 1) {
                local.push_str("::");
                local.push_str(self.text(index + 1));
                index += 2;
            }
            if self.is(index, "<")
                && let Some(next) = self.skip_angles(index, end)
            {
                index = next;
            }
            Some((index.saturating_sub(1), local))
        } else if self.ident(index) && self.known_macro(self.text(index)) {
            self.problem(self.start_of(index), DiagnosticReason::HiddenByMacro);
            return self.consume_semi(sig, end);
        } else {
            None
        };
        index = self.skip_attributes(index, end);
        while matches!(self.text(index), "final" | "override") {
            index += 1;
            index = self.skip_attributes(index, end);
        }
        if self.is(index, ":") {
            let bases = index + 1;
            index += 1;
            while index < end && !self.is(index, "{") && !self.is(index, ";") {
                if self.is(index, "(") || self.is(index, "[") || self.is(index, "{") {
                    index = self.after(index);
                } else {
                    index += 1;
                }
            }
            self.emit_types(bases, index, usize::MAX, usize::MAX, scope);
        }
        let (body, after_body) = if self.is(index, "{") {
            let open = index;
            let close = self.close(open).unwrap_or(end.saturating_sub(1).max(open));
            (Some((open, close)), (close + 1).min(end))
        } else {
            (None, index)
        };
        let stop = self.consume_semi(after_body, end);
        let last = stop
            .saturating_sub(1)
            .max(intro)
            .min(self.toks.len().saturating_sub(1));
        let symbol = match keyword.as_str() {
            "class" => Some(SymbolKind::Class),
            "enum" => Some(SymbolKind::Enum),
            "struct" => Some(SymbolKind::Struct),
            _ => Some(SymbolKind::Type),
        };
        let header_end = body.map_or(self.start_of(last), |(open, _)| self.start_of(open));
        if let Some((_, ref name)) = tag {
            let item = Item {
                symbol,
                local: name.clone(),
                first: sig,
                last,
                sig_end: header_end,
                body: (self.start_of(sig), self.end_of(last)),
                public: self.decl_public(sig, intro, scope),
            };
            if let Some(mut inner) = self.emit(item, scope)
                && let Some((open, close)) = body
            {
                if keyword == "enum" {
                    inner.region = Region::Type;
                    self.enum_body(open, close.min(end), &inner);
                } else {
                    inner.region = Region::Type;
                    inner.public = keyword != "class";
                    self.walk(open + 1, close.min(end), &inner);
                }
            }
        } else if let Some((open, close)) = body {
            self.scan_range(open + 1, close.min(end), scope);
        }
        let trailing = self.decl_names_forced(after_body, stop, body.is_some());
        for (at, bare) in trailing {
            if tag.as_ref().is_some_and(|(_, name)| name == &bare) {
                continue;
            }
            if self.known_macro(&bare) {
                self.problem(self.start_of(at), DiagnosticReason::HiddenByMacro);
                continue;
            }
            let item = Item {
                symbol: if typedef {
                    Some(SymbolKind::Type)
                } else {
                    None
                },
                local: bare.clone(),
                first: sig,
                last,
                sig_end: self.start_of(last),
                body: (self.start_of(sig), self.end_of(last)),
                public: self.decl_public(sig, at, scope),
            };
            let _ = self.emit(item, scope);
        }
        stop
    }

    fn enum_body(&mut self, open: usize, close: usize, scope: &Scope) {
        let mut index = open + 1;
        while index < close && !self.stopped() {
            if self.kind(index) == Some(crate::lexer::Kind::Directive) {
                index = self.directive(index, scope);
                continue;
            }
            if self.can_name(index) && !self.known_macro(self.text(index)) {
                let name = self.text(index).to_string();
                let mut last = index;
                let mut next = index + 1;
                if self.is(next, "=") {
                    next += 1;
                    while next < close && !self.is(next, ",") {
                        if self.is(next, "(") || self.is(next, "{") || self.is(next, "[") {
                            next = self.after(next);
                        } else {
                            last = next;
                            next += 1;
                        }
                    }
                }
                let item = Item {
                    symbol: Some(SymbolKind::Const),
                    local: name.clone(),
                    first: index,
                    last,
                    sig_end: self.end_of(last),
                    body: (self.start_of(index), self.end_of(last)),
                    public: true,
                };
                let _ = self.emit(item, scope);
                self.scan_range(index + 1, next.min(close), scope);
                index = next.max(index + 1);
                if self.is(index, ",") {
                    index += 1;
                }
                continue;
            }
            index += 1;
        }
    }
}

/// One symbol while its parent index and physical span are still draft coordinates.
struct DraftNode {
    node: Node,
    parent: usize,
    start: u32,
    end: u32,
    precision: Precision,
}

/// One unresolved reference before source ids replace draft indexes.
pub(super) struct DraftRef {
    /// Draft index of the owning node.
    pub(super) src: usize,
    /// Relation. Never containment.
    pub(super) kind: EdgeKind,
    /// Referenced spelling.
    pub(super) name: String,
    /// Qualified scope, empty at file scope.
    pub(super) scope: String,
    /// Local binding. Includes have none.
    pub(super) alias: Option<String>,
    /// One-based physical lines.
    pub(super) lines: [u32; 2],
    /// Physical start byte.
    pub(super) start: u32,
    /// Physical end byte.
    pub(super) end: u32,
    /// Precision before a later diagnostic can soften it.
    pub(super) precision: Precision,
}

/// Nodes and references collected for one file, finished into a [`LexedFile`].
pub(super) struct Draft {
    pub(super) path: String,
    nodes: Vec<DraftNode>,
    refs: Vec<DraftRef>,
    pub(super) problems: Vec<(u32, DiagnosticReason)>,
    max_facts: usize,
    pub(super) overflow: bool,
}

impl Draft {
    #[allow(
        clippy::cast_possible_truncation,
        reason = "Source offsets, line counts and token counts fit u32 because the public extractor enforces the 8 MiB input ceiling."
    )]
    pub(super) fn new(path: &str, source: &str, max_facts: usize) -> Result<Self, LexError> {
        let id = StableId::new(NodeKind::Module, path, None, None)?;
        if max_facts == 0 {
            return Err(LexError::TooManyFacts { limit: 0 });
        }
        let lines = source.bytes().filter(|byte| *byte == b'\n').count() as u32 + 1;
        let module = Node {
            id,
            kind: NodeKind::Module,
            symbol_kind: None,
            path: path.to_string(),
            lines: [1, lines.max(1)],
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
                start: 0,
                end: u32::MAX,
                precision: Precision::Structural,
            }],
            refs: Vec::new(),
            problems: Vec::new(),
            max_facts,
            overflow: false,
        })
    }

    pub(super) fn set_module_summary(&mut self, text: String) {
        self.nodes[0].node.summary = Summary {
            text,
            source: SummarySource::Doc,
        };
    }

    pub(super) fn full(&mut self) -> bool {
        if self.nodes.len() + self.refs.len() >= self.max_facts {
            self.overflow = true;
            true
        } else {
            false
        }
    }

    pub(super) fn node(
        &mut self,
        node: Node,
        parent: usize,
        (start, end): (u32, u32),
        precision: Precision,
    ) -> Option<usize> {
        if self.full() {
            return None;
        }
        self.nodes.push(DraftNode {
            node,
            parent,
            start,
            end,
            precision,
        });
        Some(self.nodes.len() - 1)
    }

    pub(super) fn reference(&mut self, reference: DraftRef) {
        if !self.full() {
            self.refs.push(reference);
        }
    }

    #[allow(
        clippy::cast_possible_truncation,
        reason = "Source offsets, line counts and token counts fit u32 because the public extractor enforces the 8 MiB input ceiling."
    )]
    pub(super) fn finish(
        mut self,
        lexed: &Lexed,
        original: &str,
        dialect: Dialect,
        max_facts: usize,
    ) -> Result<LexedFile, LexError> {
        if self.overflow {
            return Err(LexError::TooManyFacts { limit: max_facts });
        }
        let mut order: Vec<usize> = (1..self.nodes.len()).collect();
        order.sort_by_key(|&index| (self.nodes[index].start, Reverse(self.nodes[index].end)));
        let mut remap = vec![0; self.nodes.len()];
        for (position, &old) in order.iter().enumerate() {
            remap[old] = position + 1;
        }
        let mut slots: Vec<Option<DraftNode>> = self.nodes.drain(..).map(Some).collect();
        let mut nodes = Vec::with_capacity(slots.len());
        nodes.extend(slots.first_mut().and_then(Option::take));
        for &old in &order {
            nodes.extend(slots.get_mut(old).and_then(Option::take));
        }
        for node in nodes.iter_mut().skip(1) {
            node.parent = remap.get(node.parent).copied().unwrap_or(0);
        }
        assign_ordinals(&mut nodes)?;
        self.refs
            .sort_by_key(|reference| (reference.start, Reverse(reference.end)));
        let mut diagnostics = lexed.diagnostics.clone();
        let limit = original.len() as u32;
        diagnostics.extend(self.problems.iter().map(|&(logical, reason)| Diagnostic {
            line: lexed.map.line(logical).clamp(1, lexed.map.physical_lines),
            byte: lexed.map.physical(logical).min(limit),
            reason,
        }));
        diagnostics.sort_by_key(|diagnostic| (diagnostic.byte, diagnostic.reason as u8));
        let first = diagnostics
            .first()
            .map_or(u32::MAX, |diagnostic| diagnostic.byte);
        let completion = if diagnostics.is_empty() {
            Completion::Complete
        } else {
            Completion::Partial
        };
        diagnostics.truncate(LexedFile::MAX_DIAGNOSTICS);
        let label = |end: u32, base: Precision| {
            if end != u32::MAX && end > first {
                Precision::Partial
            } else {
                base
            }
        };
        let path = self.path.clone();
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
            .filter(|reference| reference.src < remap.len())
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
            .map(|draft| Labeled {
                precision: label(draft.end, draft.precision),
                fact: draft.node,
            })
            .collect();
        Ok(LexedFile {
            path,
            dialect,
            completion,
            diagnostics,
            nodes,
            edges,
            references,
        })
    }
}

fn assign_ordinals(nodes: &mut [DraftNode]) -> Result<(), LexError> {
    let mut counts: BTreeMap<(NodeKind, String), u32> = BTreeMap::new();
    for draft in nodes.iter().skip(1) {
        *counts
            .entry((draft.node.kind, draft.node.name.clone()))
            .or_insert(0) += 1;
    }
    let mut seen: BTreeMap<(NodeKind, String), u32> = BTreeMap::new();
    let mut used = BTreeSet::new();
    if let Some(root) = nodes.first() {
        used.insert(root.node.id.as_str().to_string());
    }
    for draft in nodes.iter_mut().skip(1) {
        let key = (draft.node.kind, draft.node.name.clone());
        let next = seen.entry(key.clone()).or_insert(0);
        *next += 1;
        let ordinal = (counts[&key] > 1).then_some(*next);
        let mut id = StableId::new(
            draft.node.kind,
            &draft.node.path,
            Some(&draft.node.name),
            ordinal,
        )?;
        if !used.insert(id.as_str().to_string()) {
            let fallback = Some(ordinal.unwrap_or(1).saturating_add(1).max(1));
            id = StableId::new(
                draft.node.kind,
                &draft.node.path,
                Some(&draft.node.name),
                fallback,
            )?;
            used.insert(id.as_str().to_string());
        }
        draft.node.id = id;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::CLexer;
    use dctx_map_model::SymbolKind;

    #[test]
    fn class_struct_enum_and_union_keep_their_kinds() {
        let src = b"struct S { int a; };\nclass C { public: void m(); int x; };\nenum E { A };\nunion U { int n; };\n";
        let facts = CLexer::default().extract("t.hpp", src).unwrap();
        let kind = |name: &str| {
            facts
                .nodes
                .iter()
                .find(|node| node.fact.name == name)
                .map(|node| node.fact.symbol_kind)
        };
        assert_eq!(kind("S"), Some(Some(SymbolKind::Struct)));
        assert_eq!(kind("C"), Some(Some(SymbolKind::Class)));
        assert_eq!(kind("C::m"), Some(Some(SymbolKind::Method)));
        assert_eq!(kind("E"), Some(Some(SymbolKind::Enum)));
        assert_eq!(kind("E::A"), Some(Some(SymbolKind::Const)));
        assert_eq!(kind("U"), Some(Some(SymbolKind::Type)));
        assert!(
            facts
                .nodes
                .iter()
                .any(|node| node.fact.signature.contains("union U"))
        );
    }
}
