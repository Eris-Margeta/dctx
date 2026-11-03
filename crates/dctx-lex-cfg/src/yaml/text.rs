// SPDX-License-Identifier: Apache-2.0
//! Quoted scalars, marks, document splitting and line-shape helpers for the YAML walker.
use super::*;
use crate::domain::DiagnosticReason;
use crate::path::hash_comment;

impl Parser<'_> {
    pub(super) fn quoted(&mut self) -> Option<String> {
        let quote = self.cur.byte()?;
        if quote != b'"' && quote != b'\'' { return None; }
        self.cur.i += 1;
        let mut out = String::new();
        loop {
            match self.cur.byte() {
                None => return self.bad_string(),
                Some(b'\n' | b'\r') => {
                    // An unclosed quote stops at the break so later keys stay visible.
                    if !self.quote_closes_later(quote) { return self.bad_string(); }
                    self.fold_break(&mut out);
                }
                Some(byte) if byte == quote => {
                    self.cur.i += 1;
                    if quote == b'\'' && self.cur.eat(b'\'') { out.push('\''); continue; }
                    return Some(out);
                }
                Some(b'\\') if quote == b'"' => {
                    if self.escaped_break() {
                        if !self.quote_closes_later(quote) { return self.bad_string(); }
                        self.cur.i += 1;
                        self.consume_break();
                        while matches!(self.cur.byte(), Some(b' ' | b'\t')) { self.cur.i += 1; }
                        continue;
                    }
                    self.cur.i += 1;
                    let Some(byte) = self.cur.bump() else { return self.bad_string() };
                    out.push(match byte { b'n' => '\n', b't' => '\t', b'"' => '"', b'\\' => '\\', other => char::from(other) });
                }
                Some(_) => {
                    let Some(ch) = self.cur.rest().chars().next() else { self.cur.i += 1; continue; };
                    out.push(ch);
                    self.cur.i += ch.len_utf8();
                }
            }
        }
    }
    fn escaped_break(&self) -> bool {
        matches!(self.cur.src.as_bytes().get(self.cur.i + 1), Some(b'\n' | b'\r'))
    }
    fn fold_break(&mut self, out: &mut String) {
        self.consume_break();
        if !out.is_empty() && !out.ends_with(' ') { out.push(' '); }
    }
    fn consume_break(&mut self) {
        if self.cur.byte() == Some(b'\r') { self.cur.i += 1; }
        if self.cur.byte() == Some(b'\n') { self.cur.i += 1; }
    }
    /// True when a closer for `quote` exists before this document ends.
    fn quote_closes_later(&self, quote: u8) -> bool {
        let bytes = self.cur.src.as_bytes();
        let end = self.cur.end.min(bytes.len());
        let mut index = self.cur.i;
        while index < end {
            let byte = bytes[index];
            if quote == b'"' && byte == b'\\' {
                if index + 1 >= end { return false; }
                index += 2;
                continue;
            }
            if byte == quote {
                if quote == b'\'' && bytes.get(index + 1) == Some(&b'\'') {
                    index += 2;
                    continue;
                }
                return true;
            }
            index += 1;
        }
        false
    }
    fn bad_string(&mut self) -> Option<String> {
        self.broke = true;
        self.diagnose(self.cur.i, DiagnosticReason::UnterminatedString);
        None
    }
    pub(super) fn take_mark(&mut self, mark: u8) -> Option<String> {
        if self.cur.byte() != Some(mark) { return None; }
        let save = self.cur.i;
        self.cur.i += 1;
        let start = self.cur.i;
        while matches!(self.cur.byte(), Some(b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-')) { self.cur.i += 1; }
        if self.cur.i == start { self.cur.i = save; None } else { Some(self.cur.slice(start, self.cur.i).to_string()) }
    }
    pub(super) fn skip_tag(&mut self) {
        if self.cur.byte() != Some(b'!') { return; }
        while self.cur.byte().is_some_and(|byte| !byte.is_ascii_whitespace()) { self.cur.i += 1; }
    }
    pub(super) fn trailing(&mut self) -> Option<String> {
        let save = self.cur.i;
        self.cur.skip_spaces();
        if self.cur.byte() == Some(b'#') {
            let text = hash_comment(&mut self.cur);
            if text.is_empty() { None } else { Some(text) }
        } else {
            self.cur.i = save;
            None
        }
    }
}

pub(super) enum Ahead {
    Dash,
    Key,
    Plain,
}

pub(super) fn visual_columns(bytes: &[u8]) -> usize {
    let mut column = 0usize;
    for &byte in bytes {
        if byte == b'\t' {
            column = (column / 8 + 1) * 8;
        } else {
            column += 1;
        }
    }
    column
}

/// Where a line-oriented scan is when the next line begins.
#[derive(Clone, Copy)]
enum Carry {
    Plain,
    Double,
    Single,
    Block(usize),
    Flow(i32),
}

pub(super) fn documents(source: &str) -> Vec<(usize, usize)> {
    let bytes = source.as_bytes();
    let mut ranges = Vec::new();
    let mut start = 0usize;
    let mut line_start = 0usize;
    let mut carry = Carry::Plain;
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] != b'\n' {
            index += 1;
            continue;
        }
        let (next, split) = take_line(&source[line_start..index], carry);
        if split {
            push_doc(&mut ranges, source, start, line_start);
            start = index + 1;
        }
        carry = next;
        line_start = index + 1;
        index += 1;
    }
    if line_start < source.len() {
        let (_, split) = take_line(&source[line_start..], carry);
        let end = if split { line_start } else { source.len() };
        push_doc(&mut ranges, source, start, end);
    } else {
        push_doc(&mut ranges, source, start, source.len());
    }
    if ranges.is_empty() { ranges.push((0, source.len())); }
    ranges
}

