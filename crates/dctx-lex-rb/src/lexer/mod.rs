// SPDX-License-Identifier: Apache-2.0
//! Token stream for Ruby source, including heredocs, `%`-literals, interpolation,
//! character literals and globals.
//! Implements PARSERS-v1 lexing bounded by `Limits`. The structural walker consumes this.
//! Comments, `__END__` data and string bodies never become declaration tokens.
mod heredoc;
mod scan;

use crate::domain::{Diagnostic, DiagnosticReason, LexError, Limits};

/// Token category. Keywords stay [`Kind::Ident`] and are compared by text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// Identifier, keyword, ivar, cvar or global.
    Ident,
    /// Symbol literal (`:foo`, `:"x"`).
    Symbol,
    /// String, heredoc, regex or `%`-literal. `plain` marks a decodable string.
    Str,
    /// Numeric literal.
    Number,
    /// Operator or delimiter.
    Punct,
    /// Statement break. Suppressed inside brackets and explicit continuation.
    Newline,
}

/// One significant token.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Token {
    /// Category.
    pub kind: Kind,
    /// First byte.
    pub start: u32,
    /// One past the last byte.
    pub end: u32,
    /// True when a string has no interpolation and can be decoded literally.
    pub plain: bool,
}

/// Lexed tokens, comment spans and problems.
pub(crate) struct Lexed {
    /// Significant tokens in source order.
    pub tokens: Vec<Token>,
    /// Comment byte spans, in source order. Newlines are excluded for `#`.
    pub comments: Vec<(u32, u32)>,
    /// Lexical problems, in source order.
    pub diagnostics: Vec<Diagnostic>,
    /// Byte offset of the first byte of each line. Index 0 is byte 0.
    pub line_starts: Vec<u32>,
}

impl Lexed {
    /// One-based line of a byte offset. Rows advance on `\n` only.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "At most one line per byte plus the initial line; RubyLexer::extract caps input at 8 MiB, well within u32."
    )]
    pub fn line(&self, byte: u32) -> u32 {
        self.line_starts
            .partition_point(|&start| start <= byte)
            .max(1) as u32
    }
}

struct Closer {
    byte: u8,
    at: u32,
}

struct Lx<'a> {
    src: &'a str,
    b: &'a [u8],
    i: usize,
    tokens: Vec<Token>,
    comments: Vec<(u32, u32)>,
    problems: Vec<(u32, DiagnosticReason)>,
    pending: Vec<heredoc::Pending>,
    closers: Vec<Closer>,
    line_starts: Vec<u32>,
    depth: u32,
    brackets: u32,
    max_depth: u32,
    /// Next token may be a regex or `%`-literal.
    expr: bool,
    /// Next token is a method name (`def`, `.`, `::`).
    fname: bool,
}

/// Lexes `source`. Nesting past `limits.max_depth` is [`LexError::TooDeep`].
pub(crate) fn lex(source: &str, limits: &Limits) -> Result<Lexed, LexError> {
    let mut lx = Lx {
        src: source,
        b: source.as_bytes(),
        i: 0,
        tokens: Vec::new(),
        comments: Vec::new(),
        problems: Vec::new(),
        pending: Vec::new(),
        closers: Vec::new(),
        line_starts: line_starts(source),
        depth: 0,
        brackets: 0,
        max_depth: limits.max_depth,
        expr: true,
        fname: false,
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
    let mut starts = vec![0];
    for (index, byte) in source.bytes().enumerate() {
        if byte == b'\n' {
            starts.push(as_u32(index + 1));
        }
    }
    starts
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "At most one line per byte plus the initial line; RubyLexer::extract caps input at 8 MiB, well within u32."
)]
fn line_of(starts: &[u32], byte: u32) -> u32 {
    starts.partition_point(|&start| start <= byte).max(1) as u32
}

fn as_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

