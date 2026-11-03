// SPDX-License-Identifier: Apache-2.0
//! Strings, `%`-literals, regexes, symbols and character literals.
//! Expression position decides whether `/` and `%` open a literal or act as operators.
//! Dispatch, comments and the shared operator table live in the parent module.
use super::{is_name_start_byte, LONG_OPS};
use crate::domain::LexError;
use crate::lexer::{as_u32, Kind, Lx};

impl Lx<'_> {
    pub(super) fn quoted(&mut self, quote: u8) -> Result<(), LexError> {
        let start = self.i;
        self.i += 1;
        let interp = quote != b'\'';
        let plain_kind = quote != b'`';
        self.scan_body(start, quote, false, interp, plain_kind)
    }

    pub(super) fn percent(&mut self) -> Result<(), LexError> {
        // `puts %{x}` is a command argument. `10 % 2` stays modulo.
        let command = self.tokens.last().is_some_and(|token| token.kind == Kind::Ident);
        if !self.fname && (self.expr || command) && self.try_percent()? {
            return Ok(());
        }
        self.operator()
    }

    fn try_percent(&mut self) -> Result<bool, LexError> {
        let start = self.i;
        let mut j = self.i + 1;
        let mut letter = None;
        if let Some(byte) = self.byte_at(j)
            && matches!(byte, b'q' | b'Q' | b'w' | b'W' | b'i' | b'I' | b'r' | b's' | b'x')
            && self.byte_at(j + 1).is_some_and(is_delim)
        {
            letter = Some(byte);
            j += 1;
        }
        let Some(delim) = self.byte_at(j).filter(|byte| is_delim(*byte)) else {
            return Ok(false);
        };
        // `%=` is modulo-assign. `=` delimits only after a type letter (`%q=...=`).
        if letter.is_none() && (delim == b'=' || self.byte_at(self.i + 1) != Some(delim)) {
            return Ok(false);
        }
        self.i = j + 1;
        let paired = pair_close(delim);
        let close = paired.unwrap_or(delim);
        let interp = matches!(letter, None | Some(b'Q' | b'W' | b'I' | b'r' | b'x'));
        let plain_kind = matches!(letter, None | Some(b'q' | b'Q'));
        self.scan_body(start, close, paired.is_some(), interp, plain_kind)?;
        Ok(true)
    }

    pub(super) fn slash(&mut self) -> Result<(), LexError> {
        if self.fname || (!self.expr && !self.command_regexp()) {
            return self.operator();
        }
        let start = self.i;
        self.i += 1;
        self.scan_body(start, b'/', false, true, false)
    }

    fn command_regexp(&self) -> bool {
        // Ruby command arguments use `puts /re/`; `value / divisor` is division.
        // Resolving a bare name as a local variable requires a parser's scope table.
        let Some(token) = self.tokens.last().filter(|token| token.kind == Kind::Ident) else {
            return false;
        };
        let name = self.src.get(token.start as usize..token.end as usize).unwrap_or("");
        let gap = self.src.get(token.end as usize..self.i).unwrap_or("");
        !matches!(name, "nil" | "true" | "false" | "self" | "end")
            && name.chars().next().is_some_and(|ch| ch.is_lowercase() || ch == '_')
            && !gap.is_empty()
            && gap.bytes().all(|byte| matches!(byte, b' ' | b'\t'))
            && self.byte_at(self.i + 1).is_some_and(|byte| {
                !byte.is_ascii_whitespace() && byte != b'='
            })
    }

    pub(super) fn colon(&mut self) -> Result<(), LexError> {
        if self.src.get(self.i..).unwrap_or("").starts_with("::") {
            self.emit(Kind::Punct, self.i, self.i + 2, false);
            self.i += 2;
            self.expr = true;
            self.fname = true;
            return Ok(());
        }
        // `foo:bar` is a label. `attr :width` and `:width` are symbols.
        let label = self.tokens.last().is_some_and(|token| {
            token.kind == Kind::Ident && token.end as usize == self.i
        });
        if !label && !self.fname && self.try_symbol()? {
            return Ok(());
        }
        self.emit(Kind::Punct, self.i, self.i + 1, false);
        self.i += 1;
        self.expr = true;
        self.fname = false;
        Ok(())
    }

    fn try_symbol(&mut self) -> Result<bool, LexError> {
        let start = self.i;
        match self.byte_at(self.i + 1) {
            Some(b'\'' | b'"' | b'`') => {
                let quote = self.b[self.i + 1];
                self.i += 1;
                self.quoted(quote)?;
                if let Some(token) = self.tokens.last_mut() {
                    token.kind = Kind::Symbol;
                    token.plain = false;
                    token.start = as_u32(start);
                }
                self.expr = false;
                Ok(true)
            }
            Some(b'[' | b']' | b'+' | b'-' | b'*' | b'/' | b'%' | b'&' | b'|' | b'^' | b'~' | b'!'
            | b'<' | b'>' | b'=') => {
                self.i += 1;
                self.consume_op_body();
                self.emit(Kind::Symbol, start, self.i, false);
                self.expr = false;
                self.fname = false;
                Ok(true)
            }
            Some(byte) if byte == b'@' || byte == b'$' || is_name_start_byte(byte) || byte >= 0x80 => {
                self.i += 1;
                self.consume_name();
                self.consume_suffix(true);
                self.emit(Kind::Symbol, start, self.i, false);
                self.expr = false;
                self.fname = false;
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    pub(super) fn try_char_literal(&mut self) -> bool {
        // After a value, `?` is the ternary operator. At expression start it is `?x`.
        if !self.expr {
            return false;
        }
        let Some(next) = self.byte_at(self.i + 1) else {
            return false;
        };
        if matches!(next, b'\n' | b'\r') {
            return false;
        }
        let start = self.i;
        self.i += 1;
        if next == b'\\' {
            self.i += 1;
            self.consume_char_escape();
        } else {
            self.advance_char();
        }
        if self.i <= start {
            self.i = start + 1;
        }
        self.emit(Kind::Str, start, self.i, false);
        self.expr = false;
        self.fname = false;
        true
    }

    fn consume_char_escape(&mut self) {
        let mut guard = 0u8;
        loop {
            if guard > 4 || self.i >= self.b.len() {
                return;
            }
            guard += 1;
            let kind = self.b[self.i];
            if kind == b'c' || (matches!(kind, b'C' | b'M') && self.byte_at(self.i + 1) == Some(b'-'))
            {
                self.i += 1;
                if self.byte_at(self.i) == Some(b'-') {
                    self.i += 1;
                }
                if self.byte_at(self.i) == Some(b'\\') {
                    self.i += 1;
                    continue;
                }
                self.advance_char();
                return;
            }
            if kind == b'x' {
                self.i += 1;
                for _ in 0..2 {
                    if self.byte_at(self.i).is_some_and(|byte| byte.is_ascii_hexdigit()) {
                        self.i += 1;
                    }
                }
                return;
            }
            if kind == b'u' {
                self.i += 1;
                if self.byte_at(self.i) == Some(b'{') {
                    self.i += 1;
                    while self.byte_at(self.i).is_some_and(|byte| byte.is_ascii_hexdigit()) {
                        self.i += 1;
                    }
                    if self.byte_at(self.i) == Some(b'}') {
                        self.i += 1;
                    }
                } else {
                    for _ in 0..4 {
                        if self.byte_at(self.i).is_some_and(|byte| byte.is_ascii_hexdigit()) {
                            self.i += 1;
                        }
                    }
                }
                return;
            }
            if matches!(kind, b'0'..=b'7') {
                self.i += 1;
                for _ in 0..2 {
                    if self.byte_at(self.i).is_some_and(|byte| matches!(byte, b'0'..=b'7')) {
                        self.i += 1;
                    }
                }
                return;
            }
            self.advance_char();
            return;
        }
    }

    fn consume_op_body(&mut self) {
        if self.byte_at(self.i) == Some(b'[') {
            self.i += 1;
            if self.byte_at(self.i) == Some(b']') {
                self.i += 1;
            }
            if self.byte_at(self.i) == Some(b'=') {
                self.i += 1;
            }
            return;
        }
        let rest = self.src.get(self.i..).unwrap_or("");
        let len = LONG_OPS
            .iter()
            .find(|op| rest.starts_with(*op))
            .map_or(1, |op| op.len());
        self.i += len.min(rest.len()).max(1);
    }
}

fn is_delim(byte: u8) -> bool {
    byte.is_ascii_punctuation()
}

fn pair_close(open: u8) -> Option<u8> {
    match open {
        b'(' => Some(b')'),
        b'[' => Some(b']'),
        b'{' => Some(b'}'),
        b'<' => Some(b'>'),
        _ => None,
    }
}