fn take_line(line: &str, carry: Carry) -> (Carry, bool) {
    let line = line.strip_suffix('\r').unwrap_or(line);
    match carry {
        Carry::Double => (resume_quote(line, b'"'), false),
        Carry::Single => (resume_quote(line, b'\''), false),
        Carry::Block(parent) => {
            if line_empty(line) || indent_of(line.as_bytes()) > parent {
                (Carry::Block(parent), false)
            } else {
                take_line(line, Carry::Plain)
            }
        }
        Carry::Flow(depth) => {
            if marker_at_column_0(line) {
                (Carry::Plain, true)
            } else {
                (scan_from(line.as_bytes(), 0, indent_of(line.as_bytes()), depth), false)
            }
        }
        Carry::Plain => {
            if marker_at_column_0(line) {
                (Carry::Plain, true)
            } else {
                (plain_line(line), false)
            }
        }
    }
}

fn plain_line(line: &str) -> Carry {
    let bytes = line.as_bytes();
    let mut index = 0;
    while index < bytes.len() && matches!(bytes[index], b' ' | b'\t') { index += 1; }
    scan_from(bytes, index, indent_of(bytes), 0)
}

fn resume_quote(line: &str, quote: u8) -> Carry {
    let bytes = line.as_bytes();
    let indent = indent_of(bytes);
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if quote == b'"' && byte == b'\\' {
            if index + 1 >= bytes.len() {
                return Carry::Double;
            }
            index += 2;
            continue;
        }
        if byte == quote {
            if quote == b'\'' && bytes.get(index + 1) == Some(&b'\'') {
                index += 2;
                continue;
            }
            return scan_from(bytes, index + 1, indent, 0);
        }
        index += 1;
    }
    if quote == b'"' { Carry::Double } else { Carry::Single }
}

enum QuoteEnd { Closed(usize), Open }

fn finish_quote(bytes: &[u8], start: usize, quote: u8) -> QuoteEnd {
    let mut index = start + 1;
    while index < bytes.len() {
        let byte = bytes[index];
        if quote == b'"' && byte == b'\\' {
            if index + 1 >= bytes.len() { return QuoteEnd::Open; }
            index += 2;
            continue;
        }
        if byte == quote {
            if quote == b'\'' && bytes.get(index + 1) == Some(&b'\'') {
                index += 2;
                continue;
            }
            return QuoteEnd::Closed(index + 1);
        }
        index += 1;
    }
    QuoteEnd::Open
}

fn scan_from(bytes: &[u8], mut index: usize, line_indent: usize, mut flow: i32) -> Carry {
    let mut pending = false;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b' ' || byte == b'\t' {
            index += 1;
            continue;
        }
        if byte == b'#' && flow == 0 { break; }
        if (byte == b'"' || byte == b'\'') && (flow > 0 || pending) {
            match finish_quote(bytes, index, byte) {
                QuoteEnd::Closed(next) => {
                    pending = false;
                    index = next;
                    continue;
                }
                QuoteEnd::Open => return if byte == b'"' { Carry::Double } else { Carry::Single },
            }
        }
        if pending && flow == 0 && (byte == b'|' || byte == b'>') {
            return Carry::Block(line_indent);
        }
        if byte == b'{' || byte == b'[' {
            flow += 1;
            pending = false;
            index += 1;
            continue;
        }
        if byte == b'}' || byte == b']' {
            if flow > 0 { flow -= 1; }
            pending = false;
            index += 1;
            continue;
        }
        if byte == b':' && flow == 0 && colon_value_at(bytes, index) {
            pending = true;
            index += 1;
            continue;
        }
        if byte == b'-' && flow == 0 && is_dash(bytes, index) {
            pending = true;
            index += 1;
            continue;
        }
        pending = false;
        index += 1;
    }
    if flow > 0 { Carry::Flow(flow) } else { Carry::Plain }
}

fn marker_at_column_0(line: &str) -> bool {
    if matches!(line.as_bytes().first(), Some(b' ' | b'\t')) { return false; }
    let rest = if let Some(rest) = line.strip_prefix("---") {
        rest
    } else if let Some(rest) = line.strip_prefix("...") {
        rest
    } else {
        return false;
    };
    let rest = rest.trim_start();
    rest.is_empty() || rest.starts_with('#')
}

fn line_empty(line: &str) -> bool {
    line.bytes().all(|byte| matches!(byte, b' ' | b'\t'))
}

fn indent_of(bytes: &[u8]) -> usize {
    let mut index = 0;
    while index < bytes.len() && matches!(bytes[index], b' ' | b'\t') { index += 1; }
    visual_columns(&bytes[..index])
}

fn is_dash(bytes: &[u8], index: usize) -> bool {
    bytes.get(index) == Some(&b'-') && matches!(bytes.get(index + 1), None | Some(b' ' | b'\t' | b'#'))
}

fn colon_value_at(bytes: &[u8], index: usize) -> bool {
    matches!(
        bytes.get(index + 1),
        None | Some(
            b' ' | b'\t' | b'#' | b'{' | b'[' | b'|' | b'>' | b'&' | b'*' | b'!' | b'"' | b'\''
        )
    )
}

fn push_doc(ranges: &mut Vec<(usize, usize)>, source: &str, start: usize, end: usize) {
    let Some(body) = source.get(start..end) else { return };
    if body.lines().any(|line| {
        let text = line.trim();
        !text.is_empty() && !text.starts_with('#') && !text.starts_with("%YAML") && !text.starts_with("%TAG")
    }) {
        ranges.push((start, end));
    }
}
