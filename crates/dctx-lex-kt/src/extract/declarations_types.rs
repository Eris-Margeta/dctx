// SPDX-License-Identifier: Apache-2.0
//! Type declarations: classes, interfaces, objects, enums and data classes.
//! A primary constructor exists only when a parameter list is present.
//! Supertypes are uses-type facts, and calls only when arguments follow.
use super::{Ann, Prefix, Scope, Walker};
use crate::domain::DiagnosticReason;
use dctx_map_model::{EdgeKind, SymbolKind};

struct Header {
    paren: Option<usize>,
    close: Option<usize>,
    ctor_public: bool,
    ctor_anns: Vec<Ann>,
    supers: Option<(usize, usize)>,
    where_clause: Option<(usize, usize)>,
    body: Option<usize>,
    tail: usize,
    sig_end: u32,
}

impl Walker<'_> {
    pub(crate) fn type_decl(&mut self, begin: usize, prefix: &Prefix, end: usize, scope: &Scope) -> usize {
        let keyword_at = prefix.next;
        let keyword = self.text(keyword_at);
        let Some((local, cursor)) = self.type_heading(keyword_at, end, keyword) else {
            self.problem(self.start_byte(keyword_at), DiagnosticReason::Malformed);
            return self.resume((keyword_at + 1).min(end), end);
        };
        let (cursor, angles) = self.type_angles(cursor, end);
        let header = self.type_header(cursor, end, prefix.public);
        if header.paren.is_some() && header.close.is_none() {
            return self.open_type(begin, prefix, (&local, angles), &header, end, scope);
        }
        self.closed_type(begin, prefix, (local, angles), &header, end, scope)
    }

    fn type_heading(&self, keyword_at: usize, end: usize, keyword: &str) -> Option<(String, usize)> {
        let cursor = keyword_at + 1;
        if self.is_decl_name(cursor) {
            return Some((self.bare(cursor), cursor + 1));
        }
        if keyword == "object" && self.anon_object(cursor, end) {
            return Some(("Companion".to_string(), cursor));
        }
        None
    }

    fn anon_object(&self, index: usize, end: usize) -> bool {
        index >= end || self.is(index, "{") || self.is(index, ":") || self.is(index, "where") || self.is_semi(index)
    }

    fn type_angles(&self, cursor: usize, end: usize) -> (usize, Option<(usize, usize)>) {
        if self.is(cursor, "<") && let Some(close) = self.angles_end(cursor, end) {
            return (close + 1, Some((cursor, close)));
        }
        (cursor, None)
    }

    fn open_type(
        &mut self,
        begin: usize,
        prefix: &Prefix,
        local: (&str, Option<(usize, usize)>),
        header: &Header,
        end: usize,
        scope: &Scope,
    ) -> usize {
        let (local, angles) = local;
        let open = header.paren.unwrap_or(begin);
        let sig_end = self.end_byte(open);
        let Some(child) =
            self.emit_type_symbol(begin, prefix, local, (open, sig_end, (self.start_byte(begin), sig_end)), scope)
        else {
            return self.recover_open(open, end);
        };
        self.flush_anns(&child, &prefix.anns);
        if let Some((from, to)) = angles {
            self.colon_types(from, to, &child);
        }
        if let Some(ctor) = self.emit_ctor(&child, local, open, open, header.ctor_public) {
            self.flush_anns(&ctor, &header.ctor_anns);
        }
        self.recover_open(open, end)
    }

    fn closed_type(
        &mut self,
        begin: usize,
        prefix: &Prefix,
        local: (String, Option<(usize, usize)>),
        header: &Header,
        end: usize,
        scope: &Scope,
    ) -> usize {
        let (local, angles) = local;
        let (last, next, body) = self.type_span(begin, header, end);
        let kind = kind_of(prefix, self.text(prefix.next));
        let Some(child) = self.emit(
            super::Symbol {
                kind,
                local: local.clone(),
                first: begin,
                last,
                sig_end: header.sig_end,
                body,
                is_public: prefix.public,
            },
            scope,
        ) else {
            return next;
        };
        self.flush_anns(&child, &prefix.anns);
        if let Some((from, to)) = angles {
            self.colon_types(from, to, &child);
        }
        if let (Some(open), Some(close)) = (header.paren, header.close) {
            if let Some(ctor) = self.emit_ctor(&child, &local, open, close, header.ctor_public) {
                self.flush_anns(&ctor, &header.ctor_anns);
            }
            self.walk_params(open, close, &child, Some(&child));
        }
        if let Some((from, to)) = header.supers {
            self.bind_supers(from, to, &child);
        }
        if let Some((from, to)) = header.where_clause {
            self.colon_types(from, to, &child);
        }
        if let Some(brace) = header.body {
            self.class_body(brace, &child, prefix.enum_kind);
        }
        next
    }

    fn emit_type_symbol(
        &mut self,
        begin: usize,
        prefix: &Prefix,
        local: &str,
        span: (usize, u32, (u32, u32)),
        scope: &Scope,
    ) -> Option<Scope> {
        let (last, sig_end, body) = span;
        self.emit(
            super::Symbol {
                kind: kind_of(prefix, self.text(prefix.next)),
                local: local.to_string(),
                first: begin,
                last,
                sig_end,
                body,
                is_public: prefix.public,
            },
            scope,
        )
    }

    fn emit_ctor(&mut self, owner: &Scope, local: &str, open: usize, last: usize, public: bool) -> Option<Scope> {
        let end = self.end_byte(last);
        self.emit(
            super::Symbol {
                kind: SymbolKind::Method,
                local: local.to_string(),
                first: open,
                last,
                sig_end: end,
                body: (self.start_byte(open), end),
                is_public: public,
            },
            owner,
        )
    }

    fn type_span(&self, begin: usize, header: &Header, end: usize) -> (usize, usize, (u32, u32)) {
        if let Some(open) = header.body {
            let after = self.after(open).max(open + 1);
            let last = after.saturating_sub(1).max(open);
            let next = after.min(end.max(after));
            return (last, next, (self.start_byte(open), self.end_byte(last)));
        }
        let last = header.tail.saturating_sub(1).max(begin);
        (last, self.stop_or_finish(header.tail, end), (self.start_byte(begin), header.sig_end))
    }

    fn type_header(&self, cursor: usize, end: usize, public: bool) -> Header {
        let (at_paren, paren, ctor_public, ctor_anns) = self.peel_ctor(cursor, end, public);
        if let Some(open) = paren {
            let Some(close) = self.matched(open, end) else {
                return Header {
                    paren: Some(open),
                    close: None,
                    ctor_public,
                    ctor_anns,
                    supers: None,
                    where_clause: None,
                    body: None,
                    tail: open,
                    sig_end: self.end_byte(open),
                };
            };
            let mut tail = self.header_tail(close + 1, end, self.end_byte(close));
            tail.paren = Some(open);
            tail.close = Some(close);
            tail.ctor_public = ctor_public;
            tail.ctor_anns = ctor_anns;
            return tail;
        }
        let _ = at_paren;
        self.header_tail(cursor, end, self.end_byte(cursor.saturating_sub(1).max(cursor)))
    }

    /// Annotations and modifiers may sit between the type name and `constructor`,
    /// including across a newline. An inserted semicolon there is not a new statement.
    fn peel_ctor(&self, cursor: usize, end: usize, public: bool) -> (usize, Option<usize>, bool, Vec<Ann>) {
        let mut look = cursor;
        let mut ctor_public = public;
        let mut anns = Vec::new();
        loop {
            look = self.skip_inserted(look, end);
            if look >= end {
                break;
            }
            if self.is(look, "@") {
                let ann = self.read_ann(look, end);
                if ann.to <= look {
                    break;
                }
                look = ann.to;
                anns.push(ann);
                continue;
            }
            if self.modifier_word(look) {
                match self.text(look) {
                    "private" | "protected" | "internal" => ctor_public = false,
                    "public" => ctor_public = true,
                    _ => {}
                }
                look += 1;
                continue;
            }
            break;
        }
        look = self.skip_inserted(look, end);
        if self.is(look, "constructor") {
            look += 1;
            look = self.skip_inserted(look, end);
        }
        if self.is(look, "(") && (look > cursor || self.is(cursor, "(")) {
            return (look, Some(look), ctor_public, anns);
        }
        (cursor, None, public, Vec::new())
    }

    fn skip_inserted(&self, mut index: usize, end: usize) -> usize {
        while index < end && self.is_inserted_semi(index) {
            index += 1;
        }
        index
    }

    fn header_tail(&self, mut cursor: usize, end: usize, fallback: u32) -> Header {
        cursor = self.skip_inert_mods(cursor, end);
        let mut supers = None;
        if self.is(cursor, ":") {
            let to = self.supers_end(cursor + 1, end);
            supers = Some((cursor, to));
            cursor = to;
        }
        let mut where_clause = None;
        if self.is(cursor, "where") {
            let to = self.clause_end(cursor, end);
            where_clause = Some((cursor, to));
            cursor = to;
        }
        let body = self.is(cursor, "{").then_some(cursor);
        let sig_end = if cursor > 0 { self.end_byte(cursor.saturating_sub(1)) } else { fallback };
        Header {
            paren: None,
            close: None,
            ctor_public: true,
            ctor_anns: Vec::new(),
            supers,
            where_clause,
            body,
            tail: cursor,
            sig_end: sig_end.max(fallback),
        }
    }

    fn skip_inert_mods(&self, mut cursor: usize, end: usize) -> usize {
        while self.modifier_word(cursor) {
            let next = cursor + 1;
            if self.is(next, "{") || self.is(next, ":") || self.is(next, "where") || self.is_semi(next) {
                cursor = next;
                continue;
            }
            break;
        }
        cursor.min(end)
    }

    pub(crate) fn supers_end(&self, start: usize, end: usize) -> usize {
        let mut index = start;
        let mut depth = 0i32;
        while index < end {
            if depth == 0 && self.super_stop(index) {
                return index;
            }
            if self.is(index, "<") && let Some(close) = self.angles_end(index, end) {
                index = close + 1;
                continue;
            }
            if self.is(index, "(") || self.is(index, "[") {
                depth += 1;
            } else if self.is(index, ")") || self.is(index, "]") {
                depth = depth.saturating_sub(1);
            }
            index += 1;
        }
        index
    }

    fn super_stop(&self, index: usize) -> bool {
        self.is(index, "{") || self.is(index, "where") || self.is_semi(index) || self.decl_at(index)
    }

    fn bind_supers(&mut self, from: usize, to: usize, scope: &Scope) {
        let mut index = from + 1;
        let mut guard = 0usize;
        while index < to && guard <= to {
            guard += 1;
            if self.is(index, ",") {
                index += 1;
                continue;
            }
            let next = self.one_super(index, to, scope);
            index = if next > index { next } else { index + 1 };
        }
    }

    fn one_super(&mut self, index: usize, end: usize, scope: &Scope) -> usize {
        let (name, next) = self.read_dotted(index, end);
        let mut cursor = next;
        if self.is(cursor, "<") && let Some(close) = self.angles_end(cursor, end) {
            if !name.is_empty() {
                self.refer(scope, EdgeKind::UsesType, name.clone(), None, index, cursor.saturating_sub(1).max(index));
            }
            self.emit_type_args(cursor + 1, close, scope);
            cursor = close + 1;
        }
        if self.is(cursor, "(") {
            self.super_call(&name, index, cursor, end, scope)
        } else {
            if !name.is_empty() && cursor > index {
                self.refer(scope, EdgeKind::UsesType, name, None, index, cursor - 1);
            }
            self.after_super(cursor, end, scope)
        }
    }

    fn super_call(&mut self, name: &str, index: usize, paren: usize, end: usize, scope: &Scope) -> usize {
        if !name.is_empty() {
            let last = paren.saturating_sub(1).max(index);
            self.refer(scope, EdgeKind::UsesType, name.to_string(), None, index, last);
            self.refer(scope, EdgeKind::Calls, name.to_string(), None, index, last);
        }
        let mut cursor = paren + 1;
        if let Some(close) = self.matched(paren, end) {
            self.scan_refs(paren + 1, close, scope);
            cursor = close + 1;
        }
        self.after_super(cursor, end, scope)
    }

    fn after_super(&mut self, mut cursor: usize, end: usize, scope: &Scope) -> usize {
        if self.is(cursor, "by") {
            let stop = self.expr_end(cursor + 1, end);
            self.scan_refs(cursor + 1, stop, scope);
            cursor = stop;
        }
        cursor
    }

    pub(crate) fn secondary_ctor(&mut self, begin: usize, prefix: &Prefix, end: usize, scope: &Scope) -> usize {
        let keyword_at = prefix.next;
        let paren = keyword_at + 1;
        if !self.is(paren, "(") {
            self.problem(self.start_byte(keyword_at), DiagnosticReason::Malformed);
            return self.resume(paren.min(end), end);
        }
        let Some(close) = self.matched(paren, end) else {
            self.emit_secondary(begin, prefix, paren, self.end_byte(paren), scope);
            return self.recover_open(paren, end);
        };
        let mut cursor = close + 1;
        let mut sig_end = self.end_byte(close);
        let mut deleg = None;
        if self.is(cursor, ":") {
            let to = self.supers_end(cursor + 1, end);
            sig_end = self.end_byte(to.saturating_sub(1).max(cursor));
            deleg = Some((cursor + 1, to));
            cursor = to;
        }
        let (last, next, scan) = self.ctor_tail(cursor, end, paren);
        if let Some(child) = self.emit_secondary(begin, prefix, last, sig_end, scope) {
            self.flush_anns(&child, &prefix.anns);
            self.walk_params(paren, close, &child, None);
            if let Some((from, to)) = deleg {
                self.scan_refs(from, to, &child);
            }
            if let Some((from, to)) = scan {
                self.scan_refs(from, to, &child);
            }
        }
        next
    }

    fn emit_secondary(&mut self, begin: usize, prefix: &Prefix, last: usize, sig_end: u32, scope: &Scope) -> Option<Scope> {
        let local = if scope.simple.is_empty() { "constructor".to_string() } else { scope.simple.clone() };
        self.emit(
            super::Symbol {
                kind: SymbolKind::Method,
                local,
                first: begin,
                last,
                sig_end,
                body: (self.start_byte(begin), self.end_byte(last).max(sig_end)),
                is_public: prefix.public,
            },
            scope,
        )
    }

    fn ctor_tail(&self, cursor: usize, end: usize, paren: usize) -> (usize, usize, Option<(usize, usize)>) {
        if self.is(cursor, "{") {
            let after = self.after(cursor).max(cursor + 1);
            let last = after.saturating_sub(1).max(cursor);
            let scan = self.matched(cursor, end).filter(|close| *close > cursor).map(|close| (cursor + 1, close));
            return (last, after.min(end.max(after)), scan);
        }
        (cursor.saturating_sub(1).max(paren), self.stop_or_finish(cursor, end), None)
    }

    pub(crate) fn init_block(&mut self, index: usize, end: usize, scope: &Scope) -> usize {
        let brace = index + 1;
        if self.is(brace, "{") {
            if let Some(close) = self.matched(brace, end) {
                self.scan_refs(brace + 1, close, scope);
            }
            return self.after(brace).min(end.max(brace + 1));
        }
        self.resume(brace.min(end), end)
    }

    pub(crate) fn skip_accessor(&mut self, index: usize, end: usize, scope: &Scope) -> usize {
        let next = self.skip_one_accessor(index, end);
        self.scan_accessors(index, next.max(index), scope);
        self.stop_or_finish(next, end)
    }
}

fn kind_of(prefix: &Prefix, keyword: &str) -> SymbolKind {
    if prefix.enum_kind {
        SymbolKind::Enum
    } else if prefix.annotation || keyword == "interface" {
        SymbolKind::Interface
    } else if prefix.data && keyword == "class" {
        SymbolKind::Struct
    } else {
        SymbolKind::Class
    }
}
