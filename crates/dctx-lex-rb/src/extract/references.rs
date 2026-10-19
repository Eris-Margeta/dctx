//! Imports, calls, member references and comment summaries.
//! Implements SPEC-MAP-MODE 5.3 references for Ruby without resolving them.
//! A bare word is not a call. A non-literal `require` emits no import edge.
use super::{OutRef, Scope, Walker};
use crate::domain::{comment_summary, Precision};
use crate::lexer::Kind;
use dctx_map_model::EdgeKind;

impl Walker<'_> {
    pub(super) fn import_stmt(&mut self, scope: &Scope) -> bool {
        if !matches!(self.cur(), "require" | "require_relative" | "load") {
            return false;
        }
        let start = self.tok_start();
        self.bump();
        if self.at_punct("(") {
            self.bump();
        }
        if self.plain_string() {
            let end = self.tok_end();
            if let Some(text) = decode_string(self.slice(self.tok_start(), end))
                && !text.is_empty()
            {
                self.emit_ref(OutRef {
                    owner: scope.owner,
                    scope: &scope.name,
                    kind: EdgeKind::Imports,
                    name: text,
                    start,
                    end,
                    precision: Precision::Structural,
                });
            }
            self.bump();
        }
        self.scan_statement(scope);
        true
    }

    pub(super) fn scan_statement(&mut self, scope: &Scope) {
        let mut brackets = 0i32;
        let mut ends = 0i32;
        while !self.eof() && self.refused.is_none() {
            if self.at_word("end") && !self.prev_member() {
                if ends > 0 {
                    ends -= 1;
                    self.unnest();
                    self.bump();
                    continue;
                }
                return;
            }
            if brackets == 0 && ends == 0 && self.is_sep() {
                self.bump();
                return;
            }
            if self.ender() && !self.modifier_kw() {
                if !self.open_ender(scope, &mut ends) {
                    return;
                }
                continue;
            }
            if self.opens() {
                brackets += 1;
                self.bump();
                continue;
            }
            if self.closes() {
                if brackets > 0 {
                    brackets -= 1;
                }
                self.bump();
                continue;
            }
            self.note_call(scope);
            self.bump();
        }
    }

    pub(super) fn skip_header(&mut self, scope: &Scope) {
        let mut brackets = 0i32;
        while !self.eof() && self.refused.is_none() {
            if brackets == 0 && (self.is_sep() || self.at_word("then") || self.at_word("do")) {
                self.bump();
                return;
            }
            if self.opens() {
                brackets += 1;
                self.bump();
                continue;
            }
            if self.closes() {
                if brackets > 0 {
                    brackets -= 1;
                }
                self.bump();
                continue;
            }
            self.note_call(scope);
            self.bump();
        }
    }

    fn open_ender(&mut self, scope: &Scope, ends: &mut i32) -> bool {
        let word = self.cur().to_owned();
        self.bump();
        if matches!(word.as_str(), "while" | "until" | "for" | "if" | "unless" | "case") {
            self.skip_header(scope);
        }
        *ends += 1;
        self.nest()
    }

    fn note_call(&mut self, scope: &Scope) {
        if self.kind_at(self.i) != Some(Kind::Ident) {
            return;
        }
        // `value.class` is a call. A bare `class` keyword is not.
        if is_keyword(self.cur()) && !self.prev_member() {
            return;
        }
        let next_paren = self.peek_punct("(");
        let prev_dot = self.prev_member();
        let next_link = self.peek_punct(".") || self.peek_punct("::") || self.peek_punct("&.");
        if next_link || (!next_paren && !prev_dot) {
            return;
        }
        if self.prev_scope() && is_const_name(self.cur()) && !next_paren {
            return;
        }
        let (from, name) = self.chain();
        if name.is_empty() {
            return;
        }
        let end = self.call_end();
        let member = name.contains('.') || name.contains("&.");
        self.emit_ref(OutRef {
            owner: scope.owner,
            scope: &scope.name,
            kind: EdgeKind::Calls,
            name: name.clone(),
            start: from,
            end,
            precision: Precision::Structural,
        });
        if member {
            self.emit_ref(OutRef {
                owner: scope.owner,
                scope: &scope.name,
                kind: EdgeKind::UsesType,
                name,
                start: from,
                end,
                precision: Precision::Convention,
            });
        }
    }

    fn chain(&self) -> (u32, String) {
        let mut index = self.i;
        let mut parts = vec![self.text_at(index).to_owned()];
        while index >= 2 {
            let sep = self.text_at(index - 1);
            if matches!(sep, "." | "::" | "&.")
                && matches!(self.kind_at(index - 2), Some(Kind::Ident | Kind::Symbol))
            {
                parts.push(sep.to_owned());
                parts.push(self.text_at(index - 2).to_owned());
                index -= 2;
                continue;
            }
            break;
        }
        parts.reverse();
        let start = self.toks.get(index).map_or(0, |token| token.start);
        (start, parts.concat())
    }

    fn call_end(&self) -> u32 {
        if !self.peek_punct("(") {
            return self.tok_end();
        }
        let mut index = self.i + 1;
        let mut depth = 0i32;
        while index < self.toks.len() {
            if self.text_at(index) == "(" && self.kind_at(index) == Some(Kind::Punct) {
                depth += 1;
            }
            if self.text_at(index) == ")" && self.kind_at(index) == Some(Kind::Punct) {
                depth -= 1;
                if depth == 0 {
                    return self.end_at(index);
                }
            }
            index += 1;
        }
        self.tok_end()
    }

    pub(super) fn prev_member(&self) -> bool {
        self.i > 0 && matches!(self.text_at(self.i - 1), "." | "::" | "&.")
    }

    fn prev_scope(&self) -> bool {
        self.i > 0 && self.text_at(self.i - 1) == "::"
    }

    fn ender(&self) -> bool {
        !self.prev_member()
            && self.kind_at(self.i) == Some(Kind::Ident)
            && matches!(
                self.cur(),
                "class"
                    | "module"
                    | "def"
                    | "if"
                    | "unless"
                    | "while"
                    | "until"
                    | "for"
                    | "begin"
                    | "case"
                    | "do"
            )
    }

    // A modifier follows a value (`return if ready`, `"llvm" if mac?`), including
    // inside `do` and braces. A block keyword follows a statement boundary.
    fn modifier_kw(&self) -> bool {
        matches!(self.cur(), "if" | "unless" | "while" | "until")
            && self.kind_at(self.i) == Some(Kind::Ident)
            && self.prev_is_value()
    }

    fn prev_is_value(&self) -> bool {
        if self.i == 0 {
            return false;
        }
        match self.kind_at(self.i - 1) {
            Some(Kind::Symbol | Kind::Str | Kind::Number) => true,
            Some(Kind::Ident) => !is_nonvalue_keyword(self.text_at(self.i - 1)),
            Some(Kind::Punct) => matches!(self.text_at(self.i - 1), ")" | "]" | "}"),
            _ => false,
        }
    }

    fn opens(&self) -> bool {
        self.at_kind(Kind::Punct) && matches!(self.cur(), "(" | "[" | "{")
    }

    fn closes(&self) -> bool {
        self.at_kind(Kind::Punct) && matches!(self.cur(), ")" | "]" | "}")
    }

    pub(super) fn docs(&self, byte: u32) -> Option<String> {
        let decl_line = self.lex.line(byte);
        let mut expect = decl_line.checked_sub(1)?;
        let mut blocks = Vec::new();
        for &(start, end) in self.lex.comments.iter().rev() {
            let start_line = self.lex.line(start);
            let end_line = self.lex.line(end.saturating_sub(1).max(start));
            if end_line > expect {
                continue;
            }
            if end_line < expect {
                break;
            }
            blocks.push(self.slice(start, end).to_owned());
            if start_line <= 1 {
                break;
            }
            expect = start_line - 1;
        }
        if blocks.is_empty() {
            return None;
        }
        blocks.reverse();
        comment_summary(&blocks.join("\n"))
    }
}

