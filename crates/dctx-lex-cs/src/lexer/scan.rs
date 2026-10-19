//! Character scanner for C# source: comments, literals, holes and directives.
//! Implements the lexical mask of PARSERS-v1 so strings cannot declare types.
//! Used only by [`super::lex`]. Bracket pairing and its depth refusal live in the parent.
use super::{Comment, Kind, Token};
use crate::domain::DiagnosticReason;

const MULTI: &[&str] = &[
    "??=", "?.", "??", "=>", "::", "&&", "||", "==", "!=", "++", "--", "+=", "-=", "*=", "/=",
    "%=", "&=", "|=", "^=", "..",
];

pub(crate) struct Scanned {
    pub tokens: Vec<Token>,
    pub comments: Vec<Comment>,
    pub problems: Vec<(u32, DiagnosticReason)>,
    pub too_deep_at: Option<u32>,
}

pub(crate) fn scan(src: &str, max_depth: u32) -> Scanned {
    let mut scanner = Scanner {
        src,
        pos: 0,
        tokens: Vec::new(),
        comments: Vec::new(),
        problems: Vec::new(),
        line_start: true,
        depth: 0,
        max_depth,
        too_deep_at: None,
    };
    if scanner.starts("\u{feff}") {
        scanner.pos += "\u{feff}".len();
    }
    scanner.drive(0);
    Scanned {
        tokens: scanner.tokens,
        comments: scanner.comments,
        problems: scanner.problems,
        too_deep_at: scanner.too_deep_at,
    }
}

struct Scanner<'a> {
    src: &'a str,
    pos: usize,
    tokens: Vec<Token>,
    comments: Vec<Comment>,
    problems: Vec<(u32, DiagnosticReason)>,
    line_start: bool,
    depth: u32,
    max_depth: u32,
    too_deep_at: Option<u32>,
}

impl<'a> Scanner<'a> {
    /// `stop` is the number of closing braces that end an interpolation hole. Zero scans to EOF.
    fn drive(&mut self, stop: usize) {
        let mut braces = 0i32;
        while self.pos < self.src.len() && self.too_deep_at.is_none() {
            let before = self.pos;
            self.skip_space();
            if self.pos >= self.src.len() {
                break;
            }
            if self.byte() == b'\n' {
                self.pos += 1;
                self.line_start = true;
                continue;
            }
            if self.hole_ends(stop, braces) {
                return;
            }
            if self.line_start && self.byte() == b'#' {
                self.preproc();
                continue;
            }
            self.line_start = false;
            if self.starts("//") {
                self.line_comment();
            } else if self.starts("/*") {
                self.block_comment();
            } else if self.byte() == b'{' {
                braces += 1;
                self.push_len(Kind::Punct, 1);
            } else if self.byte() == b'}' {
                if braces > 0 {
                    braces -= 1;
                }
                self.push_len(Kind::Punct, 1);
            } else if self.try_string() || self.try_char() || self.try_number() || self.try_ident()
            {
            } else {
                self.punct();
            }
            if self.pos == before {
                self.bump();
            }
        }
    }

    fn hole_ends(&mut self, stop: usize, braces: i32) -> bool {
        if stop == 0 || braces != 0 || self.count(b'}') < stop {
            return false;
        }
        self.pos += stop;
        true
    }

    fn preproc(&mut self) {
        let start = self.pos;
        while self.pos < self.src.len() && self.byte() != b'\n' {
            self.bump();
        }
        self.push(Kind::Preproc, start);
        if self.byte() == b'\n' {
            self.pos += 1;
            self.line_start = true;
        }
    }

    fn line_comment(&mut self) {
        let start = self.pos;
        let doc = self.starts("///") && !self.starts("////");
        while self.pos < self.src.len() && self.byte() != b'\n' {
            self.bump();
        }
        self.comments.push(Comment {
            start: start as u32,
            end: self.pos as u32,
            doc,
        });
    }

    fn block_comment(&mut self) {
        let start = self.pos;
        self.pos += 2;
        let doc = self.byte() == b'*' && self.src.as_bytes().get(self.pos + 1) != Some(&b'/');
        while self.pos < self.src.len() {
            if self.starts("*/") {
                self.pos += 2;
                self.comments.push(Comment {
                    start: start as u32,
                    end: self.pos as u32,
                    doc,
                });
                return;
            }
            self.bump();
        }
        self.problems.push((start as u32, DiagnosticReason::UnterminatedComment));
        self.comments.push(Comment {
            start: start as u32,
            end: self.pos as u32,
            doc,
        });
    }

