//! Go scanner: whitespace, comments, idents, numbers, strings, runes and ASI.
//! Implements the Go spec semicolon rules and the string/comment masking that
//! keeps literals from producing declarations. Used only by `lexer::lex`.
use super::{Kind, Problems, Token};
use crate::domain::DiagnosticReason;

const OPS: &[&[u8]] = &[
    b"<<=", b">>=", b"&^=", b"...", b"<-", b"++", b"--", b"==", b"!=", b"<=", b">=", b"&&", b"||",
    b"+=", b"-=", b"*=", b"/=", b"%=", b"&=", b"|=", b"^=", b"<<", b">>", b"&^", b":=",
];

pub(crate) struct Lexer<'a> {
    src: &'a str,
    b: &'a [u8],
    pos: usize,
    pub tokens: Vec<Token>,
    pub comments: Vec<(u32, u32)>,
    pub problems: Problems,
}

impl<'a> Lexer<'a> {
    pub fn new(src: &'a str) -> Self {
        Self {
            src,
            b: src.as_bytes(),
            pos: 0,
            tokens: Vec::new(),
            comments: Vec::new(),
            problems: Vec::new(),
        }
    }

    pub fn run(&mut self) {
        let mut insert = false;
        while self.pos < self.b.len() {
            let byte = self.b[self.pos];
            if matches!(byte, b' ' | b'\t' | b'\r') {
                self.pos += 1;
                continue;
            }
            if byte == b'\n' {
                if insert {
                    self.push_semi(self.pos);
                    insert = false;
                }
                self.pos += 1;
                continue;
            }
            if byte == b'/' && self.b.get(self.pos + 1) == Some(&b'/') {
                if insert {
                    self.push_semi(self.pos);
                    insert = false;
                }
                self.line_comment();
                continue;
            }
            if byte == b'/' && self.b.get(self.pos + 1) == Some(&b'*') {
                let start = self.pos;
                let newline = self.block_comment();
                if newline && insert {
                    self.push_semi(start);
                    insert = false;
                }
                continue;
            }
            let start = self.pos;
            let kind = self.scan_one();
            if self.pos == start {
                self.pos += 1;
            }
            self.push_scanned(kind, start);
            insert = self.asi(kind, start);
        }
        if insert {
            self.push_semi(self.pos);
        }
    }

    fn scan_one(&mut self) -> Kind {
        let byte = self.b[self.pos];
        if byte == b'"' || byte == b'\'' {
            return self.scan_quoted(byte);
        }
        if byte == b'`' {
            return self.scan_raw();
        }
        if byte.is_ascii_digit() {
            self.scan_number();
            return Kind::Number;
        }
        if byte == b';' {
            self.pos += 1;
            return Kind::Semi;
        }
        if self.try_op() {
            return Kind::Punct;
        }
        let ch = self.char_at(self.pos);
        if starts_ident(ch) {
            self.scan_ident();
            return Kind::Ident;
        }
        self.pos += ch.len_utf8().max(1);
        Kind::Punct
    }

    #[allow(clippy::cast_possible_truncation, reason = "Source offsets, line counts and token counts fit u32 because the public extractor enforces the 8 MiB input ceiling.")]
    fn scan_quoted(&mut self, quote: u8) -> Kind {
        let start = self.pos;
        self.pos += 1;
        while self.pos < self.b.len() {
            let byte = self.b[self.pos];
            if byte == quote {
                self.pos += 1;
                return if quote == b'\'' { Kind::Rune } else { Kind::Str };
            }
            if byte == b'\n' {
                break;
            }
            self.pos += 1;
            if byte == b'\\' && self.pos < self.b.len() && self.b[self.pos] != b'\n' {
                self.pos += 1;
            }
        }
        let reason = if quote == b'\'' {
            DiagnosticReason::UnterminatedRune
        } else {
            DiagnosticReason::UnterminatedString
        };
        self.problems.push((start as u32, reason));
        if quote == b'\'' { Kind::BadRune } else { Kind::BadStr }
    }