fn is_nonvalue_keyword(word: &str) -> bool {
    matches!(
        word,
        "if" | "unless"
            | "while"
            | "until"
            | "for"
            | "case"
            | "begin"
            | "class"
            | "module"
            | "def"
            | "do"
            | "else"
            | "elsif"
            | "when"
            | "rescue"
            | "ensure"
            | "then"
            | "and"
            | "or"
            | "not"
            | "in"
    )
}

pub(super) fn is_keyword(word: &str) -> bool {
    matches!(
        word,
        "alias"
            | "and"
            | "begin"
            | "break"
            | "case"
            | "class"
            | "def"
            | "defined?"
            | "do"
            | "else"
            | "elsif"
            | "end"
            | "ensure"
            | "false"
            | "for"
            | "if"
            | "in"
            | "module"
            | "next"
            | "nil"
            | "not"
            | "or"
            | "redo"
            | "rescue"
            | "retry"
            | "return"
            | "self"
            | "super"
            | "then"
            | "true"
            | "undef"
            | "unless"
            | "until"
            | "when"
            | "while"
            | "yield"
            | "__FILE__"
            | "__LINE__"
            | "__ENCODING__"
    )
}

pub(super) fn symbol_name(text: &str) -> Option<String> {
    let rest = text.strip_prefix(':')?;
    if rest.is_empty() || rest.contains(['#', '~', ' ']) {
        return None;
    }
    if (rest.starts_with('"') && rest.ends_with('"') && rest.len() >= 2)
        || (rest.starts_with('\'') && rest.ends_with('\'') && rest.len() >= 2)
    {
        let inner = &rest[1..rest.len() - 1];
        if inner.is_empty() || inner.contains('\\') {
            return None;
        }
        return Some(inner.to_owned());
    }
    if rest
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '!' | '?' | '='))
    {
        return Some(rest.to_owned());
    }
    None
}