    fn try_string(&mut self) -> bool {
        let bytes = self.src.as_bytes();
        let mut i = self.pos;
        let mut dollars = 0usize;
        while bytes.get(i) == Some(&b'$') {
            dollars += 1;
            i += 1;
        }
        let mut verbatim = false;
        if bytes.get(i) == Some(&b'@') {
            verbatim = true;
            i += 1;
            if dollars == 0 {
                while bytes.get(i) == Some(&b'$') {
                    dollars += 1;
                    i += 1;
                }
            }
        }
        if bytes.get(i) != Some(&b'"') {
            return false;
        }
        let quote_at = i;
        while bytes.get(i) == Some(&b'"') {
            i += 1;
        }
        let quotes = i - quote_at;
        let start = self.pos;
        self.pos = i;
        self.line_start = false;
        if quotes >= 3 {
            self.raw_body(start, quotes, dollars);
        } else if quotes == 1 {
            self.quoted_body(start, verbatim, dollars);
        }
        true
    }

    fn raw_body(&mut self, start: usize, quotes: usize, dollars: usize) {
        while self.pos < self.src.len() && self.too_deep_at.is_none() {
            if dollars > 0 && self.try_hole(dollars) {
                continue;
            }
            if self.raw_closer(quotes) {
                return;
            }
            self.bump();
        }
        self.problems.push((start as u32, DiagnosticReason::UnterminatedString));
    }

    /// Ends the literal at the first quote run of length `quotes` or more.
    /// A shorter run is content. A run of exactly `quotes` glued to a name
    /// (`"""Z`) is content too; the closer is the same run before `;` or EOF.
    fn raw_closer(&mut self, quotes: usize) -> bool {
        let run = self.count(b'"');
        if run < quotes {
            return false;
        }
        if run == quotes && self.src[self.pos + run..].chars().next().is_some_and(is_ident_start) {
            return false;
        }
        self.pos += quotes;
        true
    }

    fn quoted_body(&mut self, start: usize, verbatim: bool, dollars: usize) {
        while self.pos < self.src.len() && self.too_deep_at.is_none() {
            if dollars > 0 && self.try_hole(dollars) {
                continue;
            }
            if dollars == 1 && self.starts("}}") {
                self.pos += 2;
                continue;
            }
            match self.quote_step(verbatim) {
                Step::Closed => return,
                Step::Broken => {
                    self.problems.push((start as u32, DiagnosticReason::UnterminatedString));
                    return;
                }
                Step::Ate => {}
                Step::Plain => {
                    self.bump();
                }
            }
        }
        self.problems.push((start as u32, DiagnosticReason::UnterminatedString));
    }

    /// Advances over an escape, a doubled verbatim quote, or a real closer.
    fn quote_step(&mut self, verbatim: bool) -> Step {
        if verbatim {
            if self.starts("\"\"") {
                self.pos += 2;
                return Step::Ate;
            }
            if self.byte() == b'"' {
                self.pos += 1;
                return Step::Closed;
            }
            return Step::Plain;
        }
        if self.byte() == b'\\' {
            self.pos += 1;
            if self.pos < self.src.len() {
                self.bump();
            }
            return Step::Ate;
        }
        if self.byte() == b'"' {
            self.pos += 1;
            return Step::Closed;
        }
        if self.byte() == b'\n' {
            Step::Broken
        } else {
            Step::Plain
        }
    }

    fn try_hole(&mut self, dollars: usize) -> bool {
        let open = self.count(b'{');
        if open == 0 {
            return false;
        }
        if dollars == 1 && open >= 2 {
            self.pos += 2;
            return true;
        }
        if open < dollars {
            self.pos += open;
            return true;
        }
        // Extra braces in a longer run are literal text before the hole. The
        // rightmost `dollars` braces are the opener, so the hole starts after the run.
        self.pos += open;
        if !self.enter() {
            return true;
        }
        self.drive(dollars);
        self.depth = self.depth.saturating_sub(1);
        true
    }