impl Lx<'_> {
    fn emit(&mut self, kind: Kind, start: usize, end: usize, plain: bool) {
        self.tokens.push(Token {
            kind,
            start: as_u32(start),
            end: as_u32(end.max(start)),
            plain,
        });
    }

    fn problem(&mut self, at: usize, reason: DiagnosticReason) {
        self.problems.push((as_u32(at), reason));
    }

    fn push_depth(&mut self) -> Result<(), LexError> {
        self.depth += 1;
        if self.depth > self.max_depth {
            return Err(LexError::TooDeep {
                line: line_of(&self.line_starts, as_u32(self.i)),
                limit: self.max_depth,
            });
        }
        Ok(())
    }

    fn at_bol(&self) -> bool {
        self.i == 0
            || (self.i == '\u{feff}'.len_utf8() && self.src.starts_with('\u{feff}'))
            || self.b.get(self.i.wrapping_sub(1)) == Some(&b'\n')
    }

    fn byte_at(&self, index: usize) -> Option<u8> {
        self.b.get(index).copied()
    }

    fn open_bracket(&mut self, closer: u8) -> Result<(), LexError> {
        self.push_depth()?;
        self.brackets += 1;
        self.closers.push(Closer {
            byte: closer,
            at: as_u32(self.i),
        });
        self.expr = true;
        self.fname = false;
        Ok(())
    }

    fn advance_char(&mut self) {
        if self.i >= self.b.len() {
            return;
        }
        if !self.src.is_char_boundary(self.i) {
            self.i += 1;
            while self.i < self.b.len() && !self.src.is_char_boundary(self.i) {
                self.i += 1;
            }
            return;
        }
        let size = self.src[self.i..].chars().next().map_or(1, char::len_utf8);
        self.i += size.max(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(source: &str) -> Vec<String> {
        let lexed = lex(source, &Limits::default()).expect("lex");
        lexed
            .tokens
            .iter()
            .map(|token| source[token.start as usize..token.end as usize].to_owned())
            .collect()
    }

    #[test]
    fn slash_follows_expression_position() {
        let div = texts("a = 10 / 2\n");
        assert!(div.iter().any(|text| text == "/"), "{div:?}");
        assert!(!div.iter().any(|text| text.starts_with("/2")), "{div:?}");
        let regex = texts("return /cd/\n");
        assert!(
            regex.iter().any(|text| text.starts_with("/cd")),
            "{regex:?}"
        );
    }

    #[test]
    fn regexp_command_arguments_and_division_keep_the_next_declaration() {
        for prefix in [
            "puts /class Ghost; end/",
            "logger.match /class Ghost; end/",
            "return /class Ghost; end/",
            "pattern=/class Ghost; end/",
        ] {
            let source = format!("{prefix}\ndef real\nend\n");
            let lexed = lex(&source, &Limits::default()).expect("lex");
            assert!(
                lexed.diagnostics.is_empty(),
                "{prefix}: {:?}",
                lexed.diagnostics
            );
            assert!(
                lexed.tokens.iter().any(|token| {
                    token.kind == Kind::Str
                        && &source[token.start as usize..token.end as usize] == "/class Ghost; end/"
                }),
                "{prefix}"
            );
            let identifiers: Vec<_> = lexed
                .tokens
                .iter()
                .filter(|token| token.kind == Kind::Ident)
                .map(|token| &source[token.start as usize..token.end as usize])
                .collect();
            assert!(identifiers.contains(&"real"), "{identifiers:?}");
            assert!(!identifiers.contains(&"Ghost"), "{identifiers:?}");
        }

        for expression in [
            "10 / 2",
            "value / divisor",
            "value/2",
            "value /= 2",
            "obj./(2)",
            "@value /2",
            "$value /2",
            "VALUE /2",
            "self /2",
            "nil /2",
            "true /2",
            "false /2",
        ] {
            let source = format!("{expression}\ndef real\nend\n");
            let lexed = lex(&source, &Limits::default()).expect("lex");
            assert!(
                lexed.diagnostics.is_empty(),
                "{expression}: {:?}",
                lexed.diagnostics
            );
            assert!(
                !lexed.tokens.iter().any(|token| token.kind == Kind::Str),
                "{expression}"
            );
            assert!(
                texts(&source).iter().any(|text| text == "real"),
                "{expression}"
            );
        }
    }

    #[test]
    fn operators_globals_and_character_literals_stay_intact() {
        let assign = texts("ratio %= 2\n");
        assert!(assign.iter().any(|text| text == "%="), "{assign:?}");
        assert!(assign.iter().any(|text| text == "2"), "{assign:?}");
        let quoted = texts("x = %q=secret=\n");
        assert!(
            quoted.iter().any(|text| text.contains("secret")),
            "{quoted:?}"
        );
        assert!(!quoted.iter().any(|text| text == "secret"), "{quoted:?}");
        let lambda = texts("f = ->(x) { x }\n");
        assert!(lambda.iter().any(|text| text == "->"), "{lambda:?}");
        let chars = texts("c = ?x\n");
        assert!(chars.iter().any(|text| text == "?x"), "{chars:?}");
        let slash = texts("c = ?/\n");
        assert!(slash.iter().any(|text| text == "?/"), "{slash:?}");
        assert!(!slash.iter().any(|text| text.starts_with('/')), "{slash:?}");
        let escaped = texts("c = ?\\n\n");
        assert!(escaped.iter().any(|text| text == "?\\n"), "{escaped:?}");
        let post = texts("g = $'\n");
        assert!(post.iter().any(|text| text == "$'"), "{post:?}");
        let loaded = texts("g = $\"\n");
        assert!(loaded.iter().any(|text| text == "$\""), "{loaded:?}");
        let pre = texts("g = $`\n");
        assert!(pre.iter().any(|text| text == "$`"), "{pre:?}");
        let flag = texts("a = $-w\n");
        assert!(flag.iter().any(|text| text == "$-w"), "{flag:?}");
        let group = texts("a = $10\n");
        assert!(group.iter().any(|text| text == "$10"), "{group:?}");
        let stack = texts("a = $@\n");
        assert!(stack.iter().any(|text| text == "$@"), "{stack:?}");
        let tern = texts("n > 0 ? 1 : 2\n");
        assert!(tern.iter().any(|text| text == "?"), "{tern:?}");
        assert!(tern.iter().any(|text| text == "1"), "{tern:?}");
        assert!(tern.iter().any(|text| text == ":"), "{tern:?}");
    }
}