    #[allow(clippy::cast_possible_truncation, reason = "Source offsets, line counts and token counts fit u32 because the public extractor enforces the 8 MiB input ceiling.")]
    fn scan_raw(&mut self) -> Kind {
        let start = self.pos;
        self.pos += 1;
        while self.pos < self.b.len() {
            if self.b[self.pos] == b'`' {
                self.pos += 1;
                return Kind::Raw;
            }
            self.pos += 1;
        }
        self.problems
            .push((start as u32, DiagnosticReason::UnterminatedString));
        Kind::BadStr
    }

    fn scan_number(&mut self) {
        let hex = self.b[self.pos] == b'0'
            && matches!(self.b.get(self.pos + 1), Some(b'x' | b'X'));
        let base = self.b[self.pos] == b'0'
            && matches!(self.b.get(self.pos + 1), Some(b'x' | b'X' | b'b' | b'B' | b'o' | b'O'));
        if base {
            self.pos += 2;
            self.scan_digits(hex);
            if hex && self.b.get(self.pos) == Some(&b'.') && self.hex_at(self.pos + 1) {
                self.pos += 1;
                self.scan_digits(true);
            }
            if matches!(self.b.get(self.pos), Some(b'p' | b'P')) {
                self.scan_exponent(true);
            }
        } else {
            self.scan_digits(false);
            // `1.` and `1.e2` are one float so a following newline inserts a semicolon.
            if self.b.get(self.pos) == Some(&b'.') {
                self.pos += 1;
                if self.digit_at(self.pos, false) {
                    self.scan_digits(false);
                }
            }
            if matches!(self.b.get(self.pos), Some(b'e' | b'E')) {
                self.scan_exponent(false);
            }
        }
        if self.b.get(self.pos) == Some(&b'i') {
            self.pos += 1;
        }
    }

    fn scan_digits(&mut self, hex: bool) {
        while self.digit_at(self.pos, hex) || self.b.get(self.pos) == Some(&b'_') {
            self.pos += 1;
        }
    }

    fn scan_exponent(&mut self, hex: bool) {
        let sign_at = self.pos + 1;
        let signed = matches!(self.b.get(sign_at), Some(b'+' | b'-'));
        let digit_at = if signed { sign_at + 1 } else { sign_at };
        if self.digit_at(digit_at, hex) {
            self.pos = digit_at;
            self.scan_digits(hex);
        }
    }

    fn digit_at(&self, index: usize, hex: bool) -> bool {
        let Some(byte) = self.b.get(index) else {
            return false;
        };
        if hex {
            byte.is_ascii_hexdigit()
        } else {
            byte.is_ascii_digit()
        }
    }

    fn hex_at(&self, index: usize) -> bool {
        self.b.get(index).is_some_and(u8::is_ascii_hexdigit)
    }

    fn scan_ident(&mut self) {
        while self.pos < self.b.len() {
            let ch = self.char_at(self.pos);
            if !continues_ident(ch) {
                break;
            }
            self.pos += ch.len_utf8();
        }
    }

    fn try_op(&mut self) -> bool {
        let rest = &self.b[self.pos..];
        for op in OPS {
            if rest.starts_with(op) {
                self.pos += op.len();
                return true;
            }
        }
        if rest
            .first()
            .is_some_and(|byte| b"+-*/%&|^<>=!(),.[]{}:~".contains(byte))
        {
            self.pos += 1;
            return true;
        }
        false
    }

    #[allow(clippy::cast_possible_truncation, reason = "Source offsets, line counts and token counts fit u32 because the public extractor enforces the 8 MiB input ceiling.")]
    fn line_comment(&mut self) {
        let start = self.pos;
        self.pos += 2;
        while self.pos < self.b.len() && self.b[self.pos] != b'\n' {
            self.pos += 1;
        }
        let end = self.pos;
        if self.pos < self.b.len() {
            self.pos += 1;
        }
        self.comments.push((start as u32, end as u32));
    }

