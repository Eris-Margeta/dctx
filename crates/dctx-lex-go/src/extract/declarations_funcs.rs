//! Function and method declarations, including receivers and type parameters.
//! Implements SPEC-MAP-MODE 5.1 Go function facts and method names (`Type::Name`).
//! Depends on the walker and on reference scans for parameters, results and bodies.
//! Called from the file walker and from interface type bodies.
use super::super::{Item, Scope, Walker};
use crate::domain::DiagnosticReason;
use dctx_map_model::{NodeKind, SymbolKind};

impl Walker<'_> {
    pub(crate) fn func_decl(&mut self, index: usize, end: usize, scope: &Scope) -> usize {
        let mut cursor = index + 1;
        let mut receiver = None;
        if self.is(cursor, "(") {
            let close = self.close_of(cursor, end);
            receiver = self.receiver_base(cursor, close);
            if receiver.is_none() {
                self.problem(self.start_byte(cursor), DiagnosticReason::UnsupportedName);
                return self.skip_tail(cursor, end);
            }
            cursor = (close + 1).min(end);
        }
        self.finish_func(index, cursor, end, scope, receiver.as_deref(), false)
    }

    pub(crate) fn method_decl(&mut self, name_at: usize, end: usize, scope: &Scope) -> usize {
        self.finish_func(name_at, name_at, end, scope, None, true)
    }

    fn finish_func(
        &mut self,
        first: usize,
        name_at: usize,
        end: usize,
        scope: &Scope,
        receiver: Option<&str>,
        iface: bool,
    ) -> usize {
        if !self.can_be_name(name_at) {
            self.problem(self.start_byte(first), DiagnosticReason::Unrecognized);
            return self.skip_tail(name_at, end);
        }
        let bare = self.text(name_at).to_string();
        let local = match receiver {
            Some(receiver_type) => format!("{receiver_type}::{bare}"),
            None => bare.clone(),
        };
        let mut cursor = name_at + 1;
        if self.is(cursor, "[") {
            cursor = self.after(cursor).min(end);
        }
        if !self.is(cursor, "(") {
            self.problem(self.start_byte(name_at), DiagnosticReason::Unrecognized);
            return self.skip_tail(cursor, end);
        }
        let params = cursor;
        let params_close = self.close_of(params, end);
        cursor = (params_close + 1).min(end);
        let result_at = cursor;
        cursor = self.skip_result(cursor, end);
        let has_body = self.is(cursor, "{");
        if !has_body && !iface {
            self.problem(self.start_byte(first), DiagnosticReason::Unrecognized);
        }
        let (last, sig_end, body) = if has_body {
            let close = self.close_of(cursor, end);
            let sig = self.start_byte(cursor);
            (close, sig, (sig, self.end_byte(close)))
        } else {
            let last = cursor.saturating_sub(1).max(name_at);
            let end_byte = self.end_byte(last);
            (last, end_byte, (self.start_byte(first), end_byte))
        };
        let symbol = if receiver.is_some() || !scope.name.is_empty() {
            SymbolKind::Method
        } else {
            SymbolKind::Function
        };
        let item = Item {
            node_kind: NodeKind::Symbol,
            symbol: Some(symbol),
            local,
            bare,
            first,
            last,
            sig_end,
            body,
            doc_at: Some(if iface { name_at } else { first }),
        };
        let inner = self.item(item, scope);
        self.scan_params(params, params_close, inner.owner, &inner.name);
        self.scan_result(result_at, end, inner.owner, &inner.name);
        if has_body {
            let close = self.close_of(cursor, end);
            self.scan_calls(cursor + 1, close, inner.owner, &inner.name);
            self.consume_semi(close, end)
        } else {
            self.consume_semi(last, end)
        }
    }

    fn skip_result(&mut self, index: usize, end: usize) -> usize {
        if index >= end || self.is(index, "{") || self.is_semi(index) {
            return index;
        }
        if self.is(index, "(") {
            return self.after(index).min(end);
        }
        // Span only. Named types are emitted later, once the declaration node exists.
        self.skip_type(index, end, false, 0, "")
    }

    fn receiver_base(&self, open: usize, close: usize) -> Option<String> {
        let mut tokens = Vec::new();
        let mut index = open + 1;
        while index < close {
            if self.is(index, "[") {
                tokens.push(Recv::Bracket);
                index = self.after(index);
                continue;
            }
            if self.is(index, "*") {
                tokens.push(Recv::Star);
                index += 1;
                continue;
            }
            if self.is(index, ".") {
                tokens.push(Recv::Dot);
                index += 1;
                continue;
            }
            if self.ident(index) {
                if self.is_keyword(index) {
                    return None;
                }
                tokens.push(Recv::Id(self.text(index)));
                index += 1;
                continue;
            }
            if self.is_semi(index) {
                index += 1;
                continue;
            }
            return None;
        }
        while tokens.last() == Some(&Recv::Bracket) {
            tokens.pop();
        }
        if tokens.contains(&Recv::Bracket) {
            return None;
        }
        let typed = is_recv_type(&tokens)
            || (matches!(tokens.first(), Some(Recv::Id(_))) && is_recv_type(&tokens[1..]));
        let base = if typed { tokens.last().copied() } else { None };
        match base {
            Some(Recv::Id(name)) => Some(name.to_string()),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Recv<'a> {
    Id(&'a str),
    Star,
    Dot,
    Bracket,
}

fn is_recv_type(tokens: &[Recv<'_>]) -> bool {
    let mut index = usize::from(tokens.first() == Some(&Recv::Star));
    if !matches!(tokens.get(index), Some(Recv::Id(_))) {
        return false;
    }
    index += 1;
    while tokens.get(index) == Some(&Recv::Dot) {
        index += 1;
        if !matches!(tokens.get(index), Some(Recv::Id(_))) {
            return false;
        }
        index += 1;
    }
    index == tokens.len()
}
