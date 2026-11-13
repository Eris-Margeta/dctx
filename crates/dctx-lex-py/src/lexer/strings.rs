// SPDX-License-Identifier: Apache-2.0
//! String and f-string scanners, including prefixes and format specifications.
//! Implements PARSERS-v1 lexical rules for quotes, escapes and interpolations.
//! Called from the code scanner; an unterminated string is reported and kept.
use super::{Emitted, Kind, Lx, Prefix};
use crate::domain::{DiagnosticReason, LexError};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Step {
    Pending,
    Cont,
    Done,
}

impl Lx<'_> {
    pub(super) fn take_string(
        &mut self,
        prefix_len: usize,
        prefix: Prefix,
    ) -> Result<Emitted, LexError> {
        let start = self.pos();
        self.i += prefix_len;
        let Some((quote, triple)) = self.opening_quote() else {
            return Ok(Emitted::Other);
        };
        self.consume_close(quote, triple);
        if prefix.formatted {
            self.fstring(quote, triple, prefix.raw)?;
            return Ok(Emitted::Other);
        }
        if self.plain_string(quote, triple, prefix.raw) {
            self.emit(Kind::Str, start, self.pos(), prefix.doc);
        } else {
            self.problem(self.pos(), DiagnosticReason::UnterminatedString);
            self.emit(Kind::Str, start, self.pos(), false);
        }
        Ok(Emitted::Other)
    }

    fn plain_string(&mut self, quote: char, triple: bool, raw: bool) -> bool {
        loop {
            if self.i >= self.src.len() {
                return false;
            }
            if self.at_char('\\') {
                self.bump();
                if self.at_newline() {
                    if raw && !triple {
                        return false;
                    }
                    self.consume_nl();
                    continue;
                }
                self.bump();
                continue;
            }
            if !triple && self.at_newline() {
                return false;
            }
            if self.closes_string(quote, triple) {
                self.consume_close(quote, triple);
                return true;
            }
            if self.at_newline() {
                self.consume_nl();
            } else {
                self.i += plain_chunk(self.src, self.i, &['\\', quote, '\r', '\n']);
            }
        }
    }

    fn fstring(&mut self, quote: char, triple: bool, raw: bool) -> Result<(), LexError> {
        if self.fstrings >= self.max_depth {
            return Err(LexError::TooDeep {
                line: self.line_now(),
                limit: self.max_depth,
            });
        }
        self.fstrings += 1;
        let result = self.fstring_body(quote, triple, raw);
        self.fstrings -= 1;
        result
    }

    fn fstring_body(&mut self, quote: char, triple: bool, raw: bool) -> Result<(), LexError> {
        self.string_closed = false;
        loop {
            let before = self.i;
            if self.fstring_finished(quote, triple) {
                return Ok(());
            }
            match self.fstring_brace(quote, triple)? {
                Step::Done => return Ok(()),
                Step::Cont => continue,
                Step::Pending => {}
            }
            match self.fstring_escape(raw, triple) {
                Step::Done => return Ok(()),
                Step::Cont => continue,
                Step::Pending => {}
            }
            if self.at_newline() {
                self.consume_nl();
            } else {
                self.bump();
            }
            if self.i == before {
                return Ok(());
            }
        }
    }

    fn fstring_finished(&mut self, quote: char, triple: bool) -> bool {
        if self.string_closed {
            self.string_closed = false;
            if self.closes_string(quote, triple) {
                self.consume_close(quote, triple);
            }
            return true;
        }
        if self.i >= self.src.len() {
            self.problem(self.pos(), DiagnosticReason::UnterminatedString);
            return true;
        }
        if self.closes_string(quote, triple) {
            self.consume_close(quote, triple);
            return true;
        }
        if !triple && self.at_newline() {
            self.problem(self.pos(), DiagnosticReason::UnterminatedString);
            return true;
        }
        false
    }

    fn fstring_brace(&mut self, quote: char, triple: bool) -> Result<Step, LexError> {
        if self.at_char('{') {
            if self.peek_next() == Some('{') {
                self.bump();
                self.bump();
                return Ok(Step::Cont);
            }
            self.bump();
            self.fexpr(quote, triple)?;
            return Ok(Step::Cont);
        }
        if self.at_char('}') {
            if self.peek_next() == Some('}') {
                self.bump();
                self.bump();
                return Ok(Step::Cont);
            }
            self.problem(self.pos(), DiagnosticReason::UnterminatedString);
            self.bump();
            return Ok(Step::Cont);
        }
        Ok(Step::Pending)
    }

    fn fstring_escape(&mut self, raw: bool, triple: bool) -> Step {
        if !self.at_char('\\') {
            return Step::Pending;
        }
        self.bump();
        if self.at_newline() {
            if raw && !triple {
                self.problem(self.pos(), DiagnosticReason::UnterminatedString);
                return Step::Done;
            }
            self.consume_nl();
        } else {
            self.bump();
        }
        Step::Cont
    }

    fn fexpr(&mut self, quote: char, triple: bool) -> Result<(), LexError> {
        let mut paren = 0i32;
        let mut bracket = 0i32;
        let mut brace = 0i32;
        let mut lambdas = Vec::new();
        loop {
            // PEP 701: lex a same-quote string before the f-string closer.
            if self.lex_fexpr_string()? {
                continue;
            }
            if self.fexpr_closed(quote, triple) {
                return Ok(());
            }
            match self.fexpr_delim(quote, triple, paren, bracket, brace, &mut lambdas)? {
                Step::Done => return Ok(()),
                Step::Cont => continue,
                Step::Pending => {}
            }
            self.fexpr_token(&mut paren, &mut bracket, &mut brace, &mut lambdas)?;
        }
    }

    fn fexpr_closed(&mut self, quote: char, triple: bool) -> bool {
        if self.string_closed {
            return true;
        }
        self.skip_f_ws();
        if self.i >= self.src.len() {
            self.problem(self.pos(), DiagnosticReason::UnterminatedString);
            return true;
        }
        if self.closes_string(quote, triple) {
            self.problem(self.pos(), DiagnosticReason::UnterminatedString);
            self.string_closed = true;
            return true;
        }
        false
    }

    fn fexpr_delim(
        &mut self,
        quote: char,
        triple: bool,
        paren: i32,
        bracket: i32,
        brace: i32,
        lambdas: &mut Vec<(i32, i32, i32)>,
    ) -> Result<Step, LexError> {
        if self.starts_with(":=") {
            self.scan_one_token()?;
            return Ok(Step::Cont);
        }
        if self.at_char(':') && self.lambda_colon(lambdas, paren, bracket, brace) {
            return Ok(Step::Cont);
        }
        if !f_depth_zero(paren, bracket, brace) {
            return Ok(Step::Pending);
        }
        if self.at_char(':') {
            self.fformat(quote, triple)?;
            return Ok(Step::Done);
        }
        if self.at_char('}') {
            self.bump();
            return Ok(Step::Done);
        }
        if self.try_conversion() || self.try_debug_eq() {
            return Ok(Step::Cont);
        }
        Ok(Step::Pending)
    }

    fn fexpr_token(
        &mut self,
        paren: &mut i32,
        bracket: &mut i32,
        brace: &mut i32,
        lambdas: &mut Vec<(i32, i32, i32)>,
    ) -> Result<(), LexError> {
        let before = self.i;
        match self.scan_one_token()? {
            Emitted::Open(b'(') => *paren += 1,
            Emitted::Open(b'[') => *bracket += 1,
            Emitted::Open(b'{') => *brace += 1,
            Emitted::Close(b')') => *paren = paren.saturating_sub(1),
            Emitted::Close(b']') => *bracket = bracket.saturating_sub(1),
            Emitted::Close(b'}') => *brace = brace.saturating_sub(1),
            Emitted::Name if self.last_text_is("lambda") => {
                lambdas.push((*paren, *bracket, *brace));
            }
            _ => {}
        }
        if self.i == before {
            self.bump();
        }
        Ok(())
    }

    fn lambda_colon(
        &mut self,
        lambdas: &mut Vec<(i32, i32, i32)>,
        paren: i32,
        bracket: i32,
        brace: i32,
    ) -> bool {
        if lambdas.last().copied() != Some((paren, bracket, brace)) {
            return false;
        }
        let at = self.pos();
        self.bump();
        self.emit(Kind::Punct, at, self.pos(), false);
        lambdas.pop();
        true
    }

    fn fformat(&mut self, quote: char, triple: bool) -> Result<(), LexError> {
        self.bump();
        loop {
            if self.fformat_end(quote, triple) {
                return Ok(());
            }
            match self.fformat_brace(quote, triple)? {
                Step::Done => return Ok(()),
                Step::Cont => continue,
                Step::Pending => {}
            }
            let before = self.i;
            if self.at_newline() {
                self.consume_nl();
            } else {
                self.bump();
            }
            if self.i == before {
                return Ok(());
            }
        }
    }

    fn fformat_end(&mut self, quote: char, triple: bool) -> bool {
        if self.i >= self.src.len() {
            self.problem(self.pos(), DiagnosticReason::UnterminatedString);
            return true;
        }
        if self.closes_string(quote, triple) {
            self.problem(self.pos(), DiagnosticReason::UnterminatedString);
            self.string_closed = true;
            return true;
        }
        if !triple && self.at_newline() {
            self.problem(self.pos(), DiagnosticReason::UnterminatedString);
            return true;
        }
        false
    }

    fn fformat_brace(&mut self, quote: char, triple: bool) -> Result<Step, LexError> {
        if self.at_char('{') {
            if self.peek_next() == Some('{') {
                self.bump();
                self.bump();
                return Ok(Step::Cont);
            }
            self.bump();
            self.fexpr(quote, triple)?;
            return Ok(if self.string_closed {
                Step::Done
            } else {
                Step::Cont
            });
        }
        if self.at_char('}') {
            if self.peek_next() == Some('}') {
                self.bump();
                self.bump();
                return Ok(Step::Cont);
            }
            self.bump();
            return Ok(Step::Done);
        }
        Ok(Step::Pending)
    }

    fn try_conversion(&mut self) -> bool {
        if !self.at_char('!') || self.starts_with("!=") {
            return false;
        }
        let Some(flag) = self.src.as_bytes().get(self.i + 1).copied() else {
            return false;
        };
        if !matches!(flag, b's' | b'r' | b'a') {
            return false;
        }
        let after = self.src.get(self.i + 2..).unwrap_or("");
        let after = after.trim_start_matches([' ', '\t']);
        if after.starts_with([':', '}', '=']) {
            self.i += 2;
            true
        } else {
            false
        }
    }

    fn try_debug_eq(&mut self) -> bool {
        if !self.at_char('=') || self.starts_with("==") {
            return false;
        }
        let after = self.src.get(self.i + 1..).unwrap_or("");
        let after = after.trim_start_matches([' ', '\t']);
        if after.starts_with(['!', ':', '}']) {
            self.bump();
            true
        } else {
            false
        }
    }

    pub(super) fn skip_f_ws(&mut self) {
        loop {
            let before = self.i;
            self.skip_spaces();
            if self.continued() {
                self.bump();
                self.consume_nl();
                continue;
            }
            if self.at_newline() {
                self.consume_nl();
                continue;
            }
            if self.at_char('#') {
                self.comment();
                continue;
            }
            if self.i == before {
                break;
            }
        }
    }

    pub(super) fn look_prefix(&self) -> Option<(usize, Prefix)> {
        let rest = self.src.get(self.i..)?;
        if rest.starts_with(['\'', '"']) {
            return Some((
                0,
                Prefix {
                    raw: false,
                    formatted: false,
                    doc: true,
                },
            ));
        }
        for len in [2usize, 1] {
            let pre = rest.get(..len)?;
            if !pre.bytes().all(|b| b.is_ascii_alphabetic()) {
                continue;
            }
            let after = rest.get(len..)?;
            if after.starts_with(['\'', '"']) && valid_prefix(pre) {
                return Some((len, prefix_flags(pre)));
            }
        }
        None
    }

    fn opening_quote(&self) -> Option<(char, bool)> {
        let rest = self.src.get(self.i..)?;
        let quote = rest.chars().next()?;
        if quote != '"' && quote != '\'' {
            return None;
        }
        let triple = {
            let bytes = rest.as_bytes();
            let q = quote as u8;
            bytes.len() >= 3 && bytes[0] == q && bytes[1] == q && bytes[2] == q
        };
        Some((quote, triple))
    }

    fn closes_string(&self, quote: char, triple: bool) -> bool {
        let Some(rest) = self.src.get(self.i..) else {
            return false;
        };
        let bytes = rest.as_bytes();
        let q = quote as u8;
        if triple {
            bytes.len() >= 3 && bytes[0] == q && bytes[1] == q && bytes[2] == q
        } else {
            bytes.first() == Some(&q)
        }
    }

    fn consume_close(&mut self, quote: char, triple: bool) {
        let n = if triple { 3 } else { 1 };
        for _ in 0..n {
            if self.at_char(quote) {
                self.bump();
            }
        }
    }
}

