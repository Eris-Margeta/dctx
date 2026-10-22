//! Code, strings, `%`-literals, regex and symbols for the Ruby lexer.
//! Implements expression-position rules so `/` and `%` are not misread as operators.
//! Heredoc bodies and bracket depth live in the sibling modules; this is the driver.
use super::{as_u32, Kind, Lx};
use crate::domain::{DiagnosticReason, LexError};

const LONG_OPS: &[&str] = &[
    "...", "===", "<=>", "**=", "&&=", "||=", "==", "=~", "!=", "!~", "<=", ">=", "**", "<<", ">>",
    "&&", "||", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "=>", "->", "..", "::", "&.",
];

impl Lx<'_> {
    pub(super) fn run(&mut self) -> Result<(), LexError> {
        let mut fuel = self.b.len().saturating_mul(8).max(8);
        while self.i < self.b.len() {
            if fuel == 0 {
                self.problem(self.i, DiagnosticReason::Malformed);
                break;
            }
            fuel -= 1;
            let before = self.i;
            self.step()?;
            if self.i == before {
                self.advance_char();
            }
        }
        self.finish_pending();
        Ok(())
    }

    pub(super) fn step(&mut self) -> Result<(), LexError> {
        if self.i >= self.b.len() {
            return Ok(());
        }
        let byte = self.b[self.i];
        if matches!(byte, b' ' | b'\t' | b'\r' | 0x0b | 0x0c) {
            self.i += 1;
            return Ok(());
        }
        if byte == b'\n' {
            return self.on_newline();
        }
        if self.at_bol() && self.try_begin() {
            return Ok(());
        }
        if self.at_bol() && self.try_data() {
            return Ok(());
        }
        if byte == b'#' {
            self.line_comment();
            return Ok(());
        }
        if byte == b'\\' && self.continue_line() {
            return Ok(());
        }
        if matches!(byte, b'\'' | b'"' | b'`') {
            return self.quoted(byte);
        }
        if byte == b'%' {
            return self.percent();
        }
        if byte == b'/' {
            return self.slash();
        }
        if byte == b':' {
            return self.colon();
        }
        if byte == b'<' && self.try_heredoc() {
            return Ok(());
        }
        if byte.is_ascii_digit() {
            self.number();
            return Ok(());
        }
        if byte == b'@' || byte == b'$' || is_name_start_byte(byte) || byte >= 0x80 {
            self.ident();
            return Ok(());
        }
        if byte == b'?' {
            if self.try_char_literal() {
                return Ok(());
            }
            return self.operator();
        }
        self.operator()
    }

    fn try_begin(&mut self) -> bool {
        if !word_at(self.src, self.i, "=begin") {
            return false;
        }
        let start = self.i;
        self.i += "=begin".len();
        while self.i < self.b.len() {
            if self.at_bol() && word_at(self.src, self.i, "=end") {
                self.i += "=end".len();
                while self.i < self.b.len() && self.b[self.i] != b'\n' {
                    self.i += 1;
                }
                if self.i < self.b.len() && self.b[self.i] == b'\n' {
                    self.i += 1;
                }
                self.comments.push((as_u32(start), as_u32(self.i)));
                return true;
            }
            self.i += 1;
        }
        self.problem(start, DiagnosticReason::UnterminatedComment);
        self.comments.push((as_u32(start), as_u32(self.i)));
        true
    }

    fn try_data(&mut self) -> bool {
        if !word_at(self.src, self.i, "__END__") {
            return false;
        }
        self.i = self.b.len();
        true
    }

    fn line_comment(&mut self) {
        let start = self.i;
        while self.i < self.b.len() && self.b[self.i] != b'\n' {
            self.i += 1;
        }
        self.comments.push((as_u32(start), as_u32(self.i)));
    }

    fn continue_line(&mut self) -> bool {
        let mut j = self.i + 1;
        if self.byte_at(j) == Some(b'\r') {
            j += 1;
        }
        if self.byte_at(j) == Some(b'\n') {
            self.i = j + 1;
            true
        } else {
            false
        }
    }

    fn quoted(&mut self, quote: u8) -> Result<(), LexError> {
        let start = self.i;
        self.i += 1;
        let interp = quote != b'\'';
        let plain_kind = quote != b'`';
        self.scan_body(start, quote, false, interp, plain_kind)
    }

    fn percent(&mut self) -> Result<(), LexError> {
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

    fn slash(&mut self) -> Result<(), LexError> {
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

    fn colon(&mut self) -> Result<(), LexError> {
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

    fn ident(&mut self) {
        let start = self.i;
        if self.b[self.i] == b'@' {
            self.i += 1;
            if self.byte_at(self.i) == Some(b'@') {
                self.i += 1;
            }
            self.consume_name();
            self.consume_suffix(false);
        } else if self.b[self.i] == b'$' {
            self.i += 1;
            self.consume_global_body();
        } else {
            self.consume_name();
            self.consume_suffix(self.fname);
        }
        let text = self.src.get(start..self.i).unwrap_or("");
        self.emit(Kind::Ident, start, self.i, false);
        self.expr = expr_keyword(text);
        self.fname = text == "def" || text == "alias";
    }

    fn consume_global_body(&mut self) {
        let Some(byte) = self.byte_at(self.i) else {
            return;
        };
        // `$-w` is a two-character global. `$1` and `$foo` keep the name body.
        if byte == b'-' {
            self.i += 1;
            if self
                .byte_at(self.i)
                .is_some_and(|next| is_name_start_byte(next) || next.is_ascii_digit())
            {
                self.i += 1;
            }
            return;
        }
        if is_name_start_byte(byte) || byte.is_ascii_digit() || byte >= 0x80 {
            self.consume_name();
            return;
        }
        // One punctuation global: $~, $!, $', $", $`, $@, $:, $\.
        if byte.is_ascii_punctuation() {
            self.i += 1;
        }
    }

    fn try_char_literal(&mut self) -> bool {
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

    fn consume_name(&mut self) {
        while self.i < self.b.len() {
            let byte = self.b[self.i];
            if byte.is_ascii_alphanumeric() || byte == b'_' {
                self.i += 1;
                continue;
            }
            if byte >= 0x80 {
                self.advance_char();
                continue;
            }
            break;
        }
    }

    fn consume_suffix(&mut self, setter: bool) {
        let byte = self.byte_at(self.i);
        if matches!(byte, Some(b'!' | b'?')) {
            self.i += 1;
            return;
        }
        if setter && byte == Some(b'=') && !matches!(self.byte_at(self.i + 1), Some(b'=' | b'>')) {
            self.i += 1;
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

    fn number(&mut self) {
        let start = self.i;
        if self.b[self.i] == b'0' && matches!(self.byte_at(self.i + 1), Some(b'x' | b'X' | b'b' | b'B' | b'o' | b'O' | b'd' | b'D'))
        {
            self.i += 2;
        }
        while self.i < self.b.len() {
            let byte = self.b[self.i];
            if byte == b'.' && self.byte_at(self.i + 1) == Some(b'.') {
                break;
            }
            if byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'.' {
                self.i += 1;
                continue;
            }
            break;
        }
        self.emit(Kind::Number, start, self.i, false);
        self.expr = false;
        self.fname = false;
    }

    fn operator(&mut self) -> Result<(), LexError> {
        let rest = self.src.get(self.i..).unwrap_or("");
        let len = LONG_OPS
            .iter()
            .find(|op| rest.starts_with(*op))
            .map_or(1, |op| op.len())
            .min(rest.len().max(1));
        let text = rest.get(..len).unwrap_or("");
        if matches!(text, "(" | "[" | "{") {
            let closer = match text.as_bytes().first().copied().unwrap_or(b'{') {
                b'(' => b')',
                b'[' => b']',
                _ => b'}',
            };
            self.open_bracket(closer)?;
        } else if matches!(text, ")" | "]" | "}") {
            self.pop_closer(text.as_bytes().first().copied().unwrap_or(b')'));
            self.expr = false;
            self.fname = false;
        } else if text == ";" {
            self.expr = true;
            self.fname = false;
        } else {
            self.expr = true;
            self.fname = matches!(text, "." | "::" | "&.");
        }
        self.emit(Kind::Punct, self.i, self.i + len, false);
        self.i += len.max(1);
        Ok(())
    }

    fn pop_closer(&mut self, got: u8) {
        if self.brackets > 0 {
            self.brackets -= 1;
        }
        if let Some(open) = self.closers.pop() {
            self.depth = self.depth.saturating_sub(1);
            if open.byte != got {
                self.problem(self.i, DiagnosticReason::UnbalancedBracket);
            }
        } else {
            self.problem(self.i, DiagnosticReason::UnbalancedBracket);
        }
    }
}

fn expr_keyword(text: &str) -> bool {
    matches!(
        text,
        "if" | "unless"
            | "while"
            | "until"
            | "and"
            | "or"
            | "not"
            | "return"
            | "yield"
            | "when"
            | "then"
            | "else"
            | "elsif"
            | "do"
            | "begin"
            | "case"
            | "class"
            | "module"
            | "def"
            | "for"
            | "rescue"
            | "ensure"
            | "defined?"
            | "break"
            | "next"
            | "redo"
            | "retry"
            | "super"
            | "in"
            | "alias"
            | "undef"
    )
}

fn is_name_start_byte(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
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

pub(super) fn pair_open(close: u8) -> u8 {
    match close {
        b')' => b'(',
        b']' => b'[',
        b'}' => b'{',
        b'>' => b'<',
        _ => 0,
    }
}

fn word_at(src: &str, index: usize, word: &str) -> bool {
    let Some(rest) = src.get(index..) else {
        return false;
    };
    let Some(after) = rest.strip_prefix(word) else {
        return false;
    };
    match after.chars().next() {
        None => true,
        Some(ch) => ch.is_whitespace() || !(ch.is_ascii_alphanumeric() || ch == '_'),
    }
}
