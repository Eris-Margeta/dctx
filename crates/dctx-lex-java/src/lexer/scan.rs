// SPDX-License-Identifier: Apache-2.0
//! Character scanner for Java source: comments, literals, text blocks and words.
//! Implements the lexical mask of PARSERS-v1 so strings and comments cannot declare types.
//! Used only by [`super::lex`]; bracket pairing and depth refusal live in the parent module.
use super::{Comment, Kind, Token};
use crate::domain::DiagnosticReason;

const MULTI: [&str; 20] = [
    "...", "::", "->", "==", "!=", "<=", ">=", "&&", "||", "++", "--", "+=", "-=", "*=", "/=",
    "%=", "&=", "|=", "^=", "<<",
];

pub(crate) struct Scanner<'a> {
    src: &'a str,
    pos: usize,
    tokens: Vec<Token>,
    comments: Vec<Comment>,
    problems: Vec<(u32, DiagnosticReason)>,
    pending_doc: u32,
}

pub(crate) fn scan(src: &str) -> (Vec<Token>, Vec<Comment>, Vec<(u32, DiagnosticReason)>) {
    let mut scanner = Scanner {
        src,
        pos: 0,
        tokens: Vec::new(),
        comments: Vec::new(),
        problems: Vec::new(),
        pending_doc: 0,
    };
    scanner.run();
    (scanner.tokens, scanner.comments, scanner.problems)
}

