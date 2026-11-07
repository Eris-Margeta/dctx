// SPDX-License-Identifier: Apache-2.0
//! Members inside a type: bodies, enum constants, properties and constructors.
//! Constructor `val` and `var` parameters belong to the type, not the constructor.
//! Accessor keywords extend a property span and are not call targets.
use super::{Prefix, Scope, Walker};
use crate::domain::DiagnosticReason;
use dctx_map_model::SymbolKind;

impl Walker<'_> {
    pub(crate) fn class_body(&mut self, open: usize, scope: &Scope, enum_head: bool) {
        let Some(close) = self.matched(open, usize::MAX) else {
            return;
        };
        if self.depth >= 64 {
            self.problem(self.start_byte(open), DiagnosticReason::Malformed);
            return;
        }
        self.depth += 1;
        let mut index = open + 1;
        if enum_head {
            index = self.enum_constants(index, close, scope);
        }
        let mut guard = 0usize;
        while index < close && guard <= close {
            guard += 1;
            let next = self.declaration(index, close, scope, false);
            index = if next > index {
                next.min(close)
            } else {
                index + 1
            };
        }
        self.depth -= 1;
    }
    fn enum_constants(&mut self, mut index: usize, end: usize, scope: &Scope) -> usize {
        let mut guard = 0usize;
        while index < end && guard <= end {
            guard += 1;
            index = self.skip_soft(index, end);
            if index >= end {
                break;
            }
            if self.is_explicit_semi(index) {
                return index + 1;
            }
            if self.member_at(index, end) {
                return index;
            }
            let next = self.one_constant(index, end, scope);
            index = if next > index { next } else { index + 1 };
        }
        index
    }
    fn skip_soft(&self, mut index: usize, end: usize) -> usize {
        while index < end && (self.is(index, ",") || self.is_inserted_semi(index)) {
            index += 1;
        }
        index
    }
    fn member_at(&self, mut index: usize, end: usize) -> bool {
        let mut guard = 0usize;
        while index < end && guard < 24 {
            guard += 1;
            if self.is(index, "@") {
                let next = self.read_ann(index, end).to;
                if next <= index {
                    return false;
                }
                index = next;
                continue;
            }
            if self.modifier_word(index) {
                index += 1;
                continue;
            }
            return self.member_word(index);
        }
        false
    }
    fn member_word(&self, index: usize) -> bool {
        self.ident(index)
            && matches!(
                self.text(index),
                "fun"
                    | "val"
                    | "var"
                    | "class"
                    | "interface"
                    | "object"
                    | "typealias"
                    | "constructor"
                    | "init"
                    | "get"
                    | "set"
            )
    }
    fn one_constant(&mut self, index: usize, end: usize, scope: &Scope) -> usize {
        let mut cursor = index;
        let mut anns = Vec::new();
        while self.is(cursor, "@") {
            let ann = self.read_ann(cursor, end);
            if ann.to <= cursor {
                break;
            }
            cursor = ann.to;
            anns.push(ann);
        }
        if !self.is_decl_name(cursor) {
            return (cursor + 1).min(end);
        }
        let name = cursor;
        cursor += 1;
        let mut sig_end = self.end_byte(name);
        let mut args = None;
        if self.is(cursor, "(") {
            if let Some(close) = self.matched(cursor, end) {
                sig_end = self.end_byte(close);
                args = Some((cursor, close));
                cursor = close + 1;
            } else {
                sig_end = self.end_byte(cursor);
                let _ = self.emit_constant(name, cursor, sig_end, scope, &anns);
                return self.recover_open(cursor, end);
            }
        }
        let mut brace = None;
        if self.is(cursor, "{") {
            brace = Some(cursor);
            cursor = self.after(cursor).max(cursor + 1).min(end.max(cursor + 1));
        }
        let last = cursor.saturating_sub(1).max(name);
        if let Some(child) = self.emit_constant(name, last, sig_end, scope, &anns) {
            if let Some((open, close)) = args {
                self.scan_refs(open + 1, close, &child);
            }
            if let Some(open) = brace {
                self.class_body(open, &child, false);
            }
        }
        cursor
    }
    fn emit_constant(
        &mut self,
        name: usize,
        last: usize,
        sig_end: u32,
        scope: &Scope,
        anns: &[super::Ann],
    ) -> Option<Scope> {
        let child = self.emit(
            super::Symbol {
                kind: SymbolKind::Const,
                local: self.bare(name),
                first: name,
                last,
                sig_end,
                body: (self.start_byte(name), self.end_byte(last)),
                is_public: true,
            },
            scope,
        )?;
        self.flush_anns(&child, anns);
        Some(child)
    }
    pub(crate) fn property_decl(
        &mut self,
        begin: usize,
        prefix: &Prefix,
        end: usize,
        scope: &Scope,
    ) -> usize {
        let keyword_at = prefix.next;
        let Some(name_at) = self.find_prop_name(keyword_at + 1, end) else {
            self.problem(self.start_byte(keyword_at), DiagnosticReason::Malformed);
            return self.resume((keyword_at + 1).min(end), end);
        };
        let parsed = self.parse_property(name_at, end);
        let acc_end = self.accessor_limit(parsed.cursor, end);
        let last = acc_end.saturating_sub(1).max(name_at);
        let next = self.stop_or_finish(acc_end, end);
        let Some(child) = self.emit(
            super::Symbol {
                kind: SymbolKind::Const,
                local: self.bare(name_at),
                first: begin,
                last,
                sig_end: parsed.sig_end,
                body: (self.start_byte(begin), self.end_byte(last)),
                is_public: prefix.public,
            },
            scope,
        ) else {
            return next;
        };
        self.flush_anns(&child, &prefix.anns);
        if self.dot_between(keyword_at + 1, name_at) {
            self.emit_receiver(keyword_at + 1, name_at, &child);
        }
        if let Some((from, to)) = parsed.type_span {
            self.emit_type(from, to, &child);
        }
        if let Some((from, to)) = parsed.init {
            self.scan_refs(from, to, &child);
        }
        self.scan_accessors(parsed.cursor, acc_end, &child);
        next
    }
    fn parse_property(&self, name_at: usize, end: usize) -> PropParts {
        let mut cursor = name_at + 1;
        let mut sig_end = self.end_byte(name_at);
        let mut type_span = None;
        if self.is(cursor, ":") {
            let (next, byte) = self.type_tail(cursor + 1, end);
            sig_end = byte.max(sig_end);
            if next > cursor + 1 {
                type_span = Some((cursor + 1, next));
            }
            cursor = next;
        }
        let mut init = None;
        if self.is(cursor, "=") || self.is(cursor, "by") {
            let from = cursor + 1;
            let to = self.expr_end(from, end);
            if to > from {
                init = Some((from, to));
            }
            cursor = to;
        }
        PropParts {
            cursor,
            sig_end,
            type_span,
            init,
        }
    }
    fn find_prop_name(&self, start: usize, end: usize) -> Option<usize> {
        let mut found = None;
        let mut index = start;
        let mut depth = 0i32;
        while index < end {
            if depth == 0 && self.prop_stop(index, found.is_some()) {
                break;
            }
            if self.is(index, "(") && depth == 0 && self.receiver_paren(index, end) {
                index = self.after(index);
                continue;
            }
            if self.is(index, "(") || self.is(index, "[") || self.is(index, "{") {
                depth += 1;
            } else if self.is(index, ")") || self.is(index, "]") || self.is(index, "}") {
                depth = depth.saturating_sub(1);
            } else if self.is(index, "<") {
                if let Some(close) = self.angles_end(index, end) {
                    index = close + 1;
                    continue;
                }
            } else if depth == 0 && self.is_decl_name(index) {
                found = Some(index);
            }
            index += 1;
        }
        found
    }
    fn prop_stop(&self, index: usize, named: bool) -> bool {
        if self.is(index, ":")
            || self.is(index, "=")
            || self.is(index, "by")
            || self.is(index, "{")
            || self.is_semi(index)
            || self.decl_at(index)
        {
            return true;
        }
        named && (self.is(index, "get") || self.is(index, "set"))
    }
    fn accessor_limit(&self, index: usize, end: usize) -> usize {
        let mut cursor = index;
        let mut guard = 0usize;
        while guard < 6 {
            guard += 1;
            if self.accessor_word(cursor, end).is_none() {
                return cursor;
            }
            let next = self.skip_one_accessor(cursor, end);
            if next <= cursor {
                return cursor;
            }
            cursor = next;
        }
        cursor
    }
    fn accessor_word(&self, index: usize, end: usize) -> Option<usize> {
        let mut cursor = self.skip_semis(index, end);
        let mut guard = 0usize;
        while cursor < end && guard < 16 {
            guard += 1;
            if self.is(cursor, "@") {
                let next = self.read_ann(cursor, end).to;
                if next <= cursor {
                    return None;
                }
                cursor = next;
                continue;
            }
            if self.modifier_word(cursor) {
                cursor += 1;
                continue;
            }
            if self.is(cursor, "get") || self.is(cursor, "set") {
                return Some(cursor);
            }
            return None;
        }
        None
    }
    pub(crate) fn skip_one_accessor(&self, index: usize, end: usize) -> usize {
        let mut cursor = self.skip_semis(index, end);
        let mut guard = 0usize;
        while cursor < end && guard < 16 && (self.is(cursor, "@") || self.modifier_word(cursor)) {
            guard += 1;
            if self.is(cursor, "@") {
                let next = self.read_ann(cursor, end).to.max(cursor + 1);
                cursor = next;
            } else {
                cursor += 1;
            }
        }
        if !(self.is(cursor, "get") || self.is(cursor, "set")) {
            return index;
        }
        cursor += 1;
        if self.is(cursor, "(") {
            cursor = self.after(cursor).max(cursor + 1);
        }
        if self.is(cursor, ":") {
            cursor = self.type_tail(cursor + 1, end).0;
        }
        if self.is(cursor, "{") {
            cursor = self.after(cursor).max(cursor + 1);
        } else if self.is(cursor, "=") {
            cursor = self.expr_end(cursor + 1, end);
        }
        cursor.min(end.max(cursor))
    }
    pub(crate) fn scan_accessors(&mut self, start: usize, end: usize, scope: &Scope) {
        let mut cursor = start;
        let mut guard = 0usize;
        while cursor < end && guard < 6 {
            guard += 1;
            let Some(word) = self.accessor_word(cursor, end) else {
                return;
            };
            if word >= end {
                return;
            }
            self.scan_one_accessor(cursor, end, scope);
            let next = self.skip_one_accessor(cursor, end);
            if next <= cursor {
                return;
            }
            cursor = next;
        }
    }
    fn scan_one_accessor(&mut self, index: usize, end: usize, scope: &Scope) {
        let mut cursor = self.skip_semis(index, end);
        let mut anns = Vec::new();
        while cursor < end && (self.is(cursor, "@") || self.modifier_word(cursor)) {
            if self.is(cursor, "@") {
                let ann = self.read_ann(cursor, end);
                if ann.to <= cursor {
                    break;
                }
                cursor = ann.to;
                anns.push(ann);
            } else {
                cursor += 1;
            }
        }
        self.flush_anns(scope, &anns);
        if !(self.is(cursor, "get") || self.is(cursor, "set")) {
            return;
        }
        cursor += 1;
        if self.is(cursor, "(") {
            if let Some(close) = self.matched(cursor, end) {
                self.walk_params(cursor, close, scope, None);
            }
            cursor = self.after(cursor);
        }
        if self.is(cursor, ":") {
            let (next, _) = self.type_tail(cursor + 1, end);
            self.emit_type(cursor + 1, next, scope);
            cursor = next;
        }
        if self.is(cursor, "{") {
            if let Some(close) = self.matched(cursor, end) {
                self.scan_refs(cursor + 1, close, scope);
            }
        } else if self.is(cursor, "=") {
            let stop = self.expr_end(cursor + 1, end);
            self.scan_refs(cursor + 1, stop, scope);
        }
    }
    pub(crate) fn walk_params(
        &mut self,
        open: usize,
        close: usize,
        scope: &Scope,
        props: Option<&Scope>,
    ) {
        let mut index = open + 1;
        let mut guard = 0usize;
        while index < close && guard <= close {
            guard += 1;
            if self.is(index, ",") || self.is_semi(index) {
                index += 1;
                continue;
            }
            let next = self.one_param(index, close, scope, props);
            index = if next > index { next } else { index + 1 };
        }
    }
    fn one_param(
        &mut self,
        mut index: usize,
        close: usize,
        scope: &Scope,
        props: Option<&Scope>,
    ) -> usize {
        let mut anns = Vec::new();
        while self.is(index, "@") && index < close {
            let ann = self.read_ann(index, close);
            if ann.to <= index {
                break;
            }
            index = ann.to;
            anns.push(ann);
        }
        let mark = index;
        let (after, public, is_prop) = self.param_mods(index, close);
        index = after;
        if !self.is_decl_name(index) {
            let stop = self.param_end(index, close);
            self.flush_anns(scope, &anns);
            return if self.is(stop, ",") { stop + 1 } else { stop };
        }
        let name = index;
        index += 1;
        let mut sig_end = self.end_byte(name);
        let mut type_span = None;
        if self.is(index, ":") {
            let (next, byte) = self.type_tail(index + 1, close);
            sig_end = byte.max(sig_end);
            if next > index + 1 {
                type_span = Some((index + 1, next));
            }
            index = next;
        }
        let mut def = None;
        if self.is(index, "=") {
            let from = index + 1;
            let to = self.param_end(from, close);
            def = Some((from, to));
            index = to;
        }
        if is_prop {
            if let Some(owner) = props {
                self.emit_param_prop((mark, name, sig_end, public), owner, &anns, type_span, def);
            }
        } else {
            self.flush_anns(scope, &anns);
            if let Some((from, to)) = type_span {
                self.emit_type(from, to, scope);
            }
            if let Some((from, to)) = def {
                self.scan_refs(from, to, scope);
            }
        }
        if self.is(index, ",") {
            index + 1
        } else {
            index
        }
    }
    fn emit_param_prop(
        &mut self,
        head: (usize, usize, u32, bool),
        owner: &Scope,
        anns: &[super::Ann],
        type_span: Option<(usize, usize)>,
        def: Option<(usize, usize)>,
    ) {
        let (mark, name, sig_end, public) = head;
        let Some(child) = self.emit(
            super::Symbol {
                kind: SymbolKind::Const,
                local: self.bare(name),
                first: mark,
                last: name.max(mark),
                sig_end,
                body: (self.start_byte(mark), sig_end),
                is_public: public,
            },
            owner,
        ) else {
            return;
        };
        self.flush_anns(&child, anns);
        if let Some((from, to)) = type_span {
            self.emit_type(from, to, &child);
        }
        if let Some((from, to)) = def {
            self.scan_refs(from, to, &child);
        }
    }
    fn param_mods(&self, mut index: usize, end: usize) -> (usize, bool, bool) {
        let mut public = true;
        while index < end {
            if self.is(index, "val") || self.is(index, "var") {
                return (index + 1, public, true);
            }
            if !self.modifier_word(index) {
                break;
            }
            match self.text(index) {
                "private" | "protected" | "internal" => public = false,
                "public" => public = true,
                _ => {}
            }
            index += 1;
        }
        (index, public, false)
    }
}

struct PropParts {
    cursor: usize,
    sig_end: u32,
    type_span: Option<(usize, usize)>,
    init: Option<(usize, usize)>,
}
