// SPDX-License-Identifier: Apache-2.0
//! TOML structural walk: tables, arrays of tables, dotted keys and inline tables.
//! Implements the configuration extractor for `.toml` (PARSERS-v1, SPEC-MAP-MODE 5).
//! Malformed spans become diagnostics. The draft in `path` owns facts.
use crate::domain::{DiagnosticReason, Dialect, LexError, LexedFile, Limits};
use crate::path::{Cur, Draft, Kind, hash_comment, skip_hash_comments};
use std::collections::BTreeMap;

pub(crate) fn extract(path: &str, source: &str, limits: &Limits) -> Result<LexedFile, LexError> {
    let mut parser = Parser {
        cur: Cur::new(source),
        draft: Draft::open(path, source, limits.max_facts, limits.max_depth)?,
        max_depth: limits.max_depth,
        table: Some(0),
        table_depth: 0,
        arrays: BTreeMap::new(),
    };
    parser.walk()?;
    parser.draft.finish(Dialect::Toml)
}

struct Parser<'a> {
    cur: Cur<'a>,
    draft: Draft<'a>,
    max_depth: u32,
    table: Option<usize>,
    table_depth: u32,
    arrays: BTreeMap<String, u32>,
}

impl Parser<'_> {
    fn diagnose(&mut self, byte: usize, reason: DiagnosticReason) {
        self.draft.diagnose(byte, reason);
    }

    fn walk(&mut self) -> Result<(), LexError> {
        loop {
            let doc = skip_hash_comments(&mut self.cur);
            if self.cur.eof() {
                break;
            }
            let mark = self.cur.i;
            if self.cur.byte() == Some(b'[') {
                self.header(doc)?;
            } else {
                self.assignment(doc)?;
            }
            if self.cur.i == mark && !self.cur.eof() {
                self.diagnose(mark, DiagnosticReason::InvalidSyntax);
                self.cur.i += 1;
            }
        }
        Ok(())
    }

    #[allow(
        clippy::needless_pass_by_value,
        reason = "Pending comment text is transferred at grammar dispatch boundaries and is not reusable by the caller after that item is handled."
    )]
    fn header(&mut self, doc: Option<String>) -> Result<(), LexError> {
        // A rejected header must not leave the previous table as the owner of
        // subsequent assignments. Resume emission only after a valid header.
        self.table = None;
        self.table_depth = 0;
        let start = self.cur.i;
        if !self.cur.eat(b'[') {
            self.cur.consume_line();
            return Ok(());
        }
        let array = self.cur.eat(b'[');
        self.cur.skip_spaces();
        let Some(segments) = self.key_path() else {
            self.diagnose(self.cur.i, DiagnosticReason::InvalidSyntax);
            self.cur.consume_line();
            return Ok(());
        };
        self.cur.skip_spaces();
        if array && !self.cur.eat(b']') {
            self.diagnose(self.cur.i, DiagnosticReason::InvalidSyntax);
            self.cur.consume_line();
            return Ok(());
        }
        if !self.cur.eat(b']') {
            self.diagnose(self.cur.i, DiagnosticReason::InvalidSyntax);
            self.cur.consume_line();
            return Ok(());
        }
        self.table_depth = u32::try_from(segments.len())
            .unwrap_or(u32::MAX)
            .saturating_add(u32::from(array));
        let mut parent = 0usize;
        let mut depth = 0u32;
        let last = segments.len() - 1;
        for (ordinal, segment) in segments.iter().enumerate() {
            depth += 1;
            let kind = if ordinal < last {
                Kind::Implicit
            } else if array {
                Kind::Array
            } else {
                Kind::Table
            };
            let Some(index) = self
                .draft
                .put(parent, segment, kind, start, self.cur.i, depth)?
                .index()
            else {
                self.cur.consume_line();
                return Ok(());
            };
            parent = index;
        }
        let node = if array {
            let name = self.draft.full_name(parent);
            let slot = self.arrays.entry(name).or_insert(0);
            let ordinal = *slot;
            *slot += 1;
            depth += 1;
            let segment = ordinal.to_string();
            self.draft
                .put(
                    parent,
                    &segment,
                    Kind::Value(true),
                    start,
                    self.cur.i,
                    depth,
                )?
                .index()
        } else {
            Some(parent)
        };
        if let Some(index) = node {
            if let Some(text) = &doc {
                self.draft.set_doc(index, text);
            }
            if let Some(text) = self.trailing() {
                self.draft.set_doc(index, &text);
            }
            self.table = Some(index);
            self.table_depth = depth;
            self.draft.extend(index, self.cur.i);
        }
        Ok(())
    }

    #[allow(
        clippy::needless_pass_by_value,
        reason = "Pending comment text is transferred at grammar dispatch boundaries and is not reusable by the caller after that item is handled."
    )]
    fn assignment(&mut self, doc: Option<String>) -> Result<(), LexError> {
        let start = self.cur.i;
        let Some(segments) = self.key_path() else {
            self.diagnose(start, DiagnosticReason::InvalidSyntax);
            self.cur.consume_line();
            return Ok(());
        };
        self.skip_separator();
        if !self.cur.eat(b'=') {
            self.diagnose(self.cur.i, DiagnosticReason::InvalidSyntax);
            self.cur.consume_line();
            return Ok(());
        }
        self.skip_separator();
        let value_depth = self
            .table_depth
            .saturating_add(u32::try_from(segments.len()).unwrap_or(u32::MAX));
        let Some(mut parent) = self.table else {
            self.skip_value(value_depth)?;
            return Ok(());
        };
        let mut depth = self.table_depth;
        let last = segments.len() - 1;
        for segment in &segments[..last] {
            depth += 1;
            let Some(index) = self
                .draft
                .put(parent, segment, Kind::Implicit, start, self.cur.i, depth)?
                .index()
            else {
                self.skip_value(value_depth)?;
                return Ok(());
            };
            parent = index;
        }
        depth += 1;
        let container = matches!(self.cur.byte(), Some(b'{' | b'['));
        let node = self
            .draft
            .put(
                parent,
                &segments[last],
                Kind::Value(container),
                start,
                self.cur.i,
                depth,
            )?
            .index();
        if let (Some(index), Some(text)) = (node, doc.as_deref()) {
            self.draft.set_doc(index, text);
        }
        self.value(node, depth)?;
        if let Some(index) = node {
            if let Some(text) = self.trailing() {
                self.draft.set_doc(index, &text);
            }
            self.draft.extend(index, self.cur.i);
        }
        Ok(())
    }

    fn value(&mut self, node: Option<usize>, depth: u32) -> Result<(), LexError> {
        if depth > self.max_depth {
            return Err(LexError::TooDeep {
                line: self.draft.line(self.cur.i),
                limit: self.max_depth,
            });
        }
        match self.cur.byte() {
            Some(b'"' | b'\'') => {
                let start = self.cur.i;
                if let Some(text) = self.string()
                    && let Some(index) = node
                {
                    self.draft.set_signature(index, &text);
                    self.draft.string_ref(index, &text, start, self.cur.i)?;
                    self.draft.extend(index, self.cur.i);
                }
                Ok(())
            }
            Some(b'[') => self.array(node, depth),
            Some(b'{') => self.inline_table(node, depth),
            Some(_) => {
                let start = self.cur.i;
                self.bare();
                if let Some(index) = node {
                    self.draft
                        .set_signature(index, self.cur.slice(start, self.cur.i).trim());
                    self.draft.extend(index, self.cur.i);
                }
                Ok(())
            }
            None => {
                self.diagnose(self.cur.i, DiagnosticReason::InvalidSyntax);
                Ok(())
            }
        }
    }

    fn skip_value(&mut self, depth: u32) -> Result<(), LexError> {
        self.value(None, depth)
    }

    fn array(&mut self, parent: Option<usize>, depth: u32) -> Result<(), LexError> {
        self.cur.bump();
        let mut index = 0u32;
        loop {
            let doc = skip_hash_comments(&mut self.cur);
            if self.cur.eat(b']') {
                break;
            }
            if self.cur.eof() {
                self.diagnose(self.cur.i, DiagnosticReason::UnbalancedBracket);
                break;
            }
            let mark = self.cur.i;
            let segment = index.to_string();
            index = index.saturating_add(1);
            let child_depth = depth.saturating_add(1);
            let container = matches!(self.cur.byte(), Some(b'{' | b'['));
            let node = if let Some(parent) = parent {
                let declared = self.draft.put(
                    parent,
                    &segment,
                    Kind::Value(container),
                    mark,
                    mark,
                    child_depth,
                )?;
                if let Some(index) = declared.index()
                    && let Some(text) = &doc
                {
                    self.draft.set_doc(index, text);
                }
                declared.index()
            } else if child_depth > self.max_depth {
                return Err(LexError::TooDeep {
                    line: self.draft.line(mark),
                    limit: self.max_depth,
                });
            } else {
                None
            };
            self.value(node, child_depth)?;
            if let Some(index) = node {
                self.draft.extend(index, self.cur.i);
            }
            let _ = skip_hash_comments(&mut self.cur);
            if self.cur.eat(b',') {
                continue;
            }
            let _ = skip_hash_comments(&mut self.cur);
            if self.cur.eat(b']') {
                break;
            }
            if self.cur.eof() {
                self.diagnose(self.cur.i, DiagnosticReason::UnbalancedBracket);
                break;
            }
            self.diagnose(self.cur.i, DiagnosticReason::InvalidSyntax);
            if self.cur.i == mark {
                self.cur.i += 1;
            }
            break;
        }
        if let Some(parent) = parent {
            self.draft.extend(parent, self.cur.i);
        }
        Ok(())
    }

    fn inline_table(&mut self, parent: Option<usize>, depth: u32) -> Result<(), LexError> {
        self.cur.bump();
        loop {
            let doc = skip_hash_comments(&mut self.cur);
            if self.cur.eat(b'}') {
                break;
            }
            if self.cur.eof() {
                self.diagnose(self.cur.i, DiagnosticReason::UnbalancedBracket);
                break;
            }
            let start = self.cur.i;
            let Some(segments) = self.key_path() else {
                self.diagnose(start, DiagnosticReason::InvalidSyntax);
                break;
            };
            self.skip_separator();
            if !self.cur.eat(b'=') {
                self.diagnose(self.cur.i, DiagnosticReason::InvalidSyntax);
                break;
            }
            self.skip_separator();
            let value_depth =
                depth.saturating_add(u32::try_from(segments.len()).unwrap_or(u32::MAX));
            let Some(owner) = parent else {
                self.value(None, value_depth)?;
                let _ = skip_hash_comments(&mut self.cur);
                if self.cur.eat(b',') {
                    continue;
                }
                if self.cur.eat(b'}') {
                    break;
                }
                self.diagnose(self.cur.i, DiagnosticReason::InvalidSyntax);
                break;
            };
            let mut current = owner;
            let mut child_depth = depth;
            let last = segments.len() - 1;
            let mut skipped = false;
            for segment in &segments[..last] {
                child_depth += 1;
                let Some(index) = self
                    .draft
                    .put(
                        current,
                        segment,
                        Kind::Implicit,
                        start,
                        self.cur.i,
                        child_depth,
                    )?
                    .index()
                else {
                    skipped = true;
                    break;
                };
                current = index;
            }
            if skipped {
                self.value(None, value_depth)?;
            } else {
                child_depth += 1;
                let container = matches!(self.cur.byte(), Some(b'{' | b'['));
                let node = self
                    .draft
                    .put(
                        current,
                        &segments[last],
                        Kind::Value(container),
                        start,
                        self.cur.i,
                        child_depth,
                    )?
                    .index();
                if let (Some(index), Some(text)) = (node, doc.as_deref()) {
                    self.draft.set_doc(index, text);
                }
                self.value(node, child_depth)?;
                if let Some(index) = node {
                    self.draft.extend(index, self.cur.i);
                }
            }
            let _ = skip_hash_comments(&mut self.cur);
            if self.cur.eat(b',') {
                continue;
            }
            if self.cur.eat(b'}') {
                break;
            }
            self.diagnose(self.cur.i, DiagnosticReason::InvalidSyntax);
            break;
        }
        if let Some(parent) = parent {
            self.draft.extend(parent, self.cur.i);
        }
        Ok(())
    }

    fn key_path(&mut self) -> Option<Vec<String>> {
        let mut parts = Vec::new();
        loop {
            self.cur.skip_spaces();
            let part = self.one_key()?;
            parts.push(part);
            self.cur.skip_spaces();
            if self.cur.eat(b'.') {
                continue;
            }
            break;
        }
        Some(parts)
    }

    fn one_key(&mut self) -> Option<String> {
        match self.cur.byte() {
            Some(b'"' | b'\'') => self.string(),
            Some(b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-') => {
                let start = self.cur.i;
                while matches!(
                    self.cur.byte(),
                    Some(b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-')
                ) {
                    self.cur.i += 1;
                }
                Some(self.cur.slice(start, self.cur.i).to_string())
            }
            _ => None,
        }
    }

    fn string(&mut self) -> Option<String> {
        let quote = self.cur.byte()?;
        if quote != b'"' && quote != b'\'' {
            return None;
        }
        let marker = if quote == b'"' { "\"\"\"" } else { "'''" };
        let triple = self.cur.starts(marker);
        self.cur.i += if triple { 3 } else { 1 };
        let literal = quote == b'\'';
        let mut out = String::new();
        loop {
            if self.cur.eof() {
                self.diagnose(self.cur.i, DiagnosticReason::UnterminatedString);
                return None;
            }
            if triple && self.cur.starts(marker) {
                self.cur.i += 3;
                return Some(out);
            }
            if !triple && self.cur.byte() == Some(quote) {
                self.cur.i += 1;
                return Some(out);
            }
            if !triple && self.cur.byte() == Some(b'\n') {
                self.diagnose(self.cur.i, DiagnosticReason::UnterminatedString);
                return None;
            }
            if !literal && self.cur.byte() == Some(b'\\') {
                self.cur.i += 1;
                match self.cur.byte() {
                    Some(b'"') => out.push('"'),
                    Some(b'\\') => out.push('\\'),
                    Some(b'b') => out.push('\u{0008}'),
                    Some(b'f') => out.push('\u{000c}'),
                    Some(b'n') => out.push('\n'),
                    Some(b'r') => out.push('\r'),
                    Some(b't') => out.push('\t'),
                    Some(b'\n') if triple => {
                        self.cur.i += 1;
                        self.cur.skip_spaces();
                        continue;
                    }
                    Some(b'\r') if triple => {
                        self.cur.i += 1;
                        if self.cur.byte() == Some(b'\n') {
                            self.cur.i += 1;
                        }
                        self.cur.skip_spaces();
                        continue;
                    }
                    Some(b'u' | b'U') => {
                        let width = if self.cur.byte() == Some(b'u') { 4 } else { 8 };
                        self.cur.i += 1;
                        let hex = self.cur.slice(self.cur.i, self.cur.i.saturating_add(width));
                        if hex.len() != width || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                            self.diagnose(self.cur.i, DiagnosticReason::InvalidSyntax);
                            return None;
                        }
                        let Ok(code) = u32::from_str_radix(hex, 16) else {
                            self.diagnose(self.cur.i, DiagnosticReason::InvalidSyntax);
                            return None;
                        };
                        self.cur.i += width;
                        let Some(ch) = char::from_u32(code) else {
                            self.diagnose(self.cur.i, DiagnosticReason::InvalidSyntax);
                            return None;
                        };
                        out.push(ch);
                        continue;
                    }
                    _ => {
                        self.diagnose(self.cur.i, DiagnosticReason::InvalidSyntax);
                        return None;
                    }
                }
                self.cur.i += 1;
                continue;
            }
            if let Some(ch) = self
                .cur
                .src
                .get(self.cur.i..)
                .and_then(|text| text.chars().next())
            {
                out.push(ch);
                self.cur.i += ch.len_utf8();
            } else {
                self.cur.i += 1;
            }
        }
    }

    fn bare(&mut self) {
        while let Some(byte) = self.cur.byte() {
            if matches!(byte, b'\n' | b'#' | b',' | b']' | b'}') {
                break;
            }
            self.cur.i += 1;
        }
    }

    fn skip_separator(&mut self) {
        while let Some(b' ' | b'\t' | b'\r' | b'\n') = self.cur.byte() {
            self.cur.i += 1;
        }
    }

    fn trailing(&mut self) -> Option<String> {
        let save = self.cur.i;
        self.cur.skip_spaces();
        if self.cur.byte() == Some(b'#') {
            Some(hash_comment(&mut self.cur))
        } else {
            self.cur.i = save;
            None
        }
    }
}

#[cfg(test)]
mod tests;
