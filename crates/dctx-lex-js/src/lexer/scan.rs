// SPDX-License-Identifier: Apache-2.0
//! Code-mode scanner: whitespace, comments, words, numbers, strings, templates, regex.
//! Implements the PARSERS-v1 lexical corpus rules (interpolation, escapes, regex/division).
//! Owns the mode stack shared with `jsx`; used only by `lexer::lex`.
use super::{Kind, Problems, Token};
use crate::domain::{DiagnosticReason, Dialect, LexError};

/// Lexical mode frames; template and JSX frames survive unbalanced code brackets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Frame {
    Brace { block: bool },
    Paren { control: bool },
    Bracket,
    Template,
    JsxExpr,
    JsxTag,
    JsxChildren,
}

pub(crate) struct Lexer<'a> {
    pub src: &'a str,
    pub b: &'a [u8],
    pub pos: usize,
    pub dialect: Dialect,
    pub tokens: Vec<Token>,
    pub comments: Vec<(u32, u32)>,
    pub problems: Problems,
    pub hashbang: bool,
    pub(super) stack: Vec<Frame>,
    pub jsx_seg: usize,
    max_depth: u32,
    nl: bool,
    doc: u32,
    close_allows_regex: bool,
}

/// Multi-character punctuators, longest first; `>` is always single so `>>` closes generics.
const PUNCTS: [&str; 28] = [
    "...", "===", "!==", "**=", "<<=", "&&=", "||=", "??=", "=>", "==", "!=", "<=", "<<", "+=",
    "-=", "*=", "/=", "%=", "&=", "|=", "^=", "&&", "||", "??", "?.", "++", "--", "**",
];
const REGEX_AFTER: [&str; 15] = [
    "return",
    "typeof",
    "instanceof",
    "in",
    "of",
    "new",
    "delete",
    "void",
    "throw",
    "case",
    "do",
    "else",
    "yield",
    "await",
    "extends",
];

impl<'a> Lexer<'a> {
    pub fn new(src: &'a str, dialect: Dialect, max_depth: u32) -> Self {
        Self {
            src,
            b: src.as_bytes(),
            pos: 0,
            dialect,
            tokens: Vec::new(),
            comments: Vec::new(),
            problems: Vec::new(),
            hashbang: false,
            stack: Vec::new(),
            jsx_seg: 0,
            max_depth,
            nl: false,
            doc: 0,
            close_allows_regex: false,
        }
    }

    pub fn run(&mut self) -> Result<(), LexError> {
        if self.src.starts_with("#!") {
            self.hashbang = true;
            self.pos = self.b.iter().position(|&b| b == b'\n').unwrap_or(self.b.len());
        }
        loop {
            match self.stack.last() {
                Some(Frame::JsxTag) => self.jsx_tag()?,
                Some(Frame::JsxChildren) => self.jsx_children()?,
                _ => {
                    if !self.code()? {
                        return Ok(());
                    }
                }
            }
        }
    }

    #[allow(clippy::cast_possible_truncation, reason = "Source offsets, line counts and token counts fit u32 because the public extractor enforces the 8 MiB input ceiling.")]
    #[allow(clippy::naive_bytecount, reason = "Count newline bytes with std iterators; this bounded error path does not justify adding a bytecount dependency.")]
    pub(super) fn push(&mut self, frame: Frame) -> Result<(), LexError> {
        if self.stack.len() >= self.max_depth as usize {
            let line = self.b[..self.pos].iter().filter(|&&b| b == b'\n').count() as u32 + 1;
            return Err(LexError::TooDeep {
                line,
                limit: self.max_depth,
            });
        }
        self.stack.push(frame);
        Ok(())
    }

    #[allow(clippy::cast_possible_truncation, reason = "Source offsets, line counts and token counts fit u32 because the public extractor enforces the 8 MiB input ceiling.")]
    pub(super) fn emit(&mut self, kind: Kind, start: usize, end: usize) {
        self.tokens.push(Token {
            kind,
            start: start as u32,
            end: end as u32,
            nl: std::mem::take(&mut self.nl),
            doc: std::mem::take(&mut self.doc),
        });
    }

