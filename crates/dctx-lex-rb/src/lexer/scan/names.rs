//! Identifiers, globals, numbers and operators.
//! Keyword text decides whether an identifier opens an expression; the longest
//! matching operator wins over its one-byte prefixes.
use super::{is_name_start_byte, LONG_OPS};
use crate::domain::{DiagnosticReason, LexError};
use crate::lexer::{Kind, Lx};

impl Lx<'_> {
    pub(super) fn ident(&mut self) {
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

    pub(super) fn consume_name(&mut self) {
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

    pub(super) fn consume_suffix(&mut self, setter: bool) {
        let byte = self.byte_at(self.i);
        if matches!(byte, Some(b'!' | b'?')) {
            self.i += 1;
            return;
        }
        if setter && byte == Some(b'=') && !matches!(self.byte_at(self.i + 1), Some(b'=' | b'>')) {
            self.i += 1;
        }
    }

    pub(super) fn number(&mut self) {
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

    pub(super) fn operator(&mut self) -> Result<(), LexError> {
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
