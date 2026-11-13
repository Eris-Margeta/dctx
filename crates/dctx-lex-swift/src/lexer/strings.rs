// SPDX-License-Identifier: Apache-2.0
//! Swift quoted, multiline and extended-delimiter string scanning.
//! Implements nested interpolation under the same PARSERS-v1 depth ceiling.
//! Literal text remains opaque; interpolation expressions become ordinary tokens.
use super::{Kind, scan::Scanner};
use crate::domain::{DiagnosticReason, LexError};

impl Scanner<'_> {
    pub(super) fn string(&mut self, hashes: usize, depth: u32) -> Result<(), LexError> {
        self.depth(depth)?;
        let start = self.pos;
        self.pos += hashes;
        let multiline = self.src[self.pos..].starts_with("\"\"\"");
        let quote = if multiline { "\"\"\"" } else { "\"" };
        let suffix = "#".repeat(hashes);
        let close = format!("{quote}{suffix}");
        let escape = format!("\\{suffix}");
        self.pos += quote.len();
        let mut segment = start;
        while self.pos < self.src.len() {
            let rest = &self.src.as_bytes()[self.pos..];
            self.pos += rest
                .iter()
                .position(|byte| {
                    matches!(byte, b'"' | b'\\') || (!multiline && matches!(byte, b'\n' | b'\r'))
                })
                .unwrap_or(rest.len());
            if self.pos == self.src.len() {
                break;
            }
            if self.src[self.pos..].starts_with(&close) {
                self.pos += close.len();
                self.emit(segment, Kind::Str);
                return Ok(());
            }
            if !multiline && self.src[self.pos..].starts_with(['\n', '\r']) {
                break;
            }
            if self.src[self.pos..].starts_with(&escape) {
                self.pos += escape.len();
                if self.src[self.pos..].starts_with('(') {
                    self.emit(segment, Kind::Str);
                    let paren = self.pos;
                    self.pos += 1;
                    self.emit(paren, Kind::Punct);
                    if !self.region(true, depth + 1)? {
                        break;
                    }
                    segment = self.pos;
                } else if self.pos < self.src.len() {
                    self.advance();
                }
            } else {
                self.advance();
            }
        }
        self.emit(segment, Kind::Str);
        self.problem(start, DiagnosticReason::UnterminatedString);
        Ok(())
    }
}
