// SPDX-License-Identifier: Apache-2.0
//! Token cursor, statements and call references.
//! Implements SPEC-MAP-MODE 5.3 `calls` for C#. Bare member reads (`obj.Field`
//! with no call) are abstained: the model has no field-read relation to invent.
//! Lambdas are not symbols; their bodies are still scanned.
use super::{Body, Scope, Walker};
use crate::domain::{
    DiagnosticReason, Precision, body_fingerprint, doc_body, gap_ok, normalize, slice, xml_summary,
};
use crate::lexer::{Kind, NONE, keyword, predefined};
use dctx_map_model::{EdgeKind, SummarySource};

impl<'a> Walker<'a> {
    pub(crate) fn apply_file_doc(&mut self) {
        let Some(text) = self.doc_before(0) else {
            return;
        };
        let summary = &mut self.draft.nodes[0].node.summary;
        summary.text = text;
        summary.source = SummarySource::Doc;
    }

    pub(crate) fn body(&mut self, mut index: usize, end: usize, scope: &Scope, kind: Body) {
        while index < end && !self.draft.overflow {
            let before = index;
            index = self.skip_noise(index, end);
            if index >= end {
                break;
            }
            index = match kind {
                Body::Namespace => self.namespace_item(index, end, scope),
                Body::Type => self.member(index, end, scope),
            };
            if index <= before {
                index = before + 1;
            }
        }
    }

    fn namespace_item(&mut self, index: usize, end: usize, scope: &Scope) -> usize {
        if self.is(index, "namespace") {
            return self.namespace_decl(index, end, scope);
        }
        if self.is(index, "using") || (self.is(index, "global") && self.is(index + 1, "using")) {
            return self.using_decl(index, end, scope);
        }
        let prefix = self.prefix(index, end);
        if self.decl_kind(prefix.next).is_some() {
            return self.type_decl(index, end, scope);
        }
        if prefix.next > index && (prefix.next >= end || self.is(prefix.next, ";")) {
            self.attr_uses(&prefix, scope);
            return (prefix.next + 1).min(end);
        }
        self.statement(index, end, scope)
    }

    pub(crate) fn statement(&mut self, index: usize, end: usize, scope: &Scope) -> usize {
        let next = self.statement_end(index, end);
        self.scan_calls(index, next, scope);
        next
    }

    fn statement_end(&self, mut index: usize, end: usize) -> usize {
        while index < end {
            if self.is(index, "(") || self.is(index, "[") || self.is(index, "{") {
                index = self.after(index, end);
                let closed_brace = index > 0 && self.is(index - 1, "}");
                if closed_brace && !self.is(index, "else") && !self.is(index, ";") {
                    return index;
                }
                continue;
            }
            if self.is(index, ";") {
                return (index + 1).min(end);
            }
            index += 1;
        }
        end
    }

