//! Code-mode scan: identifiers, numbers, operators and bracket depth.
//! Implements PARSERS-v1 tokenisation for Python source outside of strings.
//! Owns the main lexer loop; strings and indentation are sibling modules.
use super::{Emitted, Kind, Lx};
use crate::domain::{DiagnosticReason, LexError};

impl Lx<'_> {
    pub(super) fn run(&mut self) -> Result<(), LexError> {
        if self.src.starts_with('\u{feff}') {
            self.i = '\u{feff}'.len_utf8();
        }
        while self.i < self.src.len() {
            let before = self.i;
            if self.at_bol && self.parens.is_empty() {
                self.logical_indent()?;
                if self.i >= self.src.len() {
                    break;
                }
                self.at_bol = false;
            }
            self.scan_code()?;
            if self.i == before {
                self.bump();
            }
        }
        if !self.at_bol {
            let at = self.pos();
            self.emit(Kind::Newline, at, at, false);
        }
        let at = self.pos();
        while self.indents.len() > 1 {
            self.indents.pop();
            self.emit(Kind::Dedent, at, at, false);
        }
        let open = self.parens.clone();
        for (_, byte) in open {
            self.problem(byte, DiagnosticReason::UnbalancedBracket);
        }
        self.parens.clear();
        Ok(())
    }

    fn scan_code(&mut self) -> Result<(), LexError> {
        self.skip_spaces();
        if self.i >= self.src.len() {
            return Ok(());
        }
        if self.at_newline() {
            if self.parens.is_empty() {
                let at = self.pos();
                self.consume_nl();
                self.emit(Kind::Newline, at, self.pos(), false);
                self.at_bol = true;
            } else {
                self.consume_nl();
            }
            return Ok(());
        }
        if self.at_char('#') {
            self.comment();
            return Ok(());
        }
        if self.continued() {
            self.bump();
            self.consume_nl();
            return Ok(());
        }
        self.scan_one_token().map(|_| ())
    }

    pub(super) fn scan_one_token(&mut self) -> Result<Emitted, LexError> {
        self.skip_spaces();
        if self.i >= self.src.len() {
            return Ok(Emitted::Other);
        }
        if let Some((len, prefix)) = self.look_prefix() {
            return self.take_string(len, prefix);
        }
        if self.number_start() {
            self.take_number();
            return Ok(Emitted::Other);
        }
        if self.peek().is_some_and(ident_start) {
            return Ok(self.take_ident());
        }
        self.take_op()
    }

    /// One string inside a replacement field. Its brackets stay off the statement stack.
    pub(super) fn lex_fexpr_string(&mut self) -> Result<bool, LexError> {
        self.skip_f_ws();
        if self.i >= self.src.len() || self.look_prefix().is_none() {
            return Ok(false);
        }
        let track = self.track_parens;
        self.track_parens = false;
        let before = self.i;
        let scanned = self.scan_one_token();
        self.track_parens = track;
        scanned?;
        if self.i == before {
            self.bump();
        }
        Ok(true)
    }

    fn take_ident(&mut self) -> Emitted {
        let start = self.pos();
        self.bump();
        while self.peek().is_some_and(ident_cont) {
            self.bump();
        }
        self.emit(Kind::Ident, start, self.pos(), false);
        Emitted::Name
    }

    fn take_number(&mut self) {
        let start = self.pos();
        if self.at_char('.') {
            self.bump();
            self.consume_digits();
        } else {
            self.consume_digits();
            if self.at_char('x') || self.at_char('X') || self.at_char('o') || self.at_char('O')
                || self.at_char('b') || self.at_char('B')
            {
                self.bump();
                self.consume_digits();
            }
            if self.at_char('.') && self.peek_next() != Some('.') {
                self.bump();
                self.consume_digits();
            }
        }
        if self.at_char('e') || self.at_char('E') {
            self.bump();
            if self.at_char('+') || self.at_char('-') {
                self.bump();
            }
            self.consume_digits();
        }
        if self.at_char('j') || self.at_char('J') {
            self.bump();
        }
        self.emit(Kind::Number, start, self.pos(), false);
    }

    fn consume_digits(&mut self) {
        while self.peek().is_some_and(|ch| ch.is_ascii_hexdigit() || ch == '_') {
            self.bump();
        }
    }

    fn take_op(&mut self) -> Result<Emitted, LexError> {
        let rest = &self.src[self.i..];
        let n = op_len(rest);
        let at = self.pos();
        if n == 1 && self.peek().is_some_and(|ch| !known_punct(ch)) {
            self.problem(at, DiagnosticReason::UnexpectedToken);
        }
        self.i += n.min(rest.len()).max(1).min(self.src.len() - self.i);
        // Multi-byte non-ascii was measured in chars by op_len; ascii ops match bytes.
        if !self.src.is_char_boundary(self.i) {
            while self.i < self.src.len() && !self.src.is_char_boundary(self.i) {
                self.i += 1;
            }
        }
        self.emit(Kind::Punct, at, self.pos(), false);
        let op = self.src.get(at as usize..self.i).unwrap_or("");
        let emitted = match op {
            "(" | "[" | "{" => Emitted::Open(op.as_bytes()[0]),
            ")" | "]" | "}" => Emitted::Close(op.as_bytes()[0]),
            _ => Emitted::Other,
        };
        if self.track_parens {
            match emitted {
                Emitted::Open(kind) => self.push_paren(kind)?,
                Emitted::Close(kind) => self.pop_paren(kind),
                _ => {}
            }
        }
        Ok(emitted)
    }

    #[allow(clippy::cast_possible_truncation, reason = "Source offsets, line counts and token counts fit u32 because the public extractor enforces the 8 MiB input ceiling.")]
    fn push_paren(&mut self, kind: u8) -> Result<(), LexError> {
        if self.parens.len() as u32 >= self.max_depth {
            return Err(LexError::TooDeep {
                line: self.line_now(),
                limit: self.max_depth,
            });
        }
        self.parens.push((kind, self.pos()));
        Ok(())
    }

    fn pop_paren(&mut self, kind: u8) {
        match self.parens.last().copied() {
            Some((open, _)) if closes(open, kind) => {
                self.parens.pop();
            }
            _ => self.problem(self.pos(), DiagnosticReason::UnbalancedBracket),
        }
    }
}

fn closes(open: u8, close: u8) -> bool {
    matches!((open, close), (b'(', b')') | (b'[', b']') | (b'{', b'}'))
}

fn ident_start(ch: char) -> bool {
    ch == '_' || ch.is_alphabetic()
}

fn ident_cont(ch: char) -> bool {
    ch == '_' || ch.is_alphanumeric()
}

fn known_punct(ch: char) -> bool {
    matches!(
        ch,
        '(' | ')' | '[' | ']' | '{' | '}' | ':' | ';' | ',' | '.' | '=' | '+' | '-' | '*'
            | '/' | '%' | '&' | '|' | '^' | '~' | '<' | '>' | '@' | '\\'
    )
}

fn op_len(rest: &str) -> usize {
    const THREE: &[&str] = &["**=", "//=", "<<=", ">>=", "..."];
    const TWO: &[&str] = &[
        "->", ":=", "==", "!=", "<=", ">=", "//", "**", "<<", ">>", "+=", "-=", "*=", "/=",
        "%=", "&=", "|=", "^=", "@=",
    ];
    for op in THREE {
        if rest.starts_with(op) {
            return op.len();
        }
    }
    for op in TWO {
        if rest.starts_with(op) {
            return op.len();
        }
    }
    rest.chars().next().map_or(1, char::len_utf8)
}
