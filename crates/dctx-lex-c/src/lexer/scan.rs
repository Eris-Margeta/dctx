//! C and C++ scanner: whitespace, comments, identifiers, numbers, strings and directives.
//! Implements line-comment and block-comment masking, prefixed and raw strings,
//! and preprocessor lines as single tokens. Used only by `lexer::lex`.
use super::{Comment, Kind, Problems, Token};
use crate::domain::DiagnosticReason;

const OPS: &[&[u8]] = &[
    b"<<=", b">>=", b"<=>", b"...", b"->*", b".*", b"::", b"->", b"<<", b">>", b"<=", b">=", b"==",
    b"!=", b"&&", b"||", b"++", b"--", b"+=", b"-=", b"*=", b"/=", b"%=", b"&=", b"|=", b"^=", b"##",
];

const RAW: &[&[u8]] = &[b"u8R\"", b"uR\"", b"UR\"", b"LR\"", b"R\""];
const PREFIX: &[&[u8]] = &[b"u8", b"u", b"U", b"L"];

pub(crate) struct Lexer<'a> {
    src: &'a str,
    b: &'a [u8],
    pos: usize,
    line_token: bool,
    pub tokens: Vec<Token>,
    pub comments: Vec<Comment>,
    pub problems: Problems,
}

impl<'a> Lexer<'a> {
    pub fn new(src: &'a str) -> Self {
        Self {
            src,
            b: src.as_bytes(),
            pos: 0,
            line_token: false,
            tokens: Vec::new(),
            comments: Vec::new(),
            problems: Vec::new(),
        }
    }

    pub fn run(&mut self) {
        while self.pos < self.b.len() {
            let byte = self.b[self.pos];
            if matches!(byte, b' ' | b'\t' | b'\r') {
                self.pos += 1;
                continue;
            }
            if byte == b'\n' {
                self.pos += 1;
                self.line_token = false;
                continue;
            }
            if byte == b'/' && self.b.get(self.pos + 1) == Some(&b'/') {
                self.line_comment();
                continue;
            }
            if byte == b'/' && self.b.get(self.pos + 1) == Some(&b'*') {
                self.block_comment();
                continue;
            }
            if byte == b'#' && !self.line_token {
                self.directive();
                continue;
            }
            let start = self.pos;
            let kind = self.scan_one();
            if self.pos == start {
                self.pos += 1;
            }
            self.push(kind, start);
            self.line_token = true;
        }
    }

    fn scan_one(&mut self) -> Kind {
        if let Some(kind) = self.try_raw() {
            return kind;
        }
        if let Some(kind) = self.try_prefixed() {
            return kind;
        }
        let byte = self.b[self.pos];
        if byte == b'"' {
            return self.scan_quoted(b'"', DiagnosticReason::UnterminatedString);
        }
        if byte == b'\'' {
            return self.scan_quoted(b'\'', DiagnosticReason::UnterminatedChar);
        }
        if byte.is_ascii_digit()
            || (byte == b'.' && self.b.get(self.pos + 1).is_some_and(u8::is_ascii_digit))
        {
            self.scan_number();
            return Kind::Number;
        }
        if self.try_op() {
            return Kind::Punct;
        }
        let ch = self.char_at(self.pos);
        if ident_start(ch) {
            self.scan_ident();
            return Kind::Ident;
        }
        self.pos += ch.len_utf8().max(1);
        Kind::Punct
    }

    fn try_raw(&mut self) -> Option<Kind> {
        let rest = &self.b[self.pos..];
        let prefix = RAW.iter().find(|prefix| rest.starts_with(prefix))?;
        let start = self.pos;
        self.pos += prefix.len();
        let delim_at = self.pos;
        while self.pos < self.b.len()
            && self.b[self.pos] != b'('
            && self.b[self.pos] != b'\n'
            && self.b[self.pos] != b'"'
            && self.pos - delim_at <= 16
        {
            self.pos += 1;
        }
        if self.pos >= self.b.len() || self.b[self.pos] != b'(' {
            self.problems
                .push((start as u32, DiagnosticReason::UnterminatedRawString));
            return Some(Kind::Bad);
        }
        let delim = self.b[delim_at..self.pos].to_vec();
        self.pos += 1;
        while self.pos < self.b.len() {
            if self.b[self.pos] == b')'
                && self.b[self.pos + 1..].starts_with(&delim)
                && self.b.get(self.pos + 1 + delim.len()) == Some(&b'"')
            {
                self.pos += 2 + delim.len();
                return Some(Kind::String);
            }
            self.pos += 1;
        }
        self.problems
            .push((start as u32, DiagnosticReason::UnterminatedRawString));
        Some(Kind::Bad)
    }

