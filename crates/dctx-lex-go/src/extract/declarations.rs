//! Package, import, function, type, const and var declarations.
//! Implements the Go declaration grammar used by SPEC-MAP-MODE 5.1, including
//! grouped specs, receivers and type parameters. Calls reference scans for names.
use super::{Item, Scope, Walker};
use crate::domain::DiagnosticReason;
use crate::lexer::Kind;
use dctx_map_model::{EdgeKind, NodeKind, SymbolKind};

impl Walker<'_> {
    pub(crate) fn package_clause(&mut self, index: usize, end: usize) -> usize {
        if self.saw_package || !self.can_be_name(index + 1) {
            self.problem(self.start_byte(index), DiagnosticReason::Unrecognized);
            return self.skip_tail(index, end);
        }
        self.saw_package = true;
        let name_at = index + 1;
        let bare = self.text(name_at).to_string();
        let item = Item {
            node_kind: NodeKind::Package,
            symbol: None,
            local: bare.clone(),
            bare,
            first: index,
            last: name_at,
            sig_end: self.end_byte(name_at),
            body: (self.start_byte(index), self.end_byte(name_at)),
            doc_at: Some(index),
        };
        let _ = self.item(item, &Scope::default());
        self.consume_semi(name_at, end)
    }

    pub(crate) fn import_decl(&mut self, index: usize, end: usize) -> usize {
        let next = index + 1;
        if self.is(next, "(") {
            let close = self.close_of(next, end);
            let mut cursor = next + 1;
            while cursor < close {
                cursor = self.skip_semis(cursor, close);
                if cursor >= close {
                    break;
                }
                let after = self.import_spec(cursor, close);
                if after <= cursor {
                    break;
                }
                cursor = after;
            }
            return self.consume_semi(close, end);
        }
        let after = self.import_spec(next, end);
        if self.is_semi(after) {
            after + 1
        } else {
            after.max(next + 1)
        }
    }

    fn import_spec(&mut self, index: usize, end: usize) -> usize {
        let mut cursor = index;
        let mut alias = None;
        if self.is(cursor, ".") {
            alias = Some(".".to_string());
            cursor += 1;
        } else if self.can_be_name(cursor) {
            alias = Some(self.text(cursor).to_string());
            cursor += 1;
        }
        match self.kind(cursor) {
            Some(Kind::Str | Kind::Raw) => {
                let token = self.toks[cursor];
                match string_value(self.src, token.kind, token.start, token.end) {
                    Some(path) if !path.is_empty() => {
                        self.refer(0, "", EdgeKind::Imports, path, alias, (index, cursor));
                    }
                    Some(_) => self.problem(self.start_byte(cursor), DiagnosticReason::Unrecognized),
                    None => self.problem(self.start_byte(cursor), DiagnosticReason::Unrecognized),
                }
                cursor += 1;
            }
            Some(Kind::BadStr) => cursor += 1,
            _ => {
                self.problem(self.start_byte(index), DiagnosticReason::Unrecognized);
                return (index + 1).min(end);
            }
        }
        if cursor < end && self.is_semi(cursor) {
            cursor += 1;
        }
        cursor
    }

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
        self.finish_func(index, cursor, end, scope, receiver, false)
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
        receiver: Option<String>,
        iface: bool,
    ) -> usize {
        if !self.can_be_name(name_at) {
            self.problem(self.start_byte(first), DiagnosticReason::Unrecognized);
            return self.skip_tail(name_at, end);
        }
        let bare = self.text(name_at).to_string();
        let local = match &receiver {
            Some(base) => format!("{base}::{bare}"),
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
        let body_open = (struct_kw || iface_kw).then_some(cursor + 1).filter(|&brace| self.is(brace, "{"));
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

    pub(crate) fn value_decl(&mut self, index: usize, end: usize, is_const: bool) -> usize {
        let next = index + 1;
        if self.is(next, "(") {
            let close = self.close_of(next, end);
            let mut cursor = next + 1;
            while cursor < close {
                cursor = self.skip_semis(cursor, close);
                if cursor >= close {
                    break;
                }
                let after = self.value_spec(index, cursor, close, is_const, true);
                if after <= cursor {
                    break;
                }
                cursor = after;
            }
            return self.consume_semi(close, end);
        }
        self.value_spec(index, next, end, is_const, false)
    }

    fn value_spec(
        &mut self,
        keyword_at: usize,
        name_at: usize,
        end: usize,
        is_const: bool,
        grouped: bool,
    ) -> usize {
        if !self.can_be_name(name_at) {
            self.problem(self.start_byte(name_at), DiagnosticReason::Unrecognized);
            return self.skip_tail(name_at, end);
        }
        let mut names = vec![name_at];
        let mut cursor = name_at + 1;
        while self.is(cursor, ",") && self.can_be_name(cursor + 1) {
            names.push(cursor + 1);
            cursor += 2;
        }
        let has_type = cursor < end && !self.is(cursor, "=") && !self.is_semi(cursor) && !self.is(cursor, ")");
        let (type_at, expr_at, spec_end) = if has_type {
            let type_end = self.skip_type(cursor, end, false, 0, "");
            if self.is(type_end, "=") {
                (Some(cursor), Some(type_end + 1), self.skip_expr(type_end + 1, end))
            } else {
                (Some(cursor), None, type_end.max(cursor + 1))
            }
        } else if self.is(cursor, "=") {
            (None, Some(cursor + 1), self.skip_expr(cursor + 1, end))
        } else {
            (None, None, cursor)
        };
        let last = spec_end.saturating_sub(1).max(name_at);
        let first = if grouped { name_at } else { keyword_at };
        let sig_end = self.end_byte(last);
        let body = (self.start_byte(first), sig_end);
        let symbol = is_const.then_some(SymbolKind::Const);
        let mut owner = 0_usize;
        let mut scope_name = String::new();
        for (ordinal, &name_index) in names.iter().enumerate() {
            let bare = self.text(name_index).to_string();
            let item = Item {
                node_kind: NodeKind::Symbol,
                symbol,
                local: bare.clone(),
                bare,
                first,
                last,
                sig_end,
                body,
                doc_at: (ordinal == 0).then_some(if grouped { name_at } else { keyword_at }),
            };
            let inner = self.item(item, &Scope::default());
            if ordinal == 0 {
                owner = inner.owner;
                scope_name = inner.name;
            }
        }
        if let Some(type_index) = type_at {
            let _ = self.skip_type(type_index, end, true, owner, &scope_name);
        }
        if let Some(expr) = expr_at {
            self.scan_calls(expr, spec_end, owner, &scope_name);
        }
        if spec_end < end && self.is_semi(spec_end) {
            spec_end + 1
        } else {
            spec_end
        }
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

    fn skip_tail(&self, mut index: usize, end: usize) -> usize {
        while index < end {
            if self.is(index, "{") {
                return self.after(index).min(end);
            }
            if self.is_semi(index) {
                return (index + 1).min(end);
            }
            if matches!(self.text(index), "(" | "[") {
                index = self.after(index).min(end);
                continue;
            }
            index += 1;
        }
        end
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

fn string_value(src: &str, kind: Kind, start: u32, end: u32) -> Option<String> {
    let raw = src.get(start as usize..end as usize)?;
    match kind {
        Kind::Raw => raw
            .strip_prefix('`')
            .and_then(|body| body.strip_suffix('`'))
            .map(str::to_string),
        Kind::Str => decode_interpreted(raw.strip_prefix('"')?.strip_suffix('"')?),
        _ => None,
    }
}

fn decode_interpreted(body: &str) -> Option<String> {
    let bytes = body.as_bytes();
    let mut out = String::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'\\' {
            let ch = body[index..].chars().next()?;
            out.push(ch);
            index += ch.len_utf8();
            continue;
        }
        index += 1;
        let escape = *bytes.get(index)?;
        index += 1;
        let ch = match escape {
            b'\\' => '\\',
            b'n' => '\n',
            b'r' => '\r',
            b't' => '\t',
            b'"' => '"',
            b'\'' => '\'',
            b'`' => '`',
            b'a' => '\u{7}',
            b'b' => '\u{8}',
            b'f' => '\u{c}',
            b'v' => '\u{b}',
            b'x' => hex_char(bytes, &mut index, 2)?,
            b'u' => hex_char(bytes, &mut index, 4)?,
            b'U' => hex_char(bytes, &mut index, 8)?,
            b'0'..=b'7' => octal_char(bytes, &mut index, escape)?,
            _ => return None,
        };
        out.push(ch);
    }
    Some(out)
}

fn hex_char(bytes: &[u8], index: &mut usize, digits: usize) -> Option<char> {
    let mut value = 0_u32;
    for _ in 0..digits {
        let digit = bytes.get(*index)?;
        let add = match digit {
            b'0'..=b'9' => digit - b'0',
            b'a'..=b'f' => digit - b'a' + 10,
            b'A'..=b'F' => digit - b'A' + 10,
            _ => return None,
        };
        value = value * 16 + u32::from(add);
        *index += 1;
    }
    char::from_u32(value)
}

fn octal_char(bytes: &[u8], index: &mut usize, first: u8) -> Option<char> {
    let mut value = u32::from(first - b'0');
    let mut count = 1;
    while count < 3 && bytes.get(*index).is_some_and(|byte| (b'0'..=b'7').contains(byte)) {
        value = value * 8 + u32::from(bytes[*index] - b'0');
        *index += 1;
        count += 1;
    }
    char::from_u32(value)
}

#[cfg(test)]
mod tests {
    use crate::GoLexer;
    use dctx_map_model::EdgeKind;

    #[test]
    fn import_paths_decode_escapes_and_raw_strings() {
        let source = "package p\nimport \"a\\x2fb\"\nimport `c/d`\n";
        let facts = GoLexer::default().extract("p.go", source.as_bytes()).unwrap();
        let imports: Vec<_> = facts
            .references
            .iter()
            .filter(|reference| reference.fact.kind == EdgeKind::Imports)
            .map(|reference| reference.fact.name.as_str())
            .collect();
        assert_eq!(imports, ["a/b", "c/d"]);
        assert_eq!(facts.completion, crate::Completion::Complete);
    }
}