    pub(crate) fn note_namespace(&mut self, name: &str) {
        let module = &mut self.draft.nodes[0].node;
        if !module.signature.is_empty() {
            return;
        }
        module.signature = name.to_string();
        if module.summary.source != SummarySource::Doc {
            module.summary.text = name.to_string();
            module.summary.source = SummarySource::Signature;
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn refer(
        &mut self,
        owner: usize,
        scope: &str,
        kind: EdgeKind,
        name: String,
        alias: Option<String>,
        first: usize,
        last: usize,
    ) {
        if name.is_empty() || self.draft.full() {
            return;
        }
        let start = self.start_byte(first);
        let end = self.end_byte(last).max(start);
        self.draft.refs.push(super::DraftRef {
            src: owner,
            kind,
            name,
            scope: scope.to_string(),
            alias,
            lines: self.span_lines(start, end),
            end,
            precision: Precision::Structural,
        });
    }

    pub(crate) fn set_body(&mut self, index: usize, body: &str) {
        if let Some(node) = self.draft.nodes.get_mut(index) {
            node.node.fingerprint = body_fingerprint(body);
        }
    }

    pub(crate) fn text(&self, index: usize) -> &'a str {
        self.toks
            .get(index)
            .map_or("", |token| slice(self.src, token.start, token.end))
    }

    pub(crate) fn kind(&self, index: usize) -> Option<Kind> {
        self.toks.get(index).map(|token| token.kind)
    }

    pub(crate) fn is(&self, index: usize, spelling: &str) -> bool {
        self.text(index) == spelling
    }

    pub(crate) fn word(&self, index: usize) -> bool {
        self.kind(index) == Some(Kind::Word)
    }

    pub(crate) fn is_keyword(&self, index: usize) -> bool {
        self.word(index) && keyword(self.text(index))
    }

    pub(crate) fn can_name(&self, index: usize) -> bool {
        self.word(index) && !keyword(self.text(index))
    }

    pub(crate) fn type_word(&self, index: usize) -> bool {
        self.word(index) && (predefined(self.text(index)) || !keyword(self.text(index)))
    }

    #[allow(
        clippy::cast_possible_truncation,
        reason = "Source offsets, line counts and token counts fit u32 because the public extractor enforces the 8 MiB input ceiling."
    )]
    pub(crate) fn start_byte(&self, index: usize) -> u32 {
        self.toks
            .get(index)
            .map_or(self.src.len() as u32, |token| token.start)
    }

    #[allow(
        clippy::cast_possible_truncation,
        reason = "Source offsets, line counts and token counts fit u32 because the public extractor enforces the 8 MiB input ceiling."
    )]
    pub(crate) fn end_byte(&self, index: usize) -> u32 {
        self.toks
            .get(index)
            .map_or(self.src.len() as u32, |token| token.end)
    }

    #[allow(
        clippy::cast_possible_truncation,
        reason = "Source offsets, line counts and token counts fit u32 because the public extractor enforces the 8 MiB input ceiling."
    )]
    pub(crate) fn line(&self, byte: u32) -> u32 {
        self.line_starts
            .partition_point(|&start| start <= byte)
            .max(1) as u32
    }

    pub(crate) fn span_lines(&self, start: u32, end: u32) -> [u32; 2] {
        let cap = self.file_end.max(1);
        let first = self.line(start).clamp(1, cap);
        let last = self
            .line(end.saturating_sub(1).max(start))
            .clamp(first, cap);
        [first, last]
    }

    pub(crate) fn span_text(&self, first: usize, last: usize) -> String {
        normalize(slice(self.src, self.start_byte(first), self.end_byte(last)))
    }

    pub(crate) fn partner_within(&self, index: usize, end: usize) -> Option<usize> {
        let partner = self.partner.get(index).copied()?;
        if partner == NONE {
            return None;
        }
        let partner = partner as usize;
        (partner > index && partner < end).then_some(partner)
    }

    pub(crate) fn close_of(&self, index: usize, end: usize) -> usize {
        self.partner_within(index, end)
            .unwrap_or_else(|| end.saturating_sub(1).max(index))
    }

    pub(crate) fn after(&self, index: usize, end: usize) -> usize {
        self.partner_within(index, end)
            .map_or(index + 1, |partner| partner + 1)
            .min(end)
    }

    pub(crate) fn skip_noise(&self, mut index: usize, end: usize) -> usize {
        while index < end && (self.kind(index) == Some(Kind::Preproc) || self.is(index, ";")) {
            index += 1;
        }
        index
    }

    pub(crate) fn problem(&mut self, byte: u32, reason: DiagnosticReason) {
        self.draft.problems.push((byte, reason));
    }

    pub(crate) fn doc_before(&self, token: usize) -> Option<String> {
        let start = self.toks.get(token)?.start;
        let prev_end = token.checked_sub(1).map_or(0, |index| self.toks[index].end);
        let mut limit = start;
        let mut group = Vec::new();
        for comment in self.comments.iter().rev() {
            if comment.end > start {
                continue;
            }
            if !comment.doc || comment.start < prev_end || !gap_ok(self.src, comment.end, limit) {
                break;
            }
            group.push(doc_body(slice(self.src, comment.start, comment.end)));
            limit = comment.start;
        }
        if group.is_empty() {
            return None;
        }
        group.reverse();
        xml_summary(&group)
    }

    pub(crate) fn recover(&mut self, index: usize, end: usize) -> usize {
        let mut cursor = index;
        while cursor < end {
            if self.is(cursor, ";") {
                return (cursor + 1).min(end);
            }
            if self.is(cursor, "{") || self.is(cursor, "(") || self.is(cursor, "[") {
                return self.after(cursor, end);
            }
            cursor += 1;
        }
        end
    }

    pub(crate) fn scan_calls(&mut self, start: usize, end: usize, scope: &Scope) {
        let mut index = start;
        let mut next_local = start;
        while index < end {
            if index >= next_local
                && let Some(next) = self.local_function(index, end, scope, &mut next_local)
            {
                index = next.max(index + 1);
                continue;
            }
            if self.is(index, "new") {
                index = self.scan_new(index, end, scope);
                continue;
            }
            if let Some(next) = self.try_call(index, end, scope) {
                index = next;
                continue;
            }
            index += 1;
        }
    }

    fn scan_new(&mut self, index: usize, end: usize, scope: &Scope) -> usize {
        let next = index + 1;
        if let Some(after) = self.parse_type_end(next, end) {
            if self.is(after, "(") {
                let last = after.saturating_sub(1).max(next);
                let type_name = self.span_text(next, last);
                let name = format!("new {type_name}");
                self.refer(
                    scope.owner,
                    &scope.name,
                    EdgeKind::Calls,
                    name,
                    None,
                    index,
                    last,
                );
                return after;
            }
            return after.max(next);
        }
        if self.is(next, "(") {
            self.refer(
                scope.owner,
                &scope.name,
                EdgeKind::Calls,
                "new".to_string(),
                None,
                index,
                index,
            );
        }
        next
    }

    /// Identifier or dotted chain immediately followed by `(` or a generic argument list and `(`.
    fn try_call(&mut self, start: usize, end: usize, scope: &Scope) -> Option<usize> {
        if !self.can_callee(start) {
            return None;
        }
        let mut last = start;
        let mut index = start;
        loop {
            let dot = index + 1;
            if dot < end && (self.is(dot, ".") || self.is(dot, "?.")) && self.can_callee(dot + 1) {
                last = dot + 1;
                index = last;
                continue;
            }
            break;
        }
        let mut after = index + 1;
        if self.is(after, "<") {
            after = self.close_angle(after, end)? + 1;
        }
        if !self.is(after, "(") {
            return None;
        }
        let name = self.span_text(start, last);
        self.refer(
            scope.owner,
            &scope.name,
            EdgeKind::Calls,
            name,
            None,
            start,
            last,
        );
        Some(after)
    }

    fn can_callee(&self, index: usize) -> bool {
        if !self.word(index) {
            return false;
        }
        matches!(self.text(index), "base" | "this") || !self.is_keyword(index)
    }
}