impl Scanner<'_> {
    fn run(&mut self) {
        if self.starts_with("\u{feff}") {
            self.pos += "\u{feff}".len();
        }
        if self.starts_with("#!") {
            while let Some(c) = self.bump() {
                if c == '\n' {
                    break;
                }
            }
        }
        while self.pos < self.src.len() {
            let before = self.pos;
            self.skip_ws();
            if self.pos >= self.src.len() {
                break;
            }
            if self.translated_len("//").is_some() {
                self.line_comment();
            } else if self.translated_len("/*").is_some() {
                self.block_comment();
            } else if self.peek() == Some('"') {
                self.string();
            } else if self.peek() == Some('\'') {
                self.char_lit();
            } else if self.number_start() {
                self.number();
            } else if self.logical_peek().is_some_and(is_ident_start) {
                self.ident();
            } else {
                self.punct();
            }
            if self.pos == before {
                self.bump();
            }
        }
    }

    fn skip_ws(&mut self) {
        while self.logical_peek().is_some_and(char::is_whitespace) {
            self.logical_bump();
        }
    }

    fn line_comment(&mut self) {
        let start = self.pos;
        while let Some(c) = self.logical_peek() {
            if c == '\n' {
                break;
            }
            self.logical_bump();
        }
        self.push_comment(start, false);
    }

    fn block_comment(&mut self) {
        let start = self.pos;
        let Some(opener) = self.translated_len("/*") else {
            self.bump();
            return;
        };
        self.pos += opener;
        let javadoc = self.peek() == Some('*')
            && self
                .src
                .get(self.pos + 1..)
                .is_some_and(|rest| !rest.starts_with('/'));
        while self.pos < self.src.len() {
            if let Some(closer) = self.translated_len("*/") {
                self.pos += closer;
                self.push_comment(start, javadoc);
                return;
            }
            if self.logical_bump().is_none() {
                break;
            }
        }
        self.problem(start, DiagnosticReason::UnterminatedComment);
        self.push_comment(start, false);
    }

    #[allow(clippy::cast_possible_truncation, reason = "Source offsets, line counts and token counts fit u32 because the public extractor enforces the 8 MiB input ceiling.")]
    fn push_comment(&mut self, start: usize, javadoc: bool) {
        let index = u32::try_from(self.comments.len()).unwrap_or(u32::MAX);
        self.comments.push(Comment {
            start: start as u32,
            end: self.pos as u32,
            javadoc,
        });
        self.pending_doc = if javadoc && index < u32::MAX {
            index + 1
        } else {
            0
        };
    }

    fn string(&mut self) {
        let start = self.pos;
        if let Some(content) = self.text_block_content_at() {
            self.pos = content;
            self.text_block(start);
            return;
        }
        self.bump();
        while let Some(c) = self.peek() {
            if c == '\n' || c == '\r' {
                self.problem(start, DiagnosticReason::UnterminatedString);
                self.emit(Kind::Str, start);
                return;
            }
            if c == '\\' {
                self.escape();
                continue;
            }
            self.bump();
            if c == '"' {
                self.emit(Kind::Str, start);
                return;
            }
        }
        self.problem(start, DiagnosticReason::UnterminatedString);
        self.emit(Kind::Str, start);
    }

    /// Byte index just after the opening line terminator, when `"""` starts a text block.
    fn text_block_content_at(&self) -> Option<usize> {
        let rest = self.src.get(self.pos..)?;
        if !rest.starts_with("\"\"\"") {
            return None;
        }
        let bytes = rest.as_bytes();
        let mut j = 3;
        while j < bytes.len() && matches!(bytes[j], b' ' | b'\t' | 0x0c) {
            j += 1;
        }
        if bytes.get(j) == Some(&b'\n') {
            return Some(self.pos + j + 1);
        }
        if bytes.get(j) == Some(&b'\r') {
            let extra = usize::from(bytes.get(j + 1) == Some(&b'\n'));
            return Some(self.pos + j + 1 + extra);
        }
        None
    }

    fn text_block(&mut self, start: usize) {
        let bytes = self.src.as_bytes();
        while self.pos < bytes.len() {
            if bytes[self.pos] == b'\\' {
                self.escape();
                continue;
            }
            if self.starts_with("\"\"\"") {
                self.pos += 3;
                self.emit(Kind::Str, start);
                return;
            }
            self.bump();
        }
        self.problem(start, DiagnosticReason::UnterminatedTextBlock);
        self.emit(Kind::Str, start);
    }

    fn char_lit(&mut self) {
        let start = self.pos;
        self.bump();
        let mut closed = false;
        while let Some(c) = self.peek() {
            if c == '\n' || c == '\r' {
                break;
            }
            if c == '\\' {
                self.escape();
                continue;
            }
            self.bump();
            if c == '\'' {
                closed = true;
                break;
            }
        }
        if !closed {
            self.problem(start, DiagnosticReason::UnterminatedChar);
        }
        self.emit(Kind::Char, start);
    }

    fn escape(&mut self) {
        self.bump();
        let Some(c) = self.peek() else {
            return;
        };
        if c == 'u' {
            while self.peek() == Some('u') {
                self.bump();
            }
            for _ in 0..4 {
                if self.peek().is_some_and(|hex| hex.is_ascii_hexdigit()) {
                    self.bump();
                } else {
                    break;
                }
            }
            return;
        }
        if c.is_ascii_digit() {
            self.bump();
            for _ in 0..2 {
                if self.peek().is_some_and(|digit| digit.is_ascii_digit()) {
                    self.bump();
                } else {
                    break;
                }
            }
            return;
        }
        self.bump();
    }

    fn number_start(&self) -> bool {
        match self.peek() {
            Some(c) if c.is_ascii_digit() => true,
            Some('.') => self.second().is_some_and(|c| c.is_ascii_digit()),
            _ => false,
        }
    }

    fn number(&mut self) {
        let start = self.pos;
        if self.byte_at(self.pos) == Some(b'0')
            && matches!(self.byte_at(self.pos + 1), Some(b'x' | b'X'))
        {
            self.pos += 2;
            self.eat_hex();
            if self.byte_at(self.pos) == Some(b'.')
                && self.byte_at(self.pos + 1).is_some_and(|b| b.is_ascii_hexdigit())
            {
                self.pos += 1;
                self.eat_hex();
            }
            if matches!(self.byte_at(self.pos), Some(b'p' | b'P')) {
                self.pos += 1;
                if matches!(self.byte_at(self.pos), Some(b'+' | b'-')) {
                    self.pos += 1;
                }
                self.eat_dec();
            }
            self.suffix();
            self.emit(Kind::Num, start);
            return;
        }
        if self.byte_at(self.pos) == Some(b'0')
            && matches!(self.byte_at(self.pos + 1), Some(b'b' | b'B'))
        {
            self.pos += 2;
            self.eat_while(|b| matches!(b, b'0' | b'1' | b'_'));
            self.suffix();
            self.emit(Kind::Num, start);
            return;
        }
        if self.byte_at(self.pos) != Some(b'.') {
            self.eat_dec();
        }
        if self.byte_at(self.pos) == Some(b'.')
            && self.byte_at(self.pos + 1).is_some_and(|b| b.is_ascii_digit())
        {
            self.pos += 1;
            self.eat_dec();
        }
        if matches!(self.byte_at(self.pos), Some(b'e' | b'E')) {
            let mut digits_at = self.pos + 1;
            if matches!(self.byte_at(digits_at), Some(b'+' | b'-')) {
                digits_at += 1;
            }
            if self.byte_at(digits_at).is_some_and(|b| b.is_ascii_digit()) {
                self.pos = digits_at;
                self.eat_dec();
            }
        }
        self.suffix();
        self.emit(Kind::Num, start);
    }

    fn eat_dec(&mut self) {
        self.eat_while(|b| b.is_ascii_digit() || b == b'_');
    }

    fn eat_hex(&mut self) {
        self.eat_while(|b| b.is_ascii_hexdigit() || b == b'_');
    }

    fn eat_while(&mut self, mut pred: impl FnMut(u8) -> bool) {
        while let Some(byte) = self.byte_at(self.pos) {
            if !pred(byte) {
                break;
            }
            self.pos += 1;
        }
    }

    fn suffix(&mut self) {
        if matches!(
            self.byte_at(self.pos),
            Some(b'l' | b'L' | b'f' | b'F' | b'd' | b'D')
        ) {
            self.pos += 1;
        }
    }

    fn ident(&mut self) {
        let start = self.pos;
        self.logical_bump();
        while self.logical_peek().is_some_and(is_ident_continue) {
            self.logical_bump();
        }
        self.emit(Kind::Word, start);
    }

    fn punct(&mut self) {
        let start = self.pos;
        for candidate in MULTI {
            if self.starts_with(candidate) {
                self.pos += candidate.len();
                self.emit(Kind::Punct, start);
                return;
            }
        }
        self.bump();
        self.emit(Kind::Punct, start);
    }

    #[allow(clippy::cast_possible_truncation, reason = "Source offsets, line counts and token counts fit u32 because the public extractor enforces the 8 MiB input ceiling.")]
    fn emit(&mut self, kind: Kind, start: usize) {
        if self.pos <= start {
            return;
        }
        self.tokens.push(Token {
            kind,
            start: start as u32,
            end: self.pos as u32,
            doc: self.pending_doc,
        });
        self.pending_doc = 0;
    }

    #[allow(clippy::cast_possible_truncation, reason = "Source offsets, line counts and token counts fit u32 because the public extractor enforces the 8 MiB input ceiling.")]
    fn problem(&mut self, at: usize, reason: DiagnosticReason) {
        self.problems.push((at as u32, reason));
    }

    fn peek(&self) -> Option<char> {
        self.src.get(self.pos..)?.chars().next()
    }

    fn second(&self) -> Option<char> {
        self.src.get(self.pos..)?.chars().nth(1)
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    fn starts_with(&self, prefix: &str) -> bool {
        self.src.get(self.pos..).is_some_and(|rest| rest.starts_with(prefix))
    }

    fn byte_at(&self, index: usize) -> Option<u8> {
        self.src.as_bytes().get(index).copied()
    }

    /// Java translates `\u` escapes before tokenization (JLS 3.3). String and
    /// character literals keep a separate escape pass on the raw bytes.
    fn logical_at(&self, pos: usize) -> Option<(char, usize)> {
        lexical_unit(self.src, pos)
    }

    fn logical_peek(&self) -> Option<char> {
        self.logical_at(self.pos).map(|(ch, _)| ch)
    }

    fn logical_bump(&mut self) -> Option<char> {
        let (ch, len) = self.logical_at(self.pos)?;
        self.pos += len;
        Some(ch)
    }

    fn translated_len(&self, prefix: &str) -> Option<usize> {
        let mut pos = self.pos;
        for ch in prefix.chars() {
            let (got, len) = self.logical_at(pos)?;
            if got != ch {
                return None;
            }
            pos += len;
        }
        Some(pos - self.pos)
    }
}