    fn try_prefixed(&mut self) -> Option<Kind> {
        let rest = &self.b[self.pos..];
        for prefix in PREFIX {
            if !rest.starts_with(prefix) {
                continue;
            }
            let quote = *rest.get(prefix.len())?;
            if quote == b'"' || quote == b'\'' {
                self.pos += prefix.len();
                let reason = if quote == b'\'' {
                    DiagnosticReason::UnterminatedChar
                } else {
                    DiagnosticReason::UnterminatedString
                };
                return Some(self.scan_quoted(quote, reason));
            }
        }
        None
    }

    fn scan_quoted(&mut self, quote: u8, reason: DiagnosticReason) -> Kind {
        let start = self.pos;
        self.pos += 1;
        while self.pos < self.b.len() {
            let byte = self.b[self.pos];
            if byte == quote {
                self.pos += 1;
                return Kind::String;
            }
            if byte == b'\n' {
                break;
            }
            self.pos += 1;
            if byte == b'\\' && self.pos < self.b.len() && self.b[self.pos] != b'\n' {
                self.pos += 1;
            }
        }
        self.problems.push((start as u32, reason));
        Kind::Bad
    }

    fn scan_number(&mut self) {
        let hex = self.b[self.pos] == b'0'
            && matches!(self.b.get(self.pos + 1), Some(b'x' | b'X'));
        if self.b[self.pos] == b'0'
            && matches!(self.b.get(self.pos + 1), Some(b'x' | b'X' | b'b' | b'B'))
        {
            self.pos += 2;
        }
        while self.pos < self.b.len()
            && (self.digit(self.pos, hex) || self.b[self.pos] == b'\'')
        {
            self.pos += 1;
        }
        if self.b.get(self.pos) == Some(&b'.') && self.digit(self.pos + 1, hex) {
            self.pos += 1;
            while self.pos < self.b.len() && (self.digit(self.pos, hex) || self.b[self.pos] == b'\'')
            {
                self.pos += 1;
            }
        }
        if matches!(self.b.get(self.pos), Some(b'e' | b'E' | b'p' | b'P')) {
            let sign = self.pos + 1;
            let signed = matches!(self.b.get(sign), Some(b'+' | b'-'));
            let digit_at = if signed { sign + 1 } else { sign };
            if self.digit(digit_at, hex) {
                self.pos = digit_at;
                while self.pos < self.b.len() && self.digit(self.pos, hex) {
                    self.pos += 1;
                }
            }
        }
        while self.pos < self.b.len() && matches!(self.b[self.pos], b'u' | b'U' | b'l' | b'L' | b'f' | b'F' | b'z' | b'Z')
        {
            self.pos += 1;
        }
    }

    fn digit(&self, index: usize, hex: bool) -> bool {
        let Some(byte) = self.b.get(index) else {
            return false;
        };
        if hex {
            byte.is_ascii_hexdigit()
        } else {
            byte.is_ascii_digit()
        }
    }

