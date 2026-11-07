// SPDX-License-Identifier: Apache-2.0
//! JSX element modes: opaque tag and text segments, code inside `{}` containers.
//! Implements the PARSERS-v1 JSX/TSX rule: keep declarations and calls in containers,
//! never lex JSX text or attribute strings as code. Used by `scan::Lexer::run`.
use super::Kind;
use super::scan::{Frame, Lexer, is_word_start};
use crate::domain::{DiagnosticReason, LexError};

impl Lexer<'_> {
    /// Whether `<` at an expression start opens JSX rather than a generic arrow.
    /// `<T,` and `<T extends` are type parameters in TSX and in JSX-free code.
    pub(super) fn jsx_start(&self, start: usize) -> bool {
        match self.char_at(start + 1) {
            Some('>') => true,
            c if is_word_start(c) => {
                let end = self.word_end(start + 1);
                let rest = self.src[end..].trim_start();
                !(rest.starts_with(',') || rest.starts_with("extends "))
            }
            _ => false,
        }
    }

    pub(super) fn open_jsx(&mut self, start: usize) -> Result<(), LexError> {
        self.jsx_seg = start;
        self.element(start)
    }

    /// Enters an element whose `<` is at `start`.
    fn element(&mut self, start: usize) -> Result<(), LexError> {
        if self.b.get(start + 1) == Some(&b'>') {
            self.pos = start + 2;
            return self.push(Frame::JsxChildren);
        }
        self.pos = self.name_end(start + 1);
        self.push(Frame::JsxTag)
    }

    fn name_end(&self, mut at: usize) -> usize {
        loop {
            let end = self.word_end(at);
            match self.b.get(end) {
                Some(b'.' | b':' | b'-') => at = end + 1,
                _ => return end,
            }
        }
    }

    /// Scans attributes until a container, `/>` or `>`.
    pub(super) fn jsx_tag(&mut self) -> Result<(), LexError> {
        while let Some(c) = self.char_at(self.pos) {
            let next = self.b.get(self.pos + 1).copied();
            match c {
                '{' => return self.container(),
                '/' if next == Some(b'>') => {
                    self.pos += 2;
                    self.close_element();
                    return Ok(());
                }
                '>' => {
                    self.pos += 1;
                    self.stack.pop();
                    return self.push(Frame::JsxChildren);
                }
                '"' | '\'' => {
                    let quote = c;
                    match self.src[self.pos + 1..].find(quote) {
                        Some(n) => self.pos += n + 2,
                        None => self.pos = self.b.len(),
                    }
                }
                c => self.pos += c.len_utf8(),
            }
        }
        self.unterminated();
        Ok(())
    }

    /// Scans text children until a container, a child element or the closing tag.
    pub(super) fn jsx_children(&mut self) -> Result<(), LexError> {
        while let Some(c) = self.char_at(self.pos) {
            let next = self.b.get(self.pos + 1).copied();
            match c {
                '{' => return self.container(),
                '<' if next == Some(b'/') => {
                    if let Some(n) = self.src[self.pos..].find('>') {
                        self.pos += n + 1
                    } else {
                        self.unterminated();
                        return Ok(());
                    }
                    self.close_element();
                    return Ok(());
                }
                '<' => return self.element(self.pos),
                c => self.pos += c.len_utf8(),
            }
        }
        self.unterminated();
        Ok(())
    }

    fn container(&mut self) -> Result<(), LexError> {
        self.segment(self.pos);
        let start = self.pos;
        self.pos += 1;
        self.emit(Kind::JsxOpen, start, self.pos);
        self.push(Frame::JsxExpr)
    }

    fn segment(&mut self, end: usize) {
        if end > self.jsx_seg {
            self.emit(Kind::Jsx, self.jsx_seg, end);
        }
        self.jsx_seg = end;
    }

    fn close_element(&mut self) {
        self.stack.pop();
        if !matches!(self.stack.last(), Some(Frame::JsxChildren)) {
            self.segment(self.pos);
        }
    }

    fn unterminated(&mut self) {
        self.problem(self.jsx_seg, DiagnosticReason::UnterminatedJsx);
        self.pos = self.b.len();
        self.segment(self.pos);
        while matches!(
            self.stack.last(),
            Some(Frame::JsxTag | Frame::JsxChildren | Frame::JsxExpr)
        ) {
            self.stack.pop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Dialect;

    fn texts(source: &str, dialect: Dialect) -> Vec<(Kind, String)> {
        let mut lexer = Lexer::new(source, dialect, 32);
        lexer.run().unwrap();
        lexer
            .tokens
            .iter()
            .map(|t| (t.kind, source[t.start as usize..t.end as usize].to_owned()))
            .collect()
    }

    #[test]
    fn nested_elements_fragments_and_containers_alternate() {
        let tokens = texts("v = <><A.B x='{'>{f(<i/>)}</A.B></>;", Dialect::JavaScript);
        let kinds: Vec<Kind> = tokens.iter().map(|t| t.0).collect();
        assert_eq!(
            kinds,
            [
                Kind::Ident,
                Kind::Punct,
                Kind::Jsx,
                Kind::JsxOpen,
                Kind::Ident,
                Kind::Punct,
                Kind::Jsx,
                Kind::Punct,
                Kind::JsxClose,
                Kind::Jsx,
                Kind::Punct
            ]
        );
        assert_eq!(tokens[2].1, "<><A.B x='{'>");
        assert_eq!(tokens[9].1, "</A.B></>");
    }

    #[test]
    fn tsx_generic_arrows_are_code_and_unclosed_jsx_is_reported() {
        let tokens = texts("f = <T,>(x: T) => x", Dialect::Tsx);
        assert_eq!(tokens[2], (Kind::Punct, "<".into()));
        let js = texts("f = <T,>(x) => x", Dialect::JavaScript);
        assert_eq!(js[2], (Kind::Punct, "<".into()));
        let extended = texts("f = <T extends U>(x) => x", Dialect::JavaScript);
        assert_eq!(extended[2], (Kind::Punct, "<".into()));
        let element = texts("v = <div>(note)</div>", Dialect::JavaScript);
        assert_eq!(element[2].0, Kind::Jsx);
        let mut lexer = Lexer::new("v = <div>{a}", Dialect::JavaScript, 32);
        lexer.run().unwrap();
        assert_eq!(lexer.problems[0].1, DiagnosticReason::UnterminatedJsx);
        assert_eq!(lexer.stack, []);
    }
}
