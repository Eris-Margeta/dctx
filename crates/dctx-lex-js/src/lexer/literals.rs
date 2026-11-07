// SPDX-License-Identifier: Apache-2.0
//! Literal scanners: numbers, quoted strings, template segments and regex bodies.
//! Implements the PARSERS-v1 lexical rules for escapes, interpolation and regex classes;
//! unterminated literals are reported and never consume past their line or input.
//! Used by `scan::Lexer::code` and its closing-brace handling.
use super::Kind;
use super::scan::{Frame, Lexer};
use crate::domain::{DiagnosticReason, LexError};

impl Lexer<'_> {
    pub(super) fn number(&mut self, start: usize) {
        let mut at = start;
        while let Some(&b) = self.b.get(at) {
            let sign = (b == b'+' || b == b'-')
                && at > start
                && matches!(self.b[at - 1], b'e' | b'E')
                && !self.src[start..at].starts_with("0x")
                && !self.src[start..at].starts_with("0X");
            let dot = b == b'.' && !self.src[start..at].contains('.');
            if b.is_ascii_alphanumeric() || b == b'_' || sign || dot {
                at += 1;
            } else {
                break;
            }
        }
        self.pos = at;
        self.emit(Kind::Number, start, at);
    }

    pub(super) fn string(&mut self, start: usize) {
        let quote = self.b[start];
        let mut at = start + 1;
        while let Some(c) = self.char_at(at) {
            match c {
                '\\' => {
                    at += 1;
                    at += self.char_at(at).map_or(0, char::len_utf8);
                }
                '\n' | '\r' => break,
                c if c as u32 == u32::from(quote) => {
                    self.pos = at + 1;
                    self.emit(Kind::Str, start, self.pos);
                    return;
                }
                c => at += c.len_utf8(),
            }
        }
        self.problem(start, DiagnosticReason::UnterminatedString);
        self.pos = at;
        self.emit(Kind::BadStr, start, at);
    }

    /// Scans template text from a backtick (`head`) or a closing substitution brace.
    pub(super) fn template(&mut self, start: usize, head: bool) -> Result<(), LexError> {
        let mut at = start + 1;
        while let Some(c) = self.char_at(at) {
            match c {
                '\\' => {
                    at += 1;
                    at += self.char_at(at).map_or(0, char::len_utf8);
                }
                '`' => {
                    self.pos = at + 1;
                    let kind = if head {
                        Kind::Template
                    } else {
                        Kind::TemplateTail
                    };
                    self.emit(kind, start, self.pos);
                    return Ok(());
                }
                '$' if self.b.get(at + 1) == Some(&b'{') => {
                    self.pos = at + 2;
                    let kind = if head {
                        Kind::TemplateHead
                    } else {
                        Kind::TemplateMiddle
                    };
                    self.emit(kind, start, self.pos);
                    return self.push(Frame::Template);
                }
                c => at += c.len_utf8(),
            }
        }
        self.problem(start, DiagnosticReason::UnterminatedTemplate);
        self.pos = at;
        let kind = if head {
            Kind::Template
        } else {
            Kind::TemplateTail
        };
        self.emit(kind, start, at);
        Ok(())
    }

    /// Scans a regex literal; on failure the caller lexes `/` as division.
    pub(super) fn regex(&mut self, start: usize) -> bool {
        let mut at = start + 1;
        let mut class = false;
        while let Some(c) = self.char_at(at) {
            match c {
                '\\' => {
                    at += 1;
                    match self.char_at(at) {
                        Some('\n' | '\r') | None => return false,
                        Some(c) => at += c.len_utf8(),
                    }
                }
                '\n' | '\r' | '\u{2028}' | '\u{2029}' => return false,
                '[' => {
                    class = true;
                    at += 1;
                }
                ']' => {
                    class = false;
                    at += 1;
                }
                '/' if !class => {
                    self.pos = self.word_end(at + 1);
                    self.emit(Kind::Regex, start, self.pos);
                    return true;
                }
                c => at += c.len_utf8(),
            }
        }
        false
    }
}