fn f_depth_zero(paren: i32, bracket: i32, brace: i32) -> bool {
    paren == 0 && bracket == 0 && brace == 0
}

fn valid_prefix(prefix: &str) -> bool {
    matches!(
        prefix.to_ascii_lowercase().as_str(),
        "r" | "u" | "f" | "b" | "t" | "fr" | "rf" | "br" | "rb" | "tr" | "rt"
    )
}

fn prefix_flags(prefix: &str) -> Prefix {
    let lower = prefix.to_ascii_lowercase();
    let formatted = lower.contains('f') || lower.contains('t');
    let bytes = lower.contains('b');
    Prefix {
        raw: lower.contains('r'),
        formatted,
        doc: !formatted && !bytes && matches!(lower.as_str(), "r" | "u"),
    }
}

// Bound each search window so repeated escapes cannot cause quadratic rescans.
// ASCII delimiters never cut a UTF-8 code point.
fn plain_chunk(source: &str, start: usize, specials: &[char]) -> usize {
    let bytes = &source.as_bytes()[start..];
    if let Some(end) = bytes[..bytes.len().min(32)]
        .iter()
        .position(|&byte| specials.contains(&char::from(byte)))
    {
        return end.max(1);
    }
    let mut end = (start + 4096).min(source.len());
    while !source.is_char_boundary(end) {
        end -= 1;
    }
    let mut rest = &source[start..end];
    for &special in specials {
        let end = rest.find(special).unwrap_or(rest.len());
        rest = &rest[..end];
    }
    rest.len().max(1)
}
