//! Usings, namespaces and type declarations.
//! Implements SPEC-MAP-MODE 5.1 for C# compilation units and type bodies.
//! The member walker and the type grammar live in sibling modules.
use super::declarations_types::visible;
use super::{Body, DeclKind, Emit, Prefix, Scope, ScopeKind, Walker};
use crate::domain::DiagnosticReason;
use dctx_map_model::{EdgeKind, SymbolKind};

impl Walker<'_> {
    pub(crate) fn using_decl(&mut self, index: usize, end: usize, scope: &Scope) -> usize {
        let mut cursor = index;
        if self.is(cursor, "global") {
            cursor += 1;
        }
        if !self.is(cursor, "using") {
            return self.statement(index, end, scope);
        }
        cursor += 1;
        let mut is_static = false;
        if self.is(cursor, "static") {
            is_static = true;
            cursor += 1;
        }
        let Some(name_end) = self.dotted_end(cursor, end) else {
            return self.statement(index, end, scope);
        };
        if self.is(name_end, "=") {
            let alias = self.text(cursor).trim_start_matches('@').to_string();
            let type_at = name_end + 1;
            let semi = self.semi_at(type_at, end);
            let last = semi.saturating_sub(1).max(type_at);
            let target = self.span_text(type_at, last);
            self.refer(scope.owner, &scope.name, EdgeKind::Imports, target, Some(alias), type_at, last);
            return (semi + 1).min(end);
        }
        if self.is(name_end, ";") {
            let last = name_end.saturating_sub(1);
            let name = self.span_text(cursor, last);
            let alias = is_static.then(|| "static".to_string());
            self.refer(scope.owner, &scope.name, EdgeKind::Imports, name, alias, cursor, last);
            return name_end + 1;
        }
        self.statement(index, end, scope)
    }

    pub(crate) fn namespace_decl(&mut self, index: usize, end: usize, scope: &Scope) -> usize {
        let Some(name_end) = self.dotted_end(index + 1, end) else {
            self.problem(self.start_byte(index), DiagnosticReason::Malformed);
            return self.recover(index, end);
        };
        let local = self.span_text(index + 1, name_end.saturating_sub(1));
        let cursor = name_end;
        let (body_from, body_to, last, next, brace_body) = if self.is(cursor, ";") {
            (cursor + 1, end, end.saturating_sub(1).max(cursor), end, false)
        } else if self.is(cursor, "{") {
            let close = self.close_of(cursor, end);
            (cursor + 1, close, close, self.after(cursor, end), true)
        } else {
            self.problem(self.start_byte(index), DiagnosticReason::Malformed);
            return self.recover(index, end);
        };
        let sig_end = self.end_byte(name_end.saturating_sub(1));
        let body = if brace_body {
            (self.start_byte(cursor), self.end_byte(last))
        } else {
            (self.start_byte(index), self.end_byte(last))
        };
        let child = self.emit(
            &Emit {
                symbol: None,
                local: &local,
                simple: &local,
                scope_kind: ScopeKind::Namespace,
                first: index,
                last,
                sig_end,
                body,
                is_public: true,
                namespace: true,
            },
            scope,
        );
        if let Some(child) = child {
            self.body(body_from, body_to, &child, Body::Namespace);
        }
        next
    }

    pub(crate) fn type_decl(&mut self, index: usize, end: usize, scope: &Scope) -> usize {
        let prefix = self.prefix(index, end);
        let Some(kind) = self.decl_kind(prefix.next) else {
            self.problem(self.start_byte(index), DiagnosticReason::Malformed);
            return self.recover(index, end);
        };
        self.finish_type(index, &prefix, kind, end, scope)
    }

    fn finish_type(
        &mut self,
        index: usize,
        prefix: &Prefix,
        kind: DeclKind,
        end: usize,
        scope: &Scope,
    ) -> usize {
        let mut cursor = prefix.next + 1;
        let mut record_struct = false;
        if kind == DeclKind::Record {
            if self.is(cursor, "struct") {
                record_struct = true;
                cursor += 1;
            } else if self.is(cursor, "class") {
                cursor += 1;
            }
        }
        if !self.can_name(cursor) {
            self.problem(self.start_byte(index), DiagnosticReason::Malformed);
            return self.recover(index, end);
        }
        let name_at = cursor;
        let local = self.text(name_at).trim_start_matches('@').to_string();
        cursor += 1;
        let mut params = Vec::new();
        if self.is(cursor, "<")
            && let Some(close) = self.close_angle(cursor, end)
        {
            params = self.type_param_names(cursor, close);
            cursor = close + 1;
        }
        let mut param_open = None;
        if matches!(kind, DeclKind::Class | DeclKind::Struct | DeclKind::Record) && self.is(cursor, "(") {
            param_open = Some(cursor);
            cursor = self.after(cursor, end);
        }
        let header_end = self.header_end(cursor, end);
        let (body_from, body_to, last, next) = self.type_body_span(header_end, end, name_at);
        let sig_last = header_end.saturating_sub(1).max(name_at).min(last);
        let body = if self.is(header_end, "{") {
            (self.start_byte(header_end), self.end_byte(last))
        } else {
            (self.start_byte(index), self.end_byte(sig_last))
        };
        let child = self.emit(
            &Emit {
                symbol: Some(symbol_of(kind)),
                local: &local,
                simple: &local,
                scope_kind: scope_of(kind),
                first: index,
                last,
                sig_end: self.end_byte(sig_last),
                body,
                is_public: visible(scope, prefix, false),
                namespace: false,
            },
            scope,
        );
        if let Some(child) = child {
            self.attr_uses(prefix, &child);
            self.header_types(prefix.next, header_end, &child, kind, record_struct, &params);
            if let Some(open) = param_open {
                self.param_types(open, &child, &params);
                if kind == DeclKind::Record {
                    self.record_components(open, &child);
                }
            }
            if body_to > body_from {
                if kind == DeclKind::Enum {
                    self.enum_body(body_from, body_to, &child);
                } else {
                    self.body(body_from, body_to, &child, Body::Type);
                }
            }
        }
        next
    }

    fn header_end(&self, mut cursor: usize, end: usize) -> usize {
        if self.is(cursor, ":") {
            cursor += 1;
            while cursor < end && !self.stops_header(cursor) {
                cursor += 1;
            }
        }
        while self.is(cursor, "where") {
            cursor += 1;
            while cursor < end && !self.stops_header(cursor) && !self.is(cursor, "where") {
                cursor += 1;
            }
        }
        cursor
    }

    fn stops_header(&self, index: usize) -> bool {
        self.is(index, "{") || self.is(index, ";") || self.is(index, "where")
    }

    fn type_body_span(&mut self, cursor: usize, end: usize, name_at: usize) -> (usize, usize, usize, usize) {
        if self.is(cursor, "{") {
            if let Some(close) = self.partner_within(cursor, end) {
                return (cursor + 1, close, close, close + 1);
            }
            let last = end.saturating_sub(1).max(cursor);
            return (cursor + 1, end, last, end);
        }
        if self.is(cursor, ";") {
            return (0, 0, cursor, (cursor + 1).min(end));
        }
        self.problem(self.start_byte(cursor.max(name_at)), DiagnosticReason::Malformed);
        let next = self.recover(cursor, end);
        (0, 0, cursor.saturating_sub(1).max(name_at), next)
    }

    fn header_types(
        &mut self,
        from: usize,
        to: usize,
        scope: &Scope,
        kind: DeclKind,
        record_struct: bool,
        params: &[String],
    ) {
        let mut index = from;
        while index < to && !self.is(index, ":") && !self.is(index, "where") {
            index += 1;
        }
        if self.is(index, ":") {
            let base = index + 1;
            let mut end = base;
            while end < to && !self.is(end, "where") {
                end += 1;
            }
            self.base_list(base, end, scope, kind, record_struct);
            index = end;
        }
        if self.is(index, "where") {
            self.uses_from(index, to, scope, params);
        }
    }

    fn record_components(&mut self, open: usize, scope: &Scope) {
        let Some(close) = self.partner_within(open, self.toks.len()) else {
            return;
        };
        for (start, end) in self.split_commas(open + 1, close) {
            let cursor = self.skip_param_prefix(start, end);
            let Some(after) = self.parse_type_end(cursor, end) else {
                continue;
            };
            if !self.can_name(after) {
                continue;
            }
            let local = self.text(after).trim_start_matches('@').to_string();
            let child = self.emit(
                &Emit {
                    symbol: Some(SymbolKind::Const),
                    local: &local,
                    simple: &local,
                    scope_kind: ScopeKind::Member,
                    first: cursor,
                    last: after,
                    sig_end: self.end_byte(after),
                    body: (self.start_byte(cursor), self.end_byte(after)),
                    is_public: false,
                    namespace: false,
                },
                scope,
            );
            if let Some(child) = child {
                self.uses_from(cursor, after, &child, &[]);
            }
        }
    }

    fn enum_body(&mut self, mut index: usize, end: usize, scope: &Scope) {
        while index < end && !self.draft.overflow {
            index = self.skip_noise(index, end);
            if index >= end {
                break;
            }
            let prefix = self.prefix(index, end);
            let at = prefix.next;
            if at >= end {
                break;
            }
            if self.is(at, ";") {
                self.body(at + 1, end, scope, Body::Type);
                return;
            }
            if !self.can_name(at) {
                index = self.recover(at, end);
                continue;
            }
            index = self.enum_constant(index, at, end, scope, &prefix);
        }
    }

    fn enum_constant(
        &mut self,
        index: usize,
        at: usize,
        end: usize,
        scope: &Scope,
        prefix: &Prefix,
    ) -> usize {
        let local = self.text(at).trim_start_matches('@').to_string();
        let mut cursor = at + 1;
        if self.is(cursor, "=") {
            cursor += 1;
            while cursor < end && !self.is(cursor, ",") && !self.is(cursor, "{") && !self.is(cursor, ";") {
                if self.is(cursor, "(") || self.is(cursor, "[") {
                    cursor = self.after(cursor, end);
                } else {
                    cursor += 1;
                }
            }
        }
        let brace = self.is(cursor, "{").then(|| self.close_of(cursor, end));
        let last = brace.unwrap_or(at);
        let body = brace
            .map_or((self.start_byte(at), self.end_byte(at)), |close| (self.start_byte(cursor), self.end_byte(close)));
        let child = self.emit(
            &Emit {
                symbol: Some(SymbolKind::Const),
                local: &local,
                simple: &local,
                scope_kind: ScopeKind::Member,
                first: at,
                last,
                sig_end: self.end_byte(at),
                body,
                is_public: visible(scope, prefix, true),
                namespace: false,
            },
            scope,
        );
        if let Some(child) = child {
            self.attr_uses(prefix, &child);
            if let Some(close) = brace {
                self.body(cursor + 1, close, &child, Body::Type);
                cursor = self.after(cursor, end);
            }
        } else if let Some(close) = brace {
            cursor = close + 1;
        }
        if self.is(cursor, ",") {
            cursor += 1;
        }
        cursor.max(index + 1)
    }

    fn semi_at(&self, mut index: usize, end: usize) -> usize {
        while index < end && !self.is(index, ";") {
            if self.is(index, "(") || self.is(index, "[") || self.is(index, "{") {
                index = self.after(index, end);
            } else {
                index += 1;
            }
        }
        index.min(end.saturating_sub(1))
    }
}

fn symbol_of(kind: DeclKind) -> SymbolKind {
    match kind {
        DeclKind::Class => SymbolKind::Class,
        DeclKind::Struct | DeclKind::Record => SymbolKind::Struct,
        DeclKind::Interface => SymbolKind::Interface,
        DeclKind::Enum => SymbolKind::Enum,
    }
}

fn scope_of(kind: DeclKind) -> ScopeKind {
    match kind {
        DeclKind::Class => ScopeKind::Class,
        DeclKind::Struct => ScopeKind::Struct,
        DeclKind::Interface => ScopeKind::Interface,
        DeclKind::Enum => ScopeKind::Enum,
        DeclKind::Record => ScopeKind::Record,
    }
}