fn lexical_unit(src: &str, pos: usize) -> Option<(char, usize)> {
    let bytes = src.as_bytes();
    if pos >= bytes.len() {
        return None;
    }
    if bytes[pos] == b'\\'
        && let Some(unit) = unicode_unit(bytes, pos)
    {
        return Some(unit);
    }
    let ch = src[pos..].chars().next()?;
    Some((ch, ch.len_utf8()))
}

fn unicode_unit(bytes: &[u8], pos: usize) -> Option<(char, usize)> {
    let mut cursor = pos + 1;
    if bytes.get(cursor) != Some(&b'u') {
        return None;
    }
    while bytes.get(cursor) == Some(&b'u') {
        cursor += 1;
    }
    let end = cursor.checked_add(4)?;
    if end > bytes.len() || !bytes[cursor..end].iter().all(u8::is_ascii_hexdigit) {
        return None;
    }
    let digits = std::str::from_utf8(&bytes[cursor..end]).ok()?;
    let code = u32::from_str_radix(digits, 16).ok()?;
    Some((char::from_u32(code)?, end - pos))
}

fn is_ident_start(c: char) -> bool {
    c == '_' || c == '$' || c.is_alphabetic()
}

fn is_ident_continue(c: char) -> bool {
    is_ident_start(c) || c.is_alphanumeric()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_do_not_end_a_string_early() {
        let (tokens, _, problems) = scan(r#""a\"b" '\"' "\u0041""#);
        assert_eq!(problems, []);
        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens[0].kind, Kind::Str);
        assert_eq!(tokens[1].kind, Kind::Char);
    }
}