    #[allow(clippy::cast_possible_truncation, reason = "Source offsets, line counts and token counts fit u32 because the public extractor enforces the 8 MiB input ceiling.")]
    pub(super) fn problem(&mut self, at: usize, reason: DiagnosticReason) {
        self.problems.push((at as u32, reason));
    }

    pub(super) fn char_at(&self, at: usize) -> Option<char> {
        self.src.get(at..).and_then(|rest| rest.chars().next())
    }

    /// Lexes one code token; returns false at the end of input.
    fn code(&mut self) -> Result<bool, LexError> {
        self.trivia();
        let start = self.pos;
        let Some(c) = self.char_at(start) else {
            return Ok(false);
        };
        let next = self.b.get(start + 1).copied().unwrap_or(0);
        match c {
            '\'' | '"' => self.string(start),
            '`' => self.template(start, true)?,
            '0'..='9' => self.number(start),
            '.' if next.is_ascii_digit() => self.number(start),
            '#' if is_word_start(self.char_at(start + 1)) => {
                self.pos = self.word_end(start + 1);
                self.emit(Kind::Private, start, self.pos);
            }
            '/' if self.regex_allowed() && self.regex(start) => {}
            '<' if self.dialect.jsx() && self.regex_allowed() && self.jsx_start(start) => {
                self.open_jsx(start)?;
            }
            '{' => {
                let block = !self.expression_brace();
                self.push(Frame::Brace { block })?;
                self.single(start);
            }
            '(' => {
                let control = self.control_paren_open();
                self.push(Frame::Paren { control })?;
                self.single(start);
            }
            '[' => {
                self.push(Frame::Bracket)?;
                self.single(start);
            }
            '}' => self.close_brace(start)?,
            ')' | ']' => {
                let frame = self.stack.last().copied();
                let allows = match frame {
                    Some(Frame::Paren { control }) if c == ')' => control,
                    _ => false,
                };
                if matches!(
                    (c, frame),
                    (')', Some(Frame::Paren { .. })) | (']', Some(Frame::Bracket))
                ) {
                    self.stack.pop();
                }
                self.single(start);
                self.close_allows_regex = allows;
            }
            c if is_word_start(Some(c)) => {
                self.pos = self.word_end(start).max(start + c.len_utf8());
                self.emit(Kind::Ident, start, self.pos);
            }
            _ => {
                let rest = &self.src[start..];
                let len = PUNCTS
                    .iter()
                    .find(|p| rest.starts_with(**p) && !(**p == "?." && next_is_digit(rest)))
                    .map_or(c.len_utf8(), |p| p.len());
                self.pos = start + len;
                self.emit(Kind::Punct, start, self.pos);
            }
        }
        Ok(true)
    }

    fn single(&mut self, start: usize) {
        self.pos = start + 1;
        self.emit(Kind::Punct, start, self.pos);
    }

    fn close_brace(&mut self, start: usize) -> Result<(), LexError> {
        let Some(depth) = self
            .stack
            .iter()
            .rposition(|f| matches!(f, Frame::Brace { .. } | Frame::Template | Frame::JsxExpr))
        else {
            self.single(start);
            self.close_allows_regex = true;
            return Ok(());
        };
        let frame = self.stack[depth];
        match frame {
            Frame::Brace { block } => {
                self.stack.truncate(depth);
                self.single(start);
                self.close_allows_regex = block;
            }
            Frame::Template => {
                self.stack.truncate(depth);
                self.template(start, false)?;
            }
            Frame::JsxExpr => {
                self.stack.truncate(depth);
                self.pos = start + 1;
                self.emit(Kind::JsxClose, start, self.pos);
                self.jsx_seg = self.pos;
            }
            _ => self.single(start),
        }
        Ok(())
    }

