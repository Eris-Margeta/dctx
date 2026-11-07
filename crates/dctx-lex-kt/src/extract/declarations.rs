// SPDX-License-Identifier: Apache-2.0
//! Top-level and member declarations: packages, imports, functions, aliases.
//! A package name is the module signature. Imports keep `*` and `as`.
//! An unclosed parameter list stops before the next declaration keyword.
use super::{Ann, Prefix, Scope, Walker};
use crate::domain::DiagnosticReason;
use dctx_map_model::{EdgeKind, SymbolKind};

const USE_SITE: &[&str] = &[
    "delegate", "field", "file", "get", "param", "property", "receiver", "set", "setparam",
];

impl Walker<'_> {
    pub(super) fn local_function(
        &mut self,
        index: usize,
        end: usize,
        scope: &Scope,
        next_candidate: &mut usize,
    ) -> Option<usize> {
        if self.is_semi(index) {
            *next_candidate = index + 1;
            return None;
        }
        let prefix = self.prefix(index, end);
        // Do not rescan the same annotation/modifier/semicolon run at each token.
        *next_candidate = prefix.next.max(index + 1);
        if !self.is(prefix.next, "fun") {
            return None;
        }
        // Anonymous functions are expressions, not named declarations.
        let (cursor, _) = self.fun_angles(prefix.next, end);
        self.find_fun_name(cursor, end)?;
        if self.depth >= 64 {
            self.problem(self.start_byte(index), DiagnosticReason::Malformed);
            return Some(end);
        }
        self.depth += 1;
        let next = self.function_decl(index, &prefix, end, scope);
        self.depth -= 1;
        Some(next.max(index + 1))
    }

    pub(crate) fn declaration(
        &mut self,
        index: usize,
        end: usize,
        scope: &Scope,
        file: bool,
    ) -> usize {
        let begin = self.skip_semis(index, end);
        if begin >= end || self.is(begin, "}") {
            return begin.min(end);
        }
        let prefix = self.prefix(begin, end);
        let at = prefix.next;
        if at >= end {
            return end;
        }
        self.dispatch(begin, &prefix, end, scope, file)
    }

    fn dispatch(
        &mut self,
        begin: usize,
        prefix: &Prefix,
        end: usize,
        scope: &Scope,
        file: bool,
    ) -> usize {
        let at = prefix.next;
        if file && self.is(at, "package") {
            self.flush_anns(scope, &prefix.anns);
            return self.package_decl(at, end);
        }
        if file && self.is(at, "import") {
            self.flush_anns(scope, &prefix.anns);
            return self.import_decl(at, end, scope);
        }
        if self.is(at, "class") || self.is(at, "interface") || self.is(at, "object") {
            return self.type_decl(begin, prefix, end, scope);
        }
        if self.is(at, "fun") {
            return self.function_decl(begin, prefix, end, scope);
        }
        if self.is(at, "val") || self.is(at, "var") {
            return self.property_decl(begin, prefix, end, scope);
        }
        if self.is(at, "typealias") {
            return self.typealias_decl(begin, prefix, end, scope);
        }
        if !file && self.is(at, "constructor") {
            return self.secondary_ctor(begin, prefix, end, scope);
        }
        if !file && self.is(at, "init") {
            return self.init_block(at, end, scope);
        }
        if !file && (self.is(at, "get") || self.is(at, "set")) {
            return self.skip_accessor(at, end, scope);
        }
        if prefix.next != begin {
            self.problem(self.start_byte(begin), DiagnosticReason::Malformed);
        }
        self.recover(at, end, scope)
    }

    fn prefix(&self, mut index: usize, end: usize) -> Prefix {
        let mut public = true;
        let mut data = false;
        let mut enum_kind = false;
        let mut annotation = false;
        let mut anns = Vec::new();
        while index < end {
            index = self.skip_semis(index, end);
            if index >= end {
                break;
            }
            if self.is(index, "@") {
                let ann = self.read_ann(index, end);
                if ann.to <= index {
                    break;
                }
                index = ann.to;
                anns.push(ann);
                continue;
            }
            if self.is(index, "fun") && self.is(index + 1, "interface") {
                index += 1;
                continue;
            }
            if !self.modifier_word(index) {
                break;
            }
            match self.text(index) {
                "private" | "protected" | "internal" => public = false,
                "public" => public = true,
                "data" => data = true,
                "enum" => enum_kind = true,
                "annotation" => annotation = true,
                _ => {}
            }
            index += 1;
        }
        Prefix {
            next: index,
            public,
            data,
            enum_kind,
            annotation,
            anns,
        }
    }

    pub(crate) fn read_ann(&self, index: usize, end: usize) -> Ann {
        let mut cursor = index + 1;
        if self.ident(cursor) && self.is(cursor + 1, ":") && use_site(self.text(cursor)) {
            cursor += 2;
        }
        let (name, after) = self.read_dotted(cursor, end);
        let mut to = after.max(index + 1);
        let mut args = None;
        if self.is(after, "(") {
            args = Some(after);
            to = self.after(after).max(after + 1);
        }
        Ann {
            name,
            at: index,
            to,
            args,
        }
    }

    pub(crate) fn flush_anns(&mut self, scope: &Scope, anns: &[Ann]) {
        for ann in anns {
            if ann.name.is_empty() {
                continue;
            }
            let last = ann.args.unwrap_or(ann.to).saturating_sub(1).max(ann.at);
            self.refer(
                scope,
                EdgeKind::UsesType,
                ann.name.clone(),
                None,
                ann.at,
                last,
            );
            if let Some(args) = ann.args {
                let close = self.matched(args, usize::MAX).unwrap_or(args);
                if close > args {
                    self.scan_refs(args + 1, close, scope);
                }
            }
        }
    }

    fn package_decl(&mut self, index: usize, end: usize) -> usize {
        let (name, next) = self.read_dotted(index + 1, end);
        if !name.is_empty() {
            self.draft.set_package(&name);
        }
        self.finish_stmt(next, end)
    }

    fn import_decl(&mut self, index: usize, end: usize, scope: &Scope) -> usize {
        let (name, next) = self.read_dotted(index + 1, end);
        let (alias, after) = self.import_alias(next, end);
        if !name.is_empty() {
            let last = after.saturating_sub(1).max(index + 1);
            self.refer(scope, EdgeKind::Imports, name, alias, index + 1, last);
        }
        self.finish_stmt(after, end)
    }

    fn import_alias(&self, index: usize, end: usize) -> (Option<String>, usize) {
        if self.text(index) != "as" {
            return (None, index);
        }
        let name_at = index + 1;
        if self.is_decl_name(name_at) {
            (Some(self.bare(name_at)), name_at + 1)
        } else {
            (None, name_at.min(end))
        }
    }

    pub(crate) fn function_decl(
        &mut self,
        begin: usize,
        prefix: &Prefix,
        end: usize,
        scope: &Scope,
    ) -> usize {
        let keyword_at = prefix.next;
        let (cursor, angles) = self.fun_angles(keyword_at, end);
        let Some(name_at) = self.find_fun_name(cursor, end) else {
            self.problem(self.start_byte(keyword_at), DiagnosticReason::Malformed);
            return self.resume(cursor, end);
        };
        let paren = name_at + 1;
        if !self.is(paren, "(") {
            self.problem(self.start_byte(keyword_at), DiagnosticReason::Malformed);
            return self.resume(paren.min(end), end);
        }
        let Some(close) = self.matched(paren, end) else {
            self.emit_short_fun(begin, prefix, name_at, paren, cursor, scope);
            return self.recover_open(paren, end);
        };
        let header = self.fun_after(close, end);
        let scan = self.body_scan(header.cursor, end);
        let (last, next, body) = self.fun_tail(header.cursor, end, header.sig_end, keyword_at);
        self.finish_fun(&FinishedFun {
            begin,
            prefix,
            name_at,
            last,
            sig_end: header.sig_end,
            body,
            angles,
            paren,
            close,
            ret: header.ret,
            where_clause: header.where_clause,
            scan,
            receiver_from: cursor,
            scope,
        });
        next
    }

    fn emit_short_fun(
        &mut self,
        begin: usize,
        prefix: &Prefix,
        name_at: usize,
        paren: usize,
        receiver_from: usize,
        scope: &Scope,
    ) {
        let sig_end = self.end_byte(paren);
        let body = (self.start_byte(paren), sig_end);
        let Some(child) = self.emit(
            self.fun_symbol(begin, prefix, name_at, (paren, sig_end, body), scope),
            scope,
        ) else {
            return;
        };
        self.flush_anns(&child, &prefix.anns);
        if self.dot_between(receiver_from, name_at) {
            self.emit_receiver(receiver_from, name_at, &child);
        }
    }

    fn finish_fun(&mut self, fun: &FinishedFun<'_, '_>) {
        let Some(child) = self.emit(
            self.fun_symbol(
                fun.begin,
                fun.prefix,
                fun.name_at,
                (fun.last, fun.sig_end, fun.body),
                fun.scope,
            ),
            fun.scope,
        ) else {
            return;
        };
        self.flush_anns(&child, &fun.prefix.anns);
        if let Some((from, to)) = fun.angles {
            self.colon_types(from, to, &child);
        }
        if self.dot_between(fun.receiver_from, fun.name_at) {
            self.emit_receiver(fun.receiver_from, fun.name_at, &child);
        }
        self.walk_params(fun.paren, fun.close, &child, None);
        if let Some((from, to)) = fun.ret {
            self.emit_type(from, to, &child);
        }
        if let Some((from, to)) = fun.where_clause {
            self.colon_types(from, to, &child);
        }
        if let Some((from, to)) = fun.scan {
            self.scan_refs(from, to, &child);
        }
    }

    fn fun_symbol(
        &self,
        begin: usize,
        prefix: &Prefix,
        name_at: usize,
        span: (usize, u32, (u32, u32)),
        scope: &Scope,
    ) -> super::Symbol {
        let (last, sig_end, body) = span;
        let local = matches!(
            self.draft.nodes[scope.owner].node.symbol_kind,
            Some(SymbolKind::Function | SymbolKind::Method)
        );
        super::Symbol {
            kind: if scope.name.is_empty() || local {
                SymbolKind::Function
            } else {
                SymbolKind::Method
            },
            local: self.bare(name_at),
            first: begin,
            last,
            sig_end,
            body,
            is_public: prefix.public && !local,
        }
    }

    fn fun_angles(&self, keyword_at: usize, end: usize) -> (usize, Option<(usize, usize)>) {
        let cursor = keyword_at + 1;
        if self.is(cursor, "<")
            && let Some(close) = self.angles_end(cursor, end)
        {
            return (close + 1, Some((cursor, close)));
        }
        (cursor, None)
    }

    fn fun_after(&self, close: usize, end: usize) -> FunHeader {
        let mut cursor = close + 1;
        let mut sig_end = self.end_byte(close);
        let mut ret = None;
        if self.is(cursor, ":") {
            let (next, byte) = self.type_tail(cursor + 1, end);
            sig_end = byte.max(sig_end);
            if next > cursor + 1 {
                ret = Some((cursor + 1, next));
            }
            cursor = next;
        }
        let mut where_clause = None;
        if self.is(cursor, "where") {
            let next = self.clause_end(cursor, end);
            sig_end = self.end_byte(next.saturating_sub(1).max(cursor));
            where_clause = Some((cursor, next));
            cursor = next;
        }
        FunHeader {
            cursor,
            sig_end,
            ret,
            where_clause,
        }
    }

    fn fun_tail(
        &self,
        cursor: usize,
        end: usize,
        sig_end: u32,
        keyword_at: usize,
    ) -> (usize, usize, (u32, u32)) {
        if self.is(cursor, "{") {
            let after = self.after(cursor).max(cursor + 1).min(end.max(cursor + 1));
            let last = after.saturating_sub(1).max(cursor);
            return (
                last,
                after.min(end.max(after)),
                (self.start_byte(cursor), self.end_byte(last)),
            );
        }
        if self.is(cursor, "=") {
            let stop = self.expr_end(cursor + 1, end);
            let last = stop.saturating_sub(1).max(cursor);
            let next = self.stop_or_finish(stop, end);
            return (last, next, (self.start_byte(cursor), self.end_byte(last)));
        }
        let last = cursor.saturating_sub(1).max(keyword_at);
        (
            last,
            self.stop_or_finish(cursor, end),
            (self.start_byte(keyword_at), sig_end),
        )
    }

    fn body_scan(&self, cursor: usize, end: usize) -> Option<(usize, usize)> {
        if self.is(cursor, "{") {
            let close = self.matched(cursor, end)?;
            return (cursor + 1 < close).then_some((cursor + 1, close));
        }
        if self.is(cursor, "=") {
            let stop = self.expr_end(cursor + 1, end);
            return (stop > cursor + 1).then_some((cursor + 1, stop));
        }
        None
    }

    pub(crate) fn stop_or_finish(&self, index: usize, end: usize) -> usize {
        if index < end && (self.is(index, "}") || self.decl_at(index)) {
            index
        } else {
            self.finish_stmt(index, end)
        }
    }

    fn find_fun_name(&self, start: usize, end: usize) -> Option<usize> {
        let mut depth = 0i32;
        let mut found = None;
        let mut index = start;
        while index < end {
            if depth == 0 && self.fun_name_stop(index) {
                break;
            }
            if self.is(index, "(") && depth == 0 {
                if self.receiver_paren(index, end) {
                    index = self.after(index);
                    continue;
                }
                return found;
            }
            depth = self.fun_depth(index, end, depth);
            if self.is(index, "<") {
                if let Some(close) = self.angles_end(index, end) {
                    index = close + 1;
                    continue;
                }
            } else if depth == 0 && self.is_decl_name(index) {
                found = Some(index);
            }
            index += 1;
        }
        None
    }

    fn fun_depth(&self, index: usize, _end: usize, depth: i32) -> i32 {
        if self.is(index, "(") || self.is(index, "[") || self.is(index, "{") {
            depth + 1
        } else if self.is(index, ")") || self.is(index, "]") || self.is(index, "}") {
            depth - 1
        } else {
            depth
        }
    }

    fn fun_name_stop(&self, index: usize) -> bool {
        self.is_semi(index) || self.is(index, "{") || self.is(index, "=") || self.decl_at(index)
    }

    pub(crate) fn receiver_paren(&self, open: usize, end: usize) -> bool {
        let Some(close) = self.matched(open, end) else {
            return false;
        };
        self.is(close + 1, ".") || self.is(close + 1, "?")
    }

    fn typealias_decl(
        &mut self,
        begin: usize,
        prefix: &Prefix,
        end: usize,
        scope: &Scope,
    ) -> usize {
        let keyword_at = prefix.next;
        let name_at = keyword_at + 1;
        if !self.is_decl_name(name_at) {
            self.problem(self.start_byte(keyword_at), DiagnosticReason::Malformed);
            return self.resume(name_at.min(end), end);
        }
        let mut cursor = name_at + 1;
        let mut angles = None;
        if self.is(cursor, "<")
            && let Some(close) = self.angles_end(cursor, end)
        {
            angles = Some((cursor, close));
            cursor = close + 1;
        }
        if !self.is(cursor, "=") {
            self.problem(self.start_byte(keyword_at), DiagnosticReason::Malformed);
            return self.resume(cursor, end);
        }
        let (next, sig_end) = self.type_tail(cursor + 1, end);
        let last = next.saturating_sub(1).max(name_at);
        if let Some(child) = self.emit(
            super::Symbol {
                kind: SymbolKind::Type,
                local: self.bare(name_at),
                first: begin,
                last,
                sig_end,
                body: (self.start_byte(begin), sig_end),
                is_public: prefix.public,
            },
            scope,
        ) {
            self.flush_anns(&child, &prefix.anns);
            if let Some((from, to)) = angles {
                self.colon_types(from, to, &child);
            }
            self.emit_type(cursor + 1, next, &child);
        }
        self.stop_or_finish(next, end)
    }

    pub(crate) fn recover(&mut self, index: usize, end: usize, scope: &Scope) -> usize {
        if index >= end || self.is(index, "}") {
            return index.min(end);
        }
        if self.is(index, "{") || self.is(index, "(") || self.is(index, "[") {
            let after = self.after(index).max(index + 1);
            let inner = after.saturating_sub(1);
            if inner > index {
                self.scan_refs(index + 1, inner.min(end), scope);
            }
            return after.min(end.max(after));
        }
        if self.decl_at(index) {
            self.problem(self.start_byte(index), DiagnosticReason::Malformed);
            return (index + 1).min(end);
        }
        let stop = self.expr_end(index, end);
        let stop = if stop <= index {
            (index + 1).min(end)
        } else {
            stop.min(end)
        };
        self.scan_refs(index, stop, scope);
        self.stop_or_finish(stop, end)
    }
}

struct FunHeader {
    cursor: usize,
    sig_end: u32,
    ret: Option<(usize, usize)>,
    where_clause: Option<(usize, usize)>,
}

struct FinishedFun<'p, 's> {
    begin: usize,
    prefix: &'p Prefix,
    name_at: usize,
    last: usize,
    sig_end: u32,
    body: (u32, u32),
    angles: Option<(usize, usize)>,
    paren: usize,
    close: usize,
    ret: Option<(usize, usize)>,
    where_clause: Option<(usize, usize)>,
    scan: Option<(usize, usize)>,
    receiver_from: usize,
    scope: &'s Scope,
}

fn use_site(word: &str) -> bool {
    USE_SITE.binary_search(&word).is_ok()
}

#[cfg(test)]
mod tests {
    use super::super::is_modifier;
    use super::USE_SITE;
    use crate::lexer::keyword;

    #[test]
    fn use_site_targets_are_sorted() {
        assert!(USE_SITE.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(super::use_site("file") && !super::use_site("class"));
        assert!(is_modifier("sealed") && !keyword("sealed"));
    }
}
