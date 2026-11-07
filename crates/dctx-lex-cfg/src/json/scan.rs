// SPDX-License-Identifier: Apache-2.0
//! Whitespace, comments and diagnostics for the JSON walker.
use super::*;

impl<'a> Parser<'a> {
    pub(super) fn diagnose(&mut self, byte: usize, reason: DiagnosticReason) {
        self.draft.diagnose(byte, reason);
    }

    pub(super) fn skip(&mut self) -> Option<String> {
        let mut lead: Option<String> = None;
        let bytes = self.cur.src.as_bytes();
        let limit = self.cur.end.min(bytes.len());
        loop {
            if self.cur.i < limit && matches!(bytes[self.cur.i], b' ' | b'\t' | b'\r') {
                while self.cur.i < limit && matches!(bytes[self.cur.i], b' ' | b'\t' | b'\r') {
                    self.cur.i += 1;
                }
                continue;
            }
            match self.cur.byte() {
                Some(b'\n') => {
                    self.cur.i += 1;
                    let save = self.cur.i;
                    self.cur.skip_spaces();
                    if self.cur.byte() == Some(b'\n') {
                        lead = None;
                    }
                    self.cur.i = save;
                }
                Some(b'/') => {
                    if !self.jsonc {
                        self.diagnose(self.cur.i, DiagnosticReason::InvalidSyntax);
                    }
                    let Some(text) = self.comment() else { break };
                    if self.jsonc && lead.is_none() && !text.is_empty() {
                        lead = Some(text);
                    }
                }
                _ => break,
            }
        }
        lead
    }

    fn comment(&mut self) -> Option<String> {
        if self.cur.starts("//") {
            self.cur.i += 2;
            let start = self.cur.i;
            while let Some(byte) = self.cur.byte() {
                if byte == b'\n' {
                    break;
                }
                self.cur.i += 1;
            }
            return Some(self.cur.slice(start, self.cur.i).trim().to_string());
        }
        if self.cur.starts("/*") {
            self.cur.i += 2;
            let start = self.cur.i;
            if let Some(rel) = self.cur.rest().find("*/") {
                let end = self.cur.i + rel;
                let text = self.cur.slice(start, end).to_string();
                self.cur.i = end + 2;
                let first = text
                    .lines()
                    .next()
                    .unwrap_or("")
                    .trim()
                    .trim_start_matches('*')
                    .trim();
                return Some(first.to_string());
            }
            self.diagnose(start, DiagnosticReason::UnterminatedComment);
            self.cur.i = self.cur.end;
            return None;
        }
        self.diagnose(self.cur.i, DiagnosticReason::InvalidSyntax);
        self.cur.i += 1;
        None
    }

    pub(super) fn closer_after_space(&self, close: u8) -> bool {
        let bytes = self.cur.src.as_bytes();
        let end = self.cur.end.min(bytes.len());
        let mut index = self.cur.i;
        while index < end && matches!(bytes[index], b' ' | b'\t' | b'\r' | b'\n') {
            index += 1;
        }
        bytes.get(index) == Some(&close)
    }

    pub(super) fn trailing(&mut self) -> Option<String> {
        if !self.jsonc {
            return None;
        }
        let save = self.cur.i;
        self.cur.skip_spaces();
        if self.cur.byte() == Some(b'/') {
            self.comment()
        } else {
            self.cur.i = save;
            None
        }
    }
}