    /// Skips whitespace and comments, recording line breaks and adjacent comments.
    fn trivia(&mut self) {
        while let Some(c) = self.char_at(self.pos) {
            let next = self.b.get(self.pos + 1).copied();
            if c == '/' && next == Some(b'/') {
                let end = self.b[self.pos..]
                    .iter()
                    .position(|&b| b == b'\n' || b == b'\r')
                    .map_or(self.b.len(), |n| self.pos + n);
                self.comment(self.pos, end);
            } else if c == '/' && next == Some(b'*') {
                let end = if let Some(n) = self.src[self.pos + 2..].find("*/") { self.pos + 2 + n + 2 } else {
                    self.problem(self.pos, DiagnosticReason::UnterminatedComment);
                    self.b.len()
                };
                if self.src[self.pos..end].contains(['\n', '\r', '\u{2028}', '\u{2029}']) {
                    self.nl = true;
                }
                self.comment(self.pos, end);
            } else if matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}') {
                self.nl = true;
                self.pos += c.len_utf8();
            } else if c.is_whitespace() || c == '\u{feff}' {
                self.pos += c.len_utf8();
            } else {
                return;
            }
        }
    }

    #[allow(clippy::cast_possible_truncation, reason = "Source offsets, line counts and token counts fit u32 because the public extractor enforces the 8 MiB input ceiling.")]
    fn comment(&mut self, start: usize, end: usize) {
        self.comments.push((start as u32, end as u32));
        self.doc = self.comments.len() as u32;
        self.pos = end;
    }

    pub(super) fn word_end(&self, mut at: usize) -> usize {
        while let Some(c) = self.char_at(at) {
            if c == '\\' && self.b.get(at + 1) == Some(&b'u') {
                at += 2;
                if self.b.get(at) == Some(&b'{') {
                    at = self.src[at..].find('}').map_or(self.b.len(), |n| at + n + 1);
                } else {
                    at = (at + 4).min(self.b.len());
                    while !self.src.is_char_boundary(at) {
                        at += 1;
                    }
                }
            } else if is_word_part(c) {
                at += c.len_utf8();
            } else {
                break;
            }
        }
        at
    }

    pub(super) fn last_text(&self) -> Option<(Kind, &'a str)> {
        let last = self.tokens.last()?;
        Some((last.kind, &self.src[last.start as usize..last.end as usize]))
    }

    fn last_is_any(&self, words: &[&str]) -> bool {
        matches!(self.last_text(), Some((Kind::Ident, w)) if words.contains(&w))
    }

    /// `if`/`for`/`while`/`with` and `for await` heads, whose `)` may precede a regex.
    fn control_paren_open(&self) -> bool {
        if self.last_is_any(&["if", "for", "while", "with"]) {
            return true;
        }
        let n = self.tokens.len();
        if n < 2 || !self.last_is_any(&["await"]) {
            return false;
        }
        let token = &self.tokens[n - 2];
        token.kind == Kind::Ident && &self.src[token.start as usize..token.end as usize] == "for"
    }

    /// Whether an expression may start here (regex, JSX, object literal).
    pub(super) fn regex_allowed(&self) -> bool {
        match self.last_text() {
            Some((Kind::Ident, word)) => REGEX_AFTER.contains(&word),
            Some((Kind::Punct, ")" | "]" | "}")) => self.close_allows_regex,
            Some((Kind::Punct, "++" | "--")) => false,
            None | Some((Kind::TemplateHead | Kind::TemplateMiddle | Kind::JsxOpen | Kind::Punct, _)) => true,
            Some(_) => false,
        }
    }

    fn expression_brace(&self) -> bool {
        match self.last_text() {
            Some((Kind::Punct, p)) => !matches!(p, ";" | "{" | "}" | ")" | "=>"),
            Some((Kind::Ident, word)) => {
                REGEX_AFTER.contains(&word) && !matches!(word, "do" | "else")
            }
            Some((Kind::TemplateHead | Kind::TemplateMiddle | Kind::JsxOpen, _)) => true,
            None | Some(_) => false,
        }
    }
}

