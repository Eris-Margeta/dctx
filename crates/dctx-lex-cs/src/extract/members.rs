// SPDX-License-Identifier: Apache-2.0
//! Fields, properties, methods and constructors inside a type body.
//! Implements SPEC-MAP-MODE 5.1 member facts for C#. Signatures stop at the name
//! or the parameter list; bodies are scanned for calls, not nested statements.
use super::declarations_types::visible;
use super::{Emit, Prefix, Scope, ScopeKind, Walker};
use crate::domain::{slice, DiagnosticReason};
use dctx_map_model::SymbolKind;

struct MethodAt {
    start: usize,
    type_from: usize,
    name: usize,
    ctor: bool,
}

struct MethodTail {
    last: usize,
    body: (u32, u32),
    next: usize,
    scan: Option<(usize, usize)>,
}

impl Walker<'_> {
    pub(crate) fn member(&mut self, index: usize, end: usize, scope: &Scope) -> usize {
        if index >= end {
            return end;
        }
        if self.is(index, ";") || self.kind(index) == Some(crate::lexer::Kind::Preproc) {
            return index + 1;
        }
        let prefix = self.prefix(index, end);
        let at = prefix.next;
        if at >= end {
            self.attr_uses(&prefix, scope);
            return end;
        }
        if self.decl_kind(at).is_some() {
            return self.type_decl(index, end, scope);
        }
        if self.is(at, "{") {
            self.problem(self.start_byte(at), DiagnosticReason::Malformed);
            self.scan_calls(at + 1, self.close_of(at, end), scope);
            return self.after(at, end);
        }
        if self.abstain_member(at) {
            self.problem(self.start_byte(at), DiagnosticReason::Malformed);
            return self.recover(at, end);
        }
        if self.can_name(at) && self.text(at) == scope.simple && self.is(at + 1, "(") {
            return self.finish_method(
                &MethodAt { start: index, type_from: at, name: at, ctor: true },
                end,
                scope,
                &prefix,
            );
        }
        if let Some(after_type) = self.parse_type_end(at, end)
            && self.can_name(after_type)
        {
            return self.finish_named(index, at, after_type, end, scope, &prefix);
        }
        self.problem(self.start_byte(at), DiagnosticReason::Malformed);
        self.recover(at, end)
    }

    fn abstain_member(&self, index: usize) -> bool {
        self.is(index, "~")
            || self.is(index, "operator")
            || self.is(index, "implicit")
            || self.is(index, "explicit")
            || (self.is(index, "this") && self.is(index + 1, "["))
    }

    fn finish_named(
        &mut self,
        start: usize,
        type_at: usize,
        name: usize,
        end: usize,
        scope: &Scope,
        prefix: &Prefix,
    ) -> usize {
        let mut after = name + 1;
        if self.is(after, "<")
            && let Some(close) = self.close_angle(after, end)
        {
            after = close + 1;
        }
        if self.is(after, "(") {
            return self.finish_method(
                &MethodAt { start, type_from: type_at, name, ctor: false },
                end,
                scope,
                prefix,
            );
        }
        if self.is(after, "{") || self.is(after, "=>") {
            return self.finish_property(start, type_at, name, after, end, scope, prefix);
        }
        if after >= end || self.is(after, ";") || self.is(after, "=") || self.is(after, ",") {
            return self.finish_field(start, type_at, name, end, scope, prefix);
        }
        self.problem(self.start_byte(name), DiagnosticReason::Malformed);
        self.recover(start, end)
    }

    fn finish_field(
        &mut self,
        start: usize,
        type_at: usize,
        mut name: usize,
        end: usize,
        scope: &Scope,
        prefix: &Prefix,
    ) -> usize {
        let mut cursor = name + 1;
        loop {
            let local = self.text(name).trim_start_matches('@').to_string();
            let emitted = self.emit(
                &Emit {
                    symbol: Some(SymbolKind::Const),
                    local: &local,
                    simple: scope.simple.as_str(),
                    scope_kind: ScopeKind::Member,
                    first: start,
                    last: name,
                    sig_end: self.end_byte(name),
                    body: (self.start_byte(start), self.end_byte(name)),
                    is_public: visible(scope, prefix, false),
                    namespace: false,
                },
                scope,
            );
            if let Some(child) = &emitted {
                self.uses_from(type_at, name, child, &[]);
                self.attr_uses(prefix, child);
            }
            if self.is(cursor, "=") {
                let expr = cursor + 1;
                let stop = self.separator(expr, end);
                if let Some(child) = &emitted {
                    self.scan_calls(expr, stop, child);
                    let text = slice(self.src, self.end_byte(cursor), self.start_byte(stop));
                    self.set_body(child.owner, text);
                }
                cursor = stop;
            }
            if self.is(cursor, ",") && self.can_name(cursor + 1) {
                name = cursor + 1;
                cursor = name + 1;
                continue;
            }
            break;
        }
        if self.is(cursor, ";") { cursor + 1 } else { cursor.max(start + 1) }
    }

    #[allow(clippy::too_many_arguments)]
    fn finish_property(
        &mut self,
        start: usize,
        type_at: usize,
        name: usize,
        after: usize,
        end: usize,
        scope: &Scope,
        prefix: &Prefix,
    ) -> usize {
        let local = self.text(name).trim_start_matches('@').to_string();
        let (last, body, next, scan) = if self.is(after, "{") {
            let close = self.close_of(after, end);
            let mut cursor = self.after(after, end);
            let init = self.is(cursor, "=").then(|| {
                let expr = cursor + 1;
                let stop = self.separator(expr, end);
                (cursor, stop)
            });
            if let Some((_, stop)) = init {
                cursor = stop;
            }
            if self.is(cursor, ";") {
                cursor += 1;
            }
            let body = init
                .map_or((self.start_byte(after), self.end_byte(close)), |(eq, stop)| (self.end_byte(eq), self.start_byte(stop)));
            (close, body, cursor, Some((after + 1, close)))
        } else {
            let expr = after + 1;
            let stop = self.separator(expr, end);
            let last = stop.saturating_sub(1).max(after);
            let body = (self.start_byte(expr), self.start_byte(stop.min(end)));
            (last, body, if self.is(stop, ";") { stop + 1 } else { stop }, Some((expr, stop)))
        };
        let child = self.emit(
            &Emit {
                symbol: Some(SymbolKind::Const),
                local: &local,
                simple: scope.simple.as_str(),
                scope_kind: ScopeKind::Member,
                first: start,
                last,
                sig_end: self.end_byte(name),
                body,
                is_public: visible(scope, prefix, false),
                namespace: false,
            },
            scope,
        );
        if let Some(child) = child {
            self.uses_from(type_at, name, &child, &[]);
            self.attr_uses(prefix, &child);
            if let Some((from, to)) = scan {
                self.scan_calls(from, to, &child);
            }
        }
        next.max(start + 1)
    }

    fn finish_method(
        &mut self,
        at: &MethodAt,
        end: usize,
        scope: &Scope,
        prefix: &Prefix,
    ) -> usize {
        let mut cursor = at.name + 1;
        let mut params = Vec::new();
        if !at.ctor
            && self.is(cursor, "<")
            && let Some(close) = self.close_angle(cursor, end)
        {
            params = self.type_param_names(cursor, close);
            cursor = close + 1;
        }
        if !self.is(cursor, "(") {
            self.problem(self.start_byte(at.name), DiagnosticReason::Malformed);
            return self.recover(at.start, end);
        }
        let paren = cursor;
        let close = self.close_of(paren, end);
        let mut sig_last = close;
        cursor = (close + 1).min(end);
        let mut init = None;
        if at.ctor && self.is(cursor, ":") {
            let mark = cursor;
            cursor += 1;
            if self.is(cursor, "base") || self.is(cursor, "this") {
                let callee = cursor;
                cursor += 1;
                if self.is(cursor, "(")
                    && let Some(end_paren) = self.partner_within(cursor, end)
                {
                    init = Some((callee, end_paren));
                    sig_last = end_paren;
                    cursor = end_paren + 1;
                }
            } else {
                cursor = mark;
            }
        }
        while self.is(cursor, "where") {
            cursor += 1;
            while cursor < end && !self.is(cursor, "{") && !self.is(cursor, "=>") && !self.is(cursor, ";") {
                cursor += 1;
            }
            if cursor > 0 {
                sig_last = cursor - 1;
            }
        }
        let local = self.text(at.name).trim_start_matches('@').to_string();
        let tail = self.method_tail(at.start, cursor, sig_last, end);
        let child = self.emit(
            &Emit {
                symbol: Some(SymbolKind::Method),
                local: &local,
                simple: scope.simple.as_str(),
                scope_kind: ScopeKind::Member,
                first: at.start,
                last: tail.last,
                sig_end: self.end_byte(sig_last),
                body: tail.body,
                is_public: visible(scope, prefix, false),
                namespace: false,
            },
            scope,
        );
        if let Some(child) = child {
            if !at.ctor {
                self.uses_from(at.type_from, at.name, &child, &params);
            }
            self.param_types(paren, &child, &params);
            if cursor > close + 1 {
                self.uses_from(close + 1, cursor, &child, &params);
            }
            self.attr_uses(prefix, &child);
            if let Some((from, to)) = init {
                self.scan_calls(from, to, &child);
            }
            if let Some((from, to)) = tail.scan {
                self.scan_calls(from, to, &child);
            }
        }
        tail.next
    }

    fn method_tail(&mut self, start: usize, cursor: usize, sig_last: usize, end: usize) -> MethodTail {
        if self.is(cursor, "{") {
            let close = self.close_of(cursor, end);
            return MethodTail {
                last: close,
                body: (self.start_byte(cursor), self.end_byte(close)),
                next: self.after(cursor, end),
                scan: Some((cursor + 1, close)),
            };
        }
        if self.is(cursor, "=>") {
            let expr = cursor + 1;
            let stop = self.separator(expr, end);
            let next = if self.is(stop, ";") { stop + 1 } else { stop };
            return MethodTail {
                last: stop.saturating_sub(1).max(cursor),
                body: (self.start_byte(expr), self.start_byte(stop.min(end))),
                next,
                scan: Some((expr, stop)),
            };
        }
        if self.is(cursor, ";") {
            return MethodTail {
                last: cursor,
                body: (self.start_byte(start), self.end_byte(sig_last)),
                next: (cursor + 1).min(end),
                scan: None,
            };
        }
        self.problem(self.start_byte(cursor.max(start)), DiagnosticReason::Malformed);
        MethodTail {
            last: sig_last,
            body: (self.start_byte(start), self.end_byte(sig_last)),
            next: cursor.max(start + 1),
            scan: None,
        }
    }

    fn separator(&self, mut index: usize, end: usize) -> usize {
        let mut angle = 0i32;
        while index < end {
            if self.is(index, "(") || self.is(index, "[") || self.is(index, "{") {
                index = self.after(index, end);
                continue;
            }
            if self.is(index, "<") {
                angle += 1;
            } else if self.is(index, ">") {
                angle -= 1;
            } else if (self.is(index, ";") || self.is(index, ",")) && angle <= 0 {
                return index;
            }
            index += 1;
        }
        end
    }
}
