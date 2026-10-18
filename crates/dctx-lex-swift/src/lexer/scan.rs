//! Swift scanner for comments, names, operators and compiler directives.
//! Implements PARSERS-v1 bounded lexing without evaluating conditions.
//! The sibling strings module handles literals and interpolation.
use super::{Kind, Token};
use crate::domain::{DiagnosticReason, LexError, Limits};

pub(super) struct Scanner<'a> {
    pub src: &'a str,
    pub pos: usize,
    pub tokens: Vec<Token>,
    pub comments: Vec<(u32, u32)>,
    pub problems: Vec<(u32, DiagnosticReason)>,
    pub limits: &'a Limits,
}

impl<'a> Scanner<'a> {
    pub fn new(src: &'a str, limits: &'a Limits) -> Self {
        Self {
            src,
            pos: 0,
            tokens: Vec::new(),
            comments: Vec::new(),
            problems: Vec::new(),
            limits,
        }
    }

    pub fn depth(&self, depth: u32) -> Result<(), LexError> {
        if depth > self.limits.max_depth {
            return Err(LexError::TooDeep {
                line: self.src[..self.pos].bytes().filter(|b| *b == b'\n').count() as u32 + 1,
                limit: self.limits.max_depth,
            });
        }
        Ok(())
    }

    pub fn emit(&mut self, start: usize, kind: Kind) {
        self.tokens.push(Token {
            start: start as u32,
            end: self.pos as u32,
            kind,
        });
    }

    pub fn problem(&mut self, start: usize, reason: DiagnosticReason) {
        if self.problems.len() < crate::LexedFile::MAX_DIAGNOSTICS {
            self.problems.push((start as u32, reason));
        }
    }

    pub fn region(&mut self, interpolation: bool, depth: u32) -> Result<bool, LexError> {
        self.depth(depth)?;
        let mut parens = 0u32;
        while self.pos < self.src.len() {
            let start = self.pos;
            let rest = &self.src[start..];
            let ch = rest.chars().next().unwrap_or(' ');
            if ch.is_whitespace() {
                self.pos += ch.len_utf8();
                continue;
            }
            if rest.starts_with("//") {
                self.pos += rest.find('\n').unwrap_or(rest.len());
                self.comments.push((start as u32, self.pos as u32));
                continue;
            }
            if rest.starts_with("/*") {
                self.comment(depth)?;
                continue;
            }
            let hashes = rest.bytes().take_while(|b| *b == b'#').count();
            if rest.as_bytes().get(hashes) == Some(&b'"') {
                self.string(hashes, depth + 1)?;
                continue;
            }
            if ch == '#' {
                self.pos += 1;
                self.word();
                self.emit(start, Kind::Directive);
                continue;
            }
            if ch == '`' {
                self.pos += 1;
                while self.pos < self.src.len() && !self.src[self.pos..].starts_with(['`', '\n']) {
                    self.advance();
                }
                if self.src[self.pos..].starts_with('`') {
                    self.pos += 1;
                } else {
                    self.problem(start, DiagnosticReason::UnsupportedName);
                }
                self.emit(start, Kind::Ident);
                continue;
            }
            if ch.is_alphabetic() || ch == '_' || ch == '$' {
                self.word();
                self.emit(start, Kind::Ident);
                continue;
            }
            if ch.is_ascii_digit() {
                self.word();
                self.emit(start, Kind::Number);
                continue;
            }
            if ch == '(' {
                parens += 1;
                self.depth(depth + parens)?;
            }
            if ch == ')' {
                if interpolation && parens == 0 {
                    self.pos += 1;
                    self.emit(start, Kind::Punct);
                    return Ok(true);
                }
                parens = parens.saturating_sub(1);
            }
            self.pos += ch.len_utf8();
            if is_operator(ch) || rest.starts_with("..") {
                while let Some(next) = self.src[self.pos..].chars().next() {
                    if !(is_operator(next) || (ch == '.' && next == '.'))
                        || self.src[self.pos..].starts_with("//")
                        || self.src[self.pos..].starts_with("/*")
                    {
                        break;
                    }
                    self.pos += next.len_utf8();
                }
            }
            self.emit(start, Kind::Punct);
        }
        Ok(!interpolation)
    }

    fn word(&mut self) {
        while let Some(ch) = self.src[self.pos..].chars().next() {
            if !(ch.is_alphanumeric() || ch == '_' || ch == '$') {
                break;
            }
            self.pos += ch.len_utf8();
        }
    }

    pub fn advance(&mut self) {
        self.pos += self.src[self.pos..]
            .chars()
            .next()
            .map_or(0, char::len_utf8);
    }

    fn comment(&mut self, outer: u32) -> Result<(), LexError> {
        let start = self.pos;
        let mut nesting = 1;
        self.pos += 2;
        self.depth(outer + nesting)?;
        while self.pos < self.src.len() {
            if self.src[self.pos..].starts_with("/*") {
                nesting += 1;
                self.depth(outer + nesting)?;
                self.pos += 2;
            } else if self.src[self.pos..].starts_with("*/") {
                nesting -= 1;
                self.pos += 2;
                if nesting == 0 {
                    break;
                }
            } else {
                self.advance();
            }
        }
        if nesting != 0 {
            self.problem(start, DiagnosticReason::UnterminatedComment);
        }
        self.comments.push((start as u32, self.pos as u32));
        Ok(())
    }
}

fn is_operator(ch: char) -> bool {
    matches!(
        ch,
        '/' | '=' | '-' | '+' | '!' | '*' | '%' | '<' | '>' | '&' | '|' | '^' | '~' | '?'
    ) || matches!(ch as u32,
        0x00a1..=0x00a7 | 0x00a9 | 0x00ab | 0x00ac | 0x00ae | 0x00b0..=0x00b1
        | 0x00b6 | 0x00bb | 0x00bf | 0x00d7 | 0x00f7 | 0x2016..=0x2017
        | 0x2020..=0x2027 | 0x2030..=0x203e | 0x2041..=0x2053 | 0x2055..=0x205e
        | 0x2190..=0x23ff | 0x2500..=0x2775 | 0x2794..=0x2bff | 0x2e00..=0x2e7f
        | 0x3001..=0x3003 | 0x3008..=0x3020 | 0x3030)
}