    fn scan_ident(&mut self) {
        while self.pos < self.b.len() {
            let ch = self.char_at(self.pos);
            if !ident_continue(ch) {
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
            .is_some_and(|byte| b"()[]{}.,;:+-*/%&|^~!?=<>#".contains(byte))
        {
            self.pos += 1;
            return true;
        }
        false
    }

    /// A directive ends at a newline, but strings, raw strings and block comments
    /// on that directive keep their own newlines (phase 3 before phase 4).
    fn directive(&mut self) {
        let start = self.pos;
        self.pos += 1;
        while self.pos < self.b.len() {
            if self.raw_at_boundary() && self.try_raw().is_some() {
                continue;
            }
            let byte = self.b[self.pos];
            if byte == b'\n' {
                break;
            }
            if byte == b'"' || byte == b'\'' {
                let reason = if byte == b'\'' {
                    DiagnosticReason::UnterminatedChar
                } else {
                    DiagnosticReason::UnterminatedString
                };
                let _ = self.scan_quoted(byte, reason);
                continue;
            }
            if byte == b'/' && self.b.get(self.pos + 1) == Some(&b'/') {
                while self.pos < self.b.len() && self.b[self.pos] != b'\n' {
                    self.pos += 1;
                }
                break;
            }
            if byte == b'/' && self.b.get(self.pos + 1) == Some(&b'*') {
                self.skip_block_in_directive();
                continue;
            }
            if self.src.is_char_boundary(self.pos) {
                let ch = self.char_at(self.pos);
                self.pos += ch.len_utf8().max(1);
            } else {
                self.pos += 1;
            }
        }
        self.push(Kind::Directive, start);
        self.line_token = true;
    }

    fn skip_block_in_directive(&mut self) {
        let start = self.pos;
        self.pos += 2;
        while self.pos + 1 < self.b.len()
            && !(self.b[self.pos] == b'*' && self.b[self.pos + 1] == b'/')
        {
            self.pos += 1;
        }
        if self.pos + 1 >= self.b.len() {
            self.pos = self.b.len();
            self.problems
                .push((start as u32, DiagnosticReason::UnterminatedComment));
            return;
        }
        self.pos += 2;
    }

    fn raw_at_boundary(&self) -> bool {
        if !self.src.is_char_boundary(self.pos) {
            return false;
        }
        if self.pos > 0 {
            let prev = self.src[..self.pos].chars().next_back().unwrap_or('\0');
            if ident_continue(prev) {
                return false;
            }
        }
        let rest = &self.b[self.pos..];
        RAW.iter().any(|prefix| rest.starts_with(prefix))
    }

    fn line_comment(&mut self) {
        let start = self.pos;
        self.pos += 2;
        while self.pos < self.b.len() && self.b[self.pos] != b'\n' {
            self.pos += 1;
        }
        let text = self.src.get(start..self.pos).unwrap_or("");
        let doc = text.starts_with("///") || text.starts_with("//!");
        self.comments.push(Comment {
            start: start as u32,
            end: self.pos as u32,
            doc,
        });
    }

    fn block_comment(&mut self) {
        let start = self.pos;
        self.pos += 2;
        while self.pos + 1 < self.b.len()
            && !(self.b[self.pos] == b'*' && self.b[self.pos + 1] == b'/')
        {
            if self.b[self.pos] == b'\n' {
                self.line_token = false;
            }
            self.pos += 1;
        }
        if self.pos + 1 >= self.b.len() {
            self.pos = self.b.len();
            self.problems
                .push((start as u32, DiagnosticReason::UnterminatedComment));
            self.comments.push(Comment {
                start: start as u32,
                end: self.pos as u32,
                doc: false,
            });
            return;
        }
        self.pos += 2;
        let text = self.src.get(start..self.pos).unwrap_or("");
        let doc = (text.starts_with("/**") && !text.starts_with("/***") && text != "/**/")
            || text.starts_with("/*!");
        self.comments.push(Comment {
            start: start as u32,
            end: self.pos as u32,
            doc,
        });
    }

    fn push(&mut self, kind: Kind, start: usize) {
        self.tokens.push(Token {
            kind,
            start: start as u32,
            end: self.pos as u32,
        });
    }

    fn char_at(&self, index: usize) -> char {
        self.src
            .get(index..)
            .and_then(|rest| rest.chars().next())
            .unwrap_or('\u{FFFD}')
    }
}

fn ident_start(ch: char) -> bool {
    ch == '_' || ch == '$' || ch.is_alphabetic()
}

fn ident_continue(ch: char) -> bool {
    ch == '_' || ch == '$' || ch.is_alphanumeric()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Limits;
    use crate::lexer::lex;

    #[test]
    fn an_open_string_is_a_diagnostic_and_not_a_declaration() {
        let lexed = lex("char *s = \"unterminated\n", &Limits::default()).unwrap();
        assert!(lexed
            .diagnostics
            .iter()
            .any(|d| d.reason == DiagnosticReason::UnterminatedString));
        let words: Vec<&str> = lexed
            .tokens
            .iter()
            .filter(|token| token.kind == Kind::Ident)
            .map(|token| lexed.logical.get(token.start as usize..token.end as usize).unwrap_or(""))
            .collect();
        assert_eq!(words, vec!["char", "s"]);
    }
}