fn is_const_name(name: &str) -> bool {
    name.chars().next().is_some_and(|ch| ch.is_ascii_uppercase())
}

fn decode_string(src: &str) -> Option<String> {
    let bytes = src.as_bytes();
    if bytes.len() < 2 {
        return None;
    }
    let inner = if matches!(bytes[0], b'\'' | b'"' | b'`') && *bytes.last()? == bytes[0] {
        src.get(1..src.len() - 1)?
    } else {
        percent_inner(src)?
    };
    unescape(inner)
}

fn percent_inner(src: &str) -> Option<&str> {
    let bytes = src.as_bytes();
    let mut index = 1;
    if matches!(bytes.get(index), Some(b'q' | b'Q')) {
        index += 1;
    }
    let open = *bytes.get(index)?;
    let close = match open {
        b'(' => b')',
        b'[' => b']',
        b'{' => b'}',
        b'<' => b'>',
        _ => open,
    };
    if *bytes.last()? != close || index + 1 >= src.len() {
        return None;
    }
    src.get(index + 1..src.len() - 1)
}

fn unescape(inner: &str) -> Option<String> {
    let bytes = inner.as_bytes();
    let mut out = String::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\' && index + 1 < bytes.len() {
            let ch = match bytes[index + 1] {
                b'n' => '\n',
                b't' => '\t',
                b'r' => '\r',
                other => char::from(other),
            };
            out.push(ch);
            index += 2;
            continue;
        }
        let ch = inner.get(index..)?.chars().next()?;
        out.push(ch);
        index += ch.len_utf8();
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoted_requires_decode_and_symbols_strip_the_colon() {
        assert_eq!(decode_string("'json'").as_deref(), Some("json"));
        assert_eq!(decode_string("\"a\\nb\"").as_deref(), Some("a\nb"));
        assert_eq!(decode_string("%q{helper}").as_deref(), Some("helper"));
        assert_eq!(symbol_name(":width").as_deref(), Some("width"));
        assert_eq!(symbol_name(":\"size\"").as_deref(), Some("size"));
        assert_eq!(symbol_name(":"), None);
        assert!(is_keyword("def"));
        assert!(!is_keyword("require"));
    }
}