fn next_is_digit(rest: &str) -> bool {
    rest.as_bytes().get(2).is_some_and(u8::is_ascii_digit)
}

pub(super) fn is_word_start(c: Option<char>) -> bool {
    match c {
        Some(c) if c.is_ascii() => c.is_ascii_alphabetic() || c == '$' || c == '_' || c == '\\',
        Some(c) => is_word_part(c),
        None => false,
    }
}

fn is_word_part(c: char) -> bool {
    if c.is_ascii() {
        c.is_ascii_alphanumeric() || c == '$' || c == '_'
    } else {
        !(c.is_whitespace() || matches!(c, '\u{feff}' | '\u{2028}' | '\u{2029}'))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lexer(source: &str) -> Lexer<'_> {
        let mut lexer = Lexer::new(source, Dialect::JavaScript, 64);
        lexer.run().unwrap();
        lexer
    }

    #[test]
    fn comments_mark_adjacency_and_line_breaks() {
        let lexed = lexer("/** a */\nf /* b */ g\n// c\n\nh");
        assert_eq!(lexed.tokens[0].doc, 1);
        assert!(lexed.tokens[0].nl);
        assert_eq!(lexed.tokens[1].doc, 2);
        assert_eq!(lexed.tokens[2].doc, 3);
        assert_eq!(lexed.comments.len(), 3);
    }

    #[test]
    fn unterminated_literals_are_reported_and_progress_continues() {
        let lexed = lexer("a = 'open\nb = `t ${ c");
        assert_eq!(lexed.tokens[2].kind, Kind::BadStr);
        assert!(
            lexed
                .problems
                .iter()
                .any(|p| p.1 == DiagnosticReason::UnterminatedString)
        );
        assert!(lexed.tokens.iter().any(|t| t.kind == Kind::TemplateHead));
        let lexed = lexer("x = 1 / 2 /* never closed");
        assert_eq!(lexed.problems[0].1, DiagnosticReason::UnterminatedComment);
    }

    #[test]
    fn numbers_escapes_and_private_names_are_whole_tokens() {
        let lexed = lexer("0x1F 1e-3 .5 10n this.#x \\u0061b");
        let texts: Vec<_> = lexed
            .tokens
            .iter()
            .map(|t| &lexed.src[t.start as usize..t.end as usize])
            .collect();
        assert_eq!(texts, ["0x1F", "1e-3", ".5", "10n", "this", ".", "#x", "\\u0061b"]);
    }

    #[test]
    fn numeric_separators_stay_inside_the_number() {
        let lexed = lexer("1_000n 0xFF_FF 0b10_10");
        let texts: Vec<_> = lexed
            .tokens
            .iter()
            .map(|t| &lexed.src[t.start as usize..t.end as usize])
            .collect();
        assert_eq!(texts, ["1_000n", "0xFF_FF", "0b10_10"]);
    }

    #[test]
    fn regex_after_control_parens_and_division_after_values() {
        let re = lexer("for await (const x of y) /[)]/g.test(x)");
        let texts: Vec<_> = re
            .tokens
            .iter()
            .map(|t| &re.src[t.start as usize..t.end as usize])
            .collect();
        assert!(texts.contains(&"/[)]/g"), "{texts:?}");
        let div = lexer("const o = { a: 1 } / 2; if (x) {} /a/;");
        let texts: Vec<_> = div
            .tokens
            .iter()
            .map(|t| &div.src[t.start as usize..t.end as usize])
            .collect();
        assert!(texts.contains(&"/"), "{texts:?}");
        assert!(texts.contains(&"/a/"), "{texts:?}");
        assert!(!texts.iter().any(|t| t.starts_with("/ ")), "{texts:?}");
    }
}
