// SPDX-License-Identifier: Apache-2.0
//! Type declarations: aliases, structs, interfaces and grouped type specs.
//! Implements SPEC-MAP-MODE 5.1 Go type facts, including interface method members.
//! Depends on the walker and on reference scans for fields and embedded types.
//! Called from the file walker for every top-level `type` clause.
use super::super::{Item, Scope, Walker};
use crate::domain::DiagnosticReason;
use dctx_map_model::{NodeKind, SymbolKind};

impl Walker<'_> {
    pub(crate) fn type_decl(&mut self, index: usize, end: usize) -> usize {
        let next = index + 1;
        if self.is(next, "(") {
            let close = self.close_of(next, end);
            let mut cursor = next + 1;
            while cursor < close {
                cursor = self.skip_semis(cursor, close);
                if cursor >= close {
                    break;
                }
                let after = self.type_spec(index, cursor, close, true);
                if after <= cursor {
                    break;
                }
                cursor = after;
            }
            return self.consume_semi(close, end);
        }
        self.type_spec(index, next, end, false)
    }

    fn type_spec(&mut self, keyword_at: usize, name_at: usize, end: usize, grouped: bool) -> usize {
        if !self.can_be_name(name_at) {
            self.problem(self.start_byte(name_at), DiagnosticReason::Unrecognized);
            return self.skip_tail(name_at, end);
        }
        let bare = self.text(name_at).to_string();
        let mut cursor = name_at + 1;
        if self.is(cursor, "[") {
            cursor = self.after(cursor).min(end);
        }
        let alias = self.is(cursor, "=");
        if alias {
            cursor += 1;
        }
        let struct_kw = !alias && self.is(cursor, "struct");
        let iface_kw = !alias && self.is(cursor, "interface");
        let body_open = (struct_kw || iface_kw)
            .then_some(cursor + 1)
            .filter(|&brace| self.is(brace, "{"));
        let symbol = if struct_kw {
            SymbolKind::Struct
        } else if iface_kw {
            SymbolKind::Interface
        } else {
            SymbolKind::Type
        };
        let first = if grouped { name_at } else { keyword_at };
        if let Some(open) = body_open {
            let close = self.close_of(open, end);
            let item = Item {
                node_kind: NodeKind::Symbol,
                symbol: Some(symbol),
                local: bare.clone(),
                bare,
                first,
                last: close,
                sig_end: self.start_byte(open),
                body: (self.start_byte(open), self.end_byte(close)),
                doc_at: Some(if grouped { name_at } else { keyword_at }),
            };
            let inner = self.item(item, &Scope::default());
            if symbol == SymbolKind::Struct {
                self.scan_fields(open, close, inner.owner, &inner.name);
            } else {
                self.interface_body(open, close, &inner);
            }
            return self.consume_semi(close, end);
        }
        if struct_kw || iface_kw {
            self.problem(self.start_byte(cursor), DiagnosticReason::Unrecognized);
        }
        let type_end = self.skip_type(cursor, end, false, 0, "");
        if type_end <= cursor {
            self.problem(self.start_byte(cursor), DiagnosticReason::Unrecognized);
        }
        let last = type_end.saturating_sub(1).max(name_at);
        let end_byte = self.end_byte(last);
        let item = Item {
            node_kind: NodeKind::Symbol,
            symbol: Some(symbol),
            local: bare.clone(),
            bare,
            first,
            last,
            sig_end: end_byte,
            body: (self.start_byte(first), end_byte),
            doc_at: Some(if grouped { name_at } else { keyword_at }),
        };
        let inner = self.item(item, &Scope::default());
        let _ = self.skip_type(cursor, end, true, inner.owner, &inner.name);
        self.consume_semi(last, end)
    }

    fn interface_body(&mut self, open: usize, close: usize, scope: &Scope) {
        let mut index = open + 1;
        while index < close {
            index = self.skip_semis(index, close);
            if index >= close {
                break;
            }
            if self.can_be_name(index) && self.method_follows(index, close) {
                let next = self.method_decl(index, close, scope);
                index = next.max(index + 1);
                continue;
            }
            let next = self.scan_one_field(index, close, scope.owner, &scope.name, false);
            index = next.max(index + 1);
        }
    }

    fn method_follows(&self, index: usize, end: usize) -> bool {
        let mut cursor = index + 1;
        if self.is(cursor, "[") {
            cursor = self.after(cursor).min(end);
        }
        cursor < end && self.is(cursor, "(")
    }
}
