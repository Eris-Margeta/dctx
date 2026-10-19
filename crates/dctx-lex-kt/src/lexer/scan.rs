//! Kotlin scanner: comments, strings, templates, raw strings and newline ends.
//! Implements the string and comment masking that keeps literals from producing
//! declarations, plus `${}` holes scanned as real tokens. Used only by `lexer::lex`.
use super::{Comment, Kind, Problems, Token};
use crate::domain::DiagnosticReason;

const OPS: &[&[u8]] = &[
    b"===", b"!==", b"<<=", b">>=", b"==", b"!=", b"<=", b">=", b"&&", b"||", b"+=", b"-=", b"*=",
    b"/=", b"%=", b"->", b"::", b"?:", b"..<", b"..", b"!!",
];

/// Words that do not end a statement, so a following newline is not a semicolon.
pub(super) const PREFIX: &[&str] = &[
    "abstract", "actual", "annotation", "as", "by", "catch", "class", "companion", "const",
    "constructor", "crossinline", "data", "do", "else", "enum", "expect", "external", "final",
    "finally", "for", "fun", "if", "import", "in", "infix", "init", "inline", "inner",
    "interface", "internal", "is", "lateinit", "noinline", "object", "open", "operator", "out",
    "override", "package", "private", "protected", "public", "reified", "sealed", "suspend",
    "tailrec", "try", "typealias", "val", "value", "var", "vararg", "when", "where", "while",
];

