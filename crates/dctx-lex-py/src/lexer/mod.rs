//! Token stream for Python source, including indentation and f-strings.
//! Implements PARSERS-v1 lexing: comments, strings, prefixes, brackets and indent,
//! all bounded by the depth ceiling. The structural walker consumes this.
mod indent;
mod scan;
mod strings;

use crate::domain::{Diagnostic, DiagnosticReason, LexError};

/// Token category. Keywords stay `Ident` and are compared by text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Ident,
    Str,
    Number,
    Punct,
    Newline,
    Indent,
    Dedent,
}

/// One token. `doc` is set only for a plain string that can be a docstring.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Token {
    pub kind: Kind,
    pub start: u32,
    pub end: u32,
    pub doc: bool,
}

/// Lexed tokens, physical comment spans and problems.
pub(crate) struct Lexed {
    pub tokens: Vec<Token>,
    pub comments: Vec<(u32, u32)>,
    pub diagnostics: Vec<Diagnostic>,
    pub line_starts: Vec<u32>,
}

impl Lexed {
    /// One-based line of a byte offset. `\n`, `\r\n` and `\r` all break lines.
    pub fn line(&self, byte: u32) -> u32 {
        self.line_starts.partition_point(|&start| start <= byte).max(1) as u32
    }
}

#[derive(Clone, Copy)]
struct Prefix {
    raw: bool,
    formatted: bool,
    doc: bool,
}

enum Emitted {
    Open(u8),
    Close(u8),
    Name,
    Other,
}

struct Lx<'a> {
    src: &'a str,
    i: usize,
    tokens: Vec<Token>,
    comments: Vec<(u32, u32)>,
    problems: Vec<(u32, DiagnosticReason)>,
    indents: Vec<usize>,
    parens: Vec<(u8, u32)>,
    at_bol: bool,
    max_depth: u32,
    line_starts: Vec<u32>,
    fstrings: u32,
    string_closed: bool,
}

pub(crate) fn lex(source: &str, max_depth: u32) -> Result<Lexed, LexError> {
    let line_starts = line_starts(source);
    let mut lx = Lx {
        src: source,
        i: 0,
        tokens: Vec::new(),
        comments: Vec::new(),
        problems: Vec::new(),
        indents: vec![0],
        parens: Vec::new(),
        at_bol: true,
        max_depth,
        line_starts,
        fstrings: 0,
        string_closed: false,
    };
    lx.run()?;
    let diagnostics = lx
        .problems
        .iter()
        .map(|&(byte, reason)| Diagnostic {
            line: line_of(&lx.line_starts, byte),
            byte,
            reason,
        })
        .collect();
    Ok(Lexed {
        tokens: lx.tokens,
        comments: lx.comments,
        diagnostics,
        line_starts: lx.line_starts,
    })
}

fn line_starts(source: &str) -> Vec<u32> {
    let bytes = source.as_bytes();
    let mut starts = vec![0];
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\n' {
            i += 1;
            starts.push(i as u32);
        } else if bytes[i] == b'\r' {
            i += 1;
            if i < bytes.len() && bytes[i] == b'\n' {
                i += 1;
            }
            starts.push(i as u32);
        } else {
            i += 1;
        }
    }
    starts
}

fn line_of(starts: &[u32], byte: u32) -> u32 {
    starts.partition_point(|&start| start <= byte).max(1) as u32
}

impl Lx<'_> {
    fn emit(&mut self, kind: Kind, start: u32, end: u32, doc: bool) {
        self.tokens.push(Token {
            kind,
            start,
            end,
            doc,
        });
    }

    fn problem(&mut self, byte: u32, reason: DiagnosticReason) {
        if self.problems.len() < 128 {
            self.problems.push((byte, reason));
        }
    }

    fn indent_top(&self) -> usize {
        self.indents.last().copied().unwrap_or(0)
    }

    fn line_now(&self) -> u32 {
        line_of(&self.line_starts, self.pos())
    }

    fn pos(&self) -> u32 {
        self.i.min(self.src.len()) as u32
    }

    fn peek(&self) -> Option<char> {
        self.src.get(self.i..)?.chars().next()
    }

    fn peek_next(&self) -> Option<char> {
        self.src.get(self.i..)?.chars().nth(1)
    }

    fn bump(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.i += ch.len_utf8();
        Some(ch)
    }

    fn at_char(&self, ch: char) -> bool {
        self.peek() == Some(ch)
    }

    fn at_newline(&self) -> bool {
        matches!(self.peek(), Some('\n' | '\r'))
    }

    fn starts_with(&self, text: &str) -> bool {
        self.src.get(self.i..).is_some_and(|rest| rest.starts_with(text))
    }

    fn last_text_is(&self, text: &str) -> bool {
        self.tokens.last().is_some_and(|token| {
            token.kind == Kind::Ident && self.token_text(token) == text
        })
    }

    fn token_text(&self, token: &Token) -> &str {
        self.src
            .get(token.start as usize..token.end as usize)
            .unwrap_or("")
    }

    fn number_start(&self) -> bool {
        match self.peek() {
            Some(ch) if ch.is_ascii_digit() => true,
            Some('.') => self.peek_next().is_some_and(|ch| ch.is_ascii_digit()),
            _ => false,
        }
    }

    fn continued(&self) -> bool {
        self.starts_with("\\\n") || self.starts_with("\\\r\n") || self.starts_with("\\\r")
    }

    fn comment(&mut self) {
        let start = self.pos();
        self.bump();
        while let Some(ch) = self.peek() {
            if ch == '\n' || ch == '\r' {
                break;
            }
            self.bump();
        }
        self.comments.push((start, self.pos()));
    }

    fn skip_spaces(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t' | '\u{c}' | '\u{b}')) {
            self.bump();
        }
    }

    fn consume_nl(&mut self) {
        if self.at_char('\r') {
            self.bump();
        }
        if self.at_char('\n') {
            self.bump();
        }
    }
}