    fn try_char(&mut self) -> bool {
        if self.byte() != b'\'' {
            return false;
        }
        let start = self.pos;
        self.pos += 1;
        if self.byte() == b'\\' {
            self.pos += 1;
            if self.pos < self.src.len() {
                self.bump();
            }
        } else if self.byte() != b'\'' && self.byte() != b'\n' && self.pos < self.src.len() {
            self.bump();
        }
        if self.byte() == b'\'' {
            self.pos += 1;
            self.push(Kind::Char, start);
        } else {
            self.problems.push((start as u32, DiagnosticReason::UnterminatedChar));
            while self.pos < self.src.len() && self.byte() != b'\'' && self.byte() != b'\n' {
                self.bump();
            }
            if self.byte() == b'\'' {
                self.pos += 1;
            }
            self.push(Kind::Char, start);
        }
        true
    }

    fn try_number(&mut self) -> bool {
        if !self.byte().is_ascii_digit() {
            return false;
        }
        let start = self.pos;
        if self.starts("0x") || self.starts("0X") || self.starts("0b") || self.starts("0B") {
            self.pos += 2;
            while self.byte().is_ascii_hexdigit() || self.byte() == b'_' {
                self.pos += 1;
            }
        } else {
            self.digits();
            if self.byte() == b'.' && self.src.as_bytes().get(self.pos + 1).is_some_and(u8::is_ascii_digit) {
                self.pos += 1;
                self.digits();
            }
            self.exponent();
        }
        while matches!(self.byte(), b'u' | b'U' | b'l' | b'L' | b'f' | b'F' | b'd' | b'D' | b'm' | b'M') {
            self.pos += 1;
        }
        self.push(Kind::Num, start);
        true
    }

    fn digits(&mut self) {
        while self.byte().is_ascii_digit() || self.byte() == b'_' {
            self.pos += 1;
        }
    }

    fn exponent(&mut self) {
        if self.byte() != b'e' && self.byte() != b'E' {
            return;
        }
        let next = self.src.as_bytes().get(self.pos + 1).copied().unwrap_or(0);
        if !next.is_ascii_digit() && next != b'+' && next != b'-' {
            return;
        }
        self.pos += 1;
        if self.byte() == b'+' || self.byte() == b'-' {
            self.pos += 1;
        }
        self.digits();
    }

    fn try_ident(&mut self) -> bool {
        if !self.ident_start() {
            return false;
        }
        let start = self.pos;
        if self.byte() == b'@' {
            self.pos += 1;
        }
        while self.peek().is_some_and(is_ident_cont) {
            self.bump();
        }
        self.push(Kind::Word, start);
        true
    }

    fn punct(&mut self) {
        let len = MULTI.iter().find(|op| self.starts(op)).map(|op| op.len()).unwrap_or(1);
        if len == 1 && !self.byte().is_ascii() {
            let start = self.pos;
            self.bump();
            self.push(Kind::Punct, start);
            return;
        }
        self.push_len(Kind::Punct, len);
    }

    fn enter(&mut self) -> bool {
        if self.depth >= self.max_depth {
            self.too_deep_at = Some(self.pos as u32);
            return false;
        }
        self.depth += 1;
        true
    }

    fn skip_space(&mut self) {
        while self.peek().is_some_and(|ch| ch.is_whitespace() && ch != '\n') {
            self.bump();
        }
    }

    fn ident_start(&self) -> bool {
        if self.byte() == b'@' {
            return self.src[self.pos + 1..].chars().next().is_some_and(is_ident_start);
        }
        self.peek().is_some_and(is_ident_start)
    }

    fn count(&self, byte: u8) -> usize {
        self.src.as_bytes()[self.pos..].iter().take_while(|item| **item == byte).count()
    }

    fn starts(&self, text: &str) -> bool {
        self.src[self.pos..].starts_with(text)
    }

    fn byte(&self) -> u8 {
        self.src.as_bytes().get(self.pos).copied().unwrap_or(0)
    }

    fn peek(&self) -> Option<char> {
        self.src[self.pos..].chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.pos += ch.len_utf8();
        Some(ch)
    }

    fn push(&mut self, kind: Kind, start: usize) {
        if self.pos < start {
            return;
        }
        self.tokens.push(Token {
            kind,
            start: start as u32,
            end: self.pos as u32,
        });
    }

    fn push_len(&mut self, kind: Kind, len: usize) {
        let start = self.pos;
        self.pos += len;
        self.push(kind, start);
    }
}

enum Step {
    Ate,
    Plain,
    Closed,
    Broken,
}

fn is_ident_start(ch: char) -> bool {
    ch == '_' || ch.is_alphabetic()
}

fn is_ident_cont(ch: char) -> bool {
    ch == '_' || ch.is_alphanumeric()
}