    #[allow(clippy::cast_possible_truncation, reason = "Source offsets, line counts and token counts fit u32 because the public extractor enforces the 8 MiB input ceiling.")]
    fn block_comment(&mut self) -> bool {
        let start = self.pos;
        self.pos += 2;
        let mut newline = false;
        while self.pos + 1 < self.b.len()
            && !(self.b[self.pos] == b'*' && self.b[self.pos + 1] == b'/')
        {
            if self.b[self.pos] == b'\n' {
                newline = true;
            }
            self.pos += 1;
        }
        if self.pos + 1 >= self.b.len() {
            self.pos = self.b.len();
            self.problems
                .push((start as u32, DiagnosticReason::UnterminatedComment));
            self.comments.push((start as u32, self.pos as u32));
            return newline;
        }
        self.pos += 2;
        self.comments.push((start as u32, self.pos as u32));
        newline
    }

    fn asi(&self, kind: Kind, start: usize) -> bool {
        let end = self.pos;
        match kind {
            Kind::Ident => {
                let word = self.src.get(start..end).unwrap_or("");
                !super::keyword(word)
                    || matches!(word, "break" | "continue" | "fallthrough" | "return")
            }
            Kind::Number | Kind::Str | Kind::Raw | Kind::Rune | Kind::BadStr | Kind::BadRune => true,
            Kind::Punct => matches!(self.src.get(start..end), Some("++" | "--" | ")" | "]" | "}")),
            Kind::Semi => false,
        }
    }

    #[allow(clippy::cast_possible_truncation, reason = "Source offsets, line counts and token counts fit u32 because the public extractor enforces the 8 MiB input ceiling.")]
    fn push_semi(&mut self, at: usize) {
        let at = at as u32;
        self.tokens.push(Token {
            kind: Kind::Semi,
            start: at,
            end: at,
        });
    }

    fn char_at(&self, index: usize) -> char {
        self.src[index..].chars().next().unwrap_or('\u{FFFD}')
    }
}

fn starts_ident(ch: char) -> bool {
    ch == '_' || ch.is_alphabetic()
}

fn continues_ident(ch: char) -> bool {
    ch == '_' || ch.is_alphanumeric()
}

impl Lexer<'_> {
    /// Records the token that `scan_one` just finished. Split from `run` so the
    /// scan loop stays a single screen. `end` is the scanner cursor.
    #[allow(clippy::cast_possible_truncation, reason = "Source offsets, line counts and token counts fit u32 because the public extractor enforces the 8 MiB input ceiling.")]
    fn push_scanned(&mut self, kind: Kind, start: usize) {
        self.tokens.push(Token {
            kind,
            start: start as u32,
            end: self.pos as u32,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{DiagnosticReason, Limits};
    use crate::lexer::lex;

    #[test]
    fn asi_strings_runes_and_comments_do_not_leak_tokens() {
        let lexed = lex("var x = 1\nvar y = 2\n", &Limits::default()).unwrap();
        let semis = lexed.tokens.iter().filter(|token| token.kind == Kind::Semi).count();
        assert!(semis >= 2);
        let source = "var s = \"func F() {}\"\nvar t = `func G()`\nvar r = '\\n'\n// func H()\n";
        let hidden = lex(source, &Limits::default()).unwrap();
        let words: Vec<&str> = hidden
            .tokens
            .iter()
            .map(|token| source.get(token.start as usize..token.end as usize).unwrap_or(""))
            .collect();
        assert!(!words.contains(&"F") && !words.contains(&"G") && !words.contains(&"H"));
        assert!(words.contains(&"s") && words.contains(&"t"));
        assert!(hidden.diagnostics.is_empty(), "{:?}", hidden.diagnostics);
        let open = lex("var s = \"hi\n", &Limits::default()).unwrap();
        assert!(open.diagnostics.iter().any(|d| d.reason == DiagnosticReason::UnterminatedString));
    }
}