pub(crate) struct Lexer<'a> {
    pub(super) src: &'a str,
    pub(super) b: &'a [u8],
    pub(super) pos: usize,
    pub(super) tokens: Vec<Token>,
    pub(super) comments: Vec<Comment>,
    pub(super) problems: Problems,
    pub(super) groups: u32,
    pub(super) insert: bool,
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
            groups: 0,
            insert: false,
        }
    }

    pub fn run(&mut self, max_depth: u32) -> Result<(), u32> {
        self.shebang();
        while self.pos < self.b.len() {
            let before = self.pos;
            self.step(max_depth)?;
            if self.pos == before {
                self.bump();
            }
        }
        if self.insert && self.groups == 0 {
            self.push_semi(self.pos);
        }
        Ok(())
    }

    fn step(&mut self, max_depth: u32) -> Result<(), u32> {
        self.skip_space();
        if self.pos >= self.b.len() {
            return Ok(());
        }
        let byte = self.b[self.pos];
        if byte == b'\n' {
            self.on_newline();
            return Ok(());
        }
        if byte == b'/' && self.comment_at(self.pos) {
            return self.consume_comment(max_depth);
        }
        if byte == b'@' {
            return self.scan_annotation(max_depth);
        }
        if byte == b'"' {
            let raw = self.b.get(self.pos + 1) == Some(&b'"') && self.b.get(self.pos + 2) == Some(&b'"');
            return self.scan_string(raw, max_depth);
        }
        if byte == b'\'' {
            self.scan_char();
            return Ok(());
        }
        if byte == b'`' {
            self.scan_ticks();
            return Ok(());
        }
        if byte == b';' {
            let start = self.pos;
            self.pos += 1;
            self.push(Kind::Semi, start);
            return Ok(());
        }
        if byte.is_ascii_digit() {
            let start = self.pos;
            self.scan_number();
            self.push(Kind::Num, start);
            return Ok(());
        }
        let ch = self.char_at(self.pos);
        if starts_ident(ch) {
            let start = self.pos;
            self.scan_ident();
            self.push(Kind::Ident, start);
            return Ok(());
        }
        let start = self.pos;
        if !self.try_op() {
            self.bump();
        }
        self.push(Kind::Punct, start);
        Ok(())
    }

    fn scan_annotation(&mut self, max_depth: u32) -> Result<(), u32> {
        let start = self.pos;
        self.pos += 1;
        self.push(Kind::Punct, start);
        self.skip_gap();
        self.scan_ann_name();
        self.skip_space();
        if self.pos < self.b.len() && self.b[self.pos] == b'(' {
            let base = self.groups;
            self.step(max_depth)?;
            while self.pos < self.b.len() && self.groups > base {
                let before = self.pos;
                self.step(max_depth)?;
                if self.pos == before {
                    self.bump();
                }
            }
        }
        self.insert = false;
        Ok(())
    }

    fn scan_ann_name(&mut self) {
        if !self.consume_ident() {
            return;
        }
        self.skip_gap();
        if self.peek(b':') && self.b.get(self.pos + 1) != Some(&b':') {
            let start = self.pos;
            self.pos += 1;
            self.push(Kind::Punct, start);
            self.skip_gap();
            self.consume_ident();
        }
        loop {
            self.skip_gap();
            if self.peek(b'.') && self.b.get(self.pos + 1) != Some(&b'.') {
                let start = self.pos;
                self.pos += 1;
                self.push(Kind::Punct, start);
                self.skip_gap();
                if !self.consume_ident() {
                    break;
                }
                continue;
            }
            break;
        }
    }

    fn scan_string(&mut self, raw: bool, max_depth: u32) -> Result<(), u32> {
        let start = self.pos;
        self.pos += if raw { 3 } else { 1 };
        let mut chunk = self.pos;
        let mut emitted = false;
        loop {
            if self.pos >= self.b.len() || (!raw && self.b[self.pos] == b'\n') {
                self.fail_string(start, raw, emitted);
                return Ok(());
            }
            if !raw && self.b[self.pos] == b'\\' {
                self.pos += 1;
                if self.pos < self.b.len() && self.b[self.pos] != b'\n' {
                    self.bump();
                }
                continue;
            }
            if raw && self.b[self.pos..].starts_with(b"\"\"\"") {
                self.emit_chunk(Kind::Raw, chunk);
                self.pos += 3;
                self.insert = true;
                return Ok(());
            }
            if !raw && self.b[self.pos] == b'"' {
                self.emit_chunk(Kind::Str, chunk);
                self.pos += 1;
                self.insert = true;
                return Ok(());
            }
            if self.b[self.pos] == b'$' && self.template_at() {
                let kind = if raw { Kind::Raw } else { Kind::Str };
                emitted |= self.emit_chunk(kind, chunk);
                self.pos += 1;
                if self.b.get(self.pos) == Some(&b'{') {
                    self.pos += 1;
                    if !self.scan_interp(max_depth)? {
                        self.fail_string(start, raw, emitted);
                        return Ok(());
                    }
                } else {
                    let id = self.pos;
                    self.scan_ident();
                    self.push(Kind::Ident, id);
                }
                chunk = self.pos;
                continue;
            }
            self.bump();
        }
    }

    fn scan_interp(&mut self, max_depth: u32) -> Result<bool, u32> {
        let mut depth = 1u32;
        if depth > max_depth {
            return Err(self.pos as u32);
        }
        while self.pos < self.b.len() && depth > 0 {
            self.skip_space();
            if self.pos >= self.b.len() {
                break;
            }
            if self.b[self.pos] == b'\n' {
                self.on_newline();
                continue;
            }
            if self.b[self.pos] == b'/' && self.comment_at(self.pos) {
                self.consume_comment(max_depth)?;
                continue;
            }
            if self.b[self.pos] == b'{' {
                depth += 1;
                if depth > max_depth {
                    return Err(self.pos as u32);
                }
                let at = self.pos;
                self.pos += 1;
                self.push(Kind::Punct, at);
                continue;
            }
            if self.b[self.pos] == b'}' {
                depth -= 1;
                if depth == 0 {
                    self.pos += 1;
                    return Ok(true);
                }
                let at = self.pos;
                self.pos += 1;
                self.push(Kind::Punct, at);
                continue;
            }
            let before = self.pos;
            self.step(max_depth)?;
            if self.pos == before {
                self.bump();
            }
        }
        Ok(false)
    }

    fn fail_string(&mut self, start: usize, raw: bool, emitted: bool) {
        let reason = if raw {
            DiagnosticReason::UnterminatedRaw
        } else {
            DiagnosticReason::UnterminatedString
        };
        self.problems.push((start as u32, reason));
        if !emitted {
            let kind = if raw { Kind::BadRaw } else { Kind::BadStr };
            self.push(kind, start);
        }
        self.insert = true;
    }

    fn emit_chunk(&mut self, kind: Kind, from: usize) -> bool {
        if from >= self.pos {
            return false;
        }
        self.push(kind, from);
        true
    }

    fn template_at(&self) -> bool {
        let Some(next) = self.b.get(self.pos + 1) else {
            return false;
        };
        *next == b'{' || starts_ident(self.char_at(self.pos + 1))
    }

    fn scan_char(&mut self) {
        let start = self.pos;
        self.pos += 1;
        if self.pos < self.b.len() && self.b[self.pos] == b'\\' {
            self.pos += 1;
            if self.b.get(self.pos) == Some(&b'u') {
                self.pos += 1;
                for _ in 0..4 {
                    if self.b.get(self.pos).is_some_and(u8::is_ascii_hexdigit) {
                        self.pos += 1;
                    }
                }
            } else if self.pos < self.b.len() && self.b[self.pos] != b'\n' {
                self.bump();
            }
        } else if self.pos < self.b.len() && self.b[self.pos] != b'\'' && self.b[self.pos] != b'\n' {
            self.bump();
        }
        if self.pos < self.b.len() && self.b[self.pos] == b'\'' {
            self.pos += 1;
            self.push(Kind::Char, start);
            return;
        }
        self.problems
            .push((start as u32, DiagnosticReason::UnterminatedChar));
        self.push(Kind::BadChar, start);
    }

    fn scan_ticks(&mut self) {
        let start = self.pos;
        self.pos += 1;
        while self.pos < self.b.len() && self.b[self.pos] != b'`' && self.b[self.pos] != b'\n' {
            self.bump();
        }
        if self.pos < self.b.len() && self.b[self.pos] == b'`' {
            self.pos += 1;
            self.push(Kind::Ident, start);
            return;
        }
        self.problems
            .push((start as u32, DiagnosticReason::UnterminatedString));
        self.push(Kind::BadStr, start);
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

    fn consume_ident(&mut self) -> bool {
        if self.pos >= self.b.len() || !starts_ident(self.char_at(self.pos)) {
            return false;
        }
        let start = self.pos;
        self.scan_ident();
        self.push(Kind::Ident, start);
        true
    }

    fn try_op(&mut self) -> bool {
        let rest = &self.b[self.pos..];
        for op in OPS {
            if rest.starts_with(op) {
                self.pos += op.len();
                return true;
            }
        }
        if rest.first().is_some_and(|byte| b"+-*/%&|^<>=!(),.[]{}:~?".contains(byte)) {
            self.pos += 1;
            return true;
        }
        false
    }

    fn consume_comment(&mut self, max_depth: u32) -> Result<(), u32> {
        if self.b.get(self.pos + 1) == Some(&b'/') {
            self.line_comment();
            return Ok(());
        }
        self.block_comment(max_depth)
    }

    fn line_comment(&mut self) {
        if self.insert && self.groups == 0 {
            self.push_semi(self.pos);
        }
        let start = self.pos;
        self.pos += 2;
        while self.pos < self.b.len() && self.b[self.pos] != b'\n' {
            self.bump();
        }
        let end = self.pos;
        if self.pos < self.b.len() {
            self.pos += 1;
        }
        self.comments.push(Comment {
            start: start as u32,
            end: end as u32,
            javadoc: false,
        });
        self.insert = false;
    }

    fn block_comment(&mut self, max_depth: u32) -> Result<(), u32> {
        let start = self.pos;
        let newline_before = self.insert && self.groups == 0 && self.comment_has_newline(start);
        if newline_before {
            self.push_semi(start);
        }
        self.pos += 2;
        let javadoc = self.b.get(self.pos) == Some(&b'*') && self.b.get(self.pos + 1) != Some(&b'/');
        let mut depth = 1u32;
        while self.pos < self.b.len() && depth > 0 {
            if self.b[self.pos] == b'/' && self.b.get(self.pos + 1) == Some(&b'*') {
                depth += 1;
                if depth > max_depth {
                    return Err(self.pos as u32);
                }
                self.pos += 2;
                continue;
            }
            if self.b[self.pos] == b'*' && self.b.get(self.pos + 1) == Some(&b'/') {
                depth -= 1;
                self.pos += 2;
                continue;
            }
            self.bump();
        }
        if depth > 0 {
            self.problems
                .push((start as u32, DiagnosticReason::UnterminatedComment));
        }
        self.comments.push(Comment {
            start: start as u32,
            end: self.pos as u32,
            javadoc,
        });
        self.insert = false;
        Ok(())
    }

    fn comment_has_newline(&self, start: usize) -> bool {
        let mut i = start + 2;
        let mut depth = 1u32;
        while i < self.b.len() && depth > 0 {
            if self.b[i] == b'\n' {
                return true;
            }
            if self.b[i] == b'/' && self.b.get(i + 1) == Some(&b'*') {
                depth += 1;
                i += 2;
                continue;
            }
            if self.b[i] == b'*' && self.b.get(i + 1) == Some(&b'/') {
                depth -= 1;
                i += 2;
                continue;
            }
            i += 1;
        }
        false
    }

}

pub(super) fn starts_ident(ch: char) -> bool {
    ch == '_' || ch.is_alphabetic()
}

pub(super) fn continues_ident(ch: char) -> bool {
    ch == '_' || ch.is_alphanumeric()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_table_is_sorted_and_newline_ends_a_statement() {
        assert!(PREFIX.windows(2).all(|pair| pair[0] < pair[1]));
        let mut lexer = Lexer::new("val x = 1\nfun f() {}\n");
        lexer.run(8).expect("lex");
        assert!(lexer.tokens.iter().any(|token| token.kind == Kind::Semi));
    }
}
