// SPDX-License-Identifier: Apache-2.0
//! Code, strings, `%`-literals, regex and symbols for the Ruby lexer.
//! Implements expression-position rules so `/` and `%` are not misread as operators.
//! Heredoc bodies and bracket depth live in the sibling modules; this is the driver.
mod literals;
mod names;

use super::{as_u32, Lx};
use crate::domain::{DiagnosticReason, LexError};

const LONG_OPS: &[&str] = &[
    "...", "===", "<=>", "**=", "&&=", "||=", "==", "=~", "!=", "!~", "<=", ">=", "**", "<<", ">>",
    "&&", "||", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "=>", "->", "..", "::", "&.",
];

impl Lx<'_> {
    pub(super) fn run(&mut self) -> Result<(), LexError> {
        let mut fuel = self.b.len().saturating_mul(8).max(8);
        while self.i < self.b.len() {
            if fuel == 0 {
                self.problem(self.i, DiagnosticReason::Malformed);
                break;
            }
            fuel -= 1;
            let before = self.i;
            self.step()?;
            if self.i == before {
                self.advance_char();
            }
        }
        self.finish_pending();
        Ok(())
    }

    pub(super) fn step(&mut self) -> Result<(), LexError> {
        if self.i >= self.b.len() {
            return Ok(());
        }
        let byte = self.b[self.i];
        if matches!(byte, b' ' | b'\t' | b'\r' | 0x0b | 0x0c) {
            self.i += 1;
            return Ok(());
        }
        if byte == b'\n' {
            return self.on_newline();
        }
        if self.at_bol() && self.try_begin() {
            return Ok(());
        }
        if self.at_bol() && self.try_data() {
            return Ok(());
        }
        if byte == b'#' {
            self.line_comment();
            return Ok(());
        }
        if byte == b'\\' && self.continue_line() {
            return Ok(());
        }
        if matches!(byte, b'\'' | b'"' | b'`') {
            return self.quoted(byte);
        }
        if byte == b'%' {
            return self.percent();
        }
        if byte == b'/' {
            return self.slash();
        }
        if byte == b':' {
            return self.colon();
        }
        if byte == b'<' && self.try_heredoc() {
            return Ok(());
        }
        if byte.is_ascii_digit() {
            self.number();
            return Ok(());
        }
        if byte == b'@' || byte == b'$' || is_name_start_byte(byte) || byte >= 0x80 {
            self.ident();
            return Ok(());
        }
        if byte == b'?' {
            if self.try_char_literal() {
                return Ok(());
            }
            return self.operator();
        }
        self.operator()
    }

    fn try_begin(&mut self) -> bool {
        if !word_at(self.src, self.i, "=begin") {
            return false;
        }
        let start = self.i;
        self.i += "=begin".len();
        while self.i < self.b.len() {
            if self.at_bol() && word_at(self.src, self.i, "=end") {
                self.i += "=end".len();
                while self.i < self.b.len() && self.b[self.i] != b'\n' {
                    self.i += 1;
                }
                if self.i < self.b.len() && self.b[self.i] == b'\n' {
                    self.i += 1;
                }
                self.comments.push((as_u32(start), as_u32(self.i)));
                return true;
            }
            self.i += 1;
        }
        self.problem(start, DiagnosticReason::UnterminatedComment);
        self.comments.push((as_u32(start), as_u32(self.i)));
        true
    }

    fn try_data(&mut self) -> bool {
        if !word_at(self.src, self.i, "__END__") {
            return false;
        }
        self.i = self.b.len();
        true
    }

    fn line_comment(&mut self) {
        let start = self.i;
        while self.i < self.b.len() && self.b[self.i] != b'\n' {
            self.i += 1;
        }
        self.comments.push((as_u32(start), as_u32(self.i)));
    }

    fn continue_line(&mut self) -> bool {
        let mut j = self.i + 1;
        if self.byte_at(j) == Some(b'\r') {
            j += 1;
        }
        if self.byte_at(j) == Some(b'\n') {
            self.i = j + 1;
            true
        } else {
            false
        }
    }
}

fn is_name_start_byte(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

pub(super) fn pair_open(close: u8) -> u8 {
    match close {
        b')' => b'(',
        b']' => b'[',
        b'}' => b'{',
        b'>' => b'<',
        _ => 0,
    }
}

fn word_at(src: &str, index: usize, word: &str) -> bool {
    let Some(rest) = src.get(index..) else {
        return false;
    };
    let Some(after) = rest.strip_prefix(word) else {
        return false;
    };
    match after.chars().next() {
        None => true,
        Some(ch) => ch.is_whitespace() || !(ch.is_ascii_alphanumeric() || ch == '_'),
    }
}
