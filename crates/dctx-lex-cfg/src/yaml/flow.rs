// SPDX-License-Identifier: Apache-2.0
//! Flow collections, keys, colons and plain-scalar folding for the YAML walker.
use super::text::{Ahead, visual_columns};
use super::*;
use crate::domain::DiagnosticReason;
use crate::path::{Kind, skip_hash_comments};

impl Parser<'_> {
    pub(super) fn flow(&mut self, parent: Option<usize>, depth: u32) -> Result<(), LexError> {
        let (close, map) = match self.cur.byte() {
            Some(b'{') => (b'}', true),
            Some(b'[') => (b']', false),
            _ => return Ok(()),
        };
        self.enter(depth)?;
        self.cur.bump();
        let mut index = 0u32;
        loop {
            let _ = skip_hash_comments(&mut self.cur);
            if self.cur.eat(close) {
                break;
            }
            if self.cur.eof() {
                self.diagnose(self.cur.i, DiagnosticReason::UnbalancedBracket);
                break;
            }
            let start = self.cur.i;
            let Some(key) = self.flow_key(map, &mut index) else {
                break;
            };
            let nested = matches!(self.cur.byte(), Some(b'{' | b'['));
            let node = self.make(parent, &key, nested, depth, start)?;
            self.flow_value(node, depth)?;
            self.close(node);
            let _ = skip_hash_comments(&mut self.cur);
            if self.cur.eat(b',') {
                continue;
            }
            if self.cur.eat(close) {
                break;
            }
            if self.cur.eof() && !map {
                self.diagnose(self.cur.i, DiagnosticReason::UnbalancedBracket);
            } else {
                self.diagnose(self.cur.i, DiagnosticReason::InvalidSyntax);
                if !map && self.cur.i == start {
                    self.cur.bump();
                }
            }
            break;
        }
        Ok(())
    }
    fn flow_key(&mut self, map: bool, index: &mut u32) -> Option<String> {
        if !map {
            let segment = index.to_string();
            *index = index.saturating_add(1);
            return Some(segment);
        }
        let start = self.cur.i;
        self.broke = false;
        if let Some(key) = self.parse_key() {
            let _ = skip_hash_comments(&mut self.cur);
            return Some(key);
        }
        if !self.broke {
            self.diagnose(start, DiagnosticReason::InvalidSyntax);
        }
        if self.cur.i == start {
            self.cur.bump();
        }
        None
    }
    fn flow_value(&mut self, node: Option<usize>, depth: u32) -> Result<(), LexError> {
        let _ = skip_hash_comments(&mut self.cur);
        match self.cur.byte() {
            Some(b'{' | b'[') => self.flow(node, depth.saturating_add(1)),
            Some(b'"' | b'\'') => self.read_string(node, self.cur.i),
            Some(b'*') => {
                if let Some(alias) = self.take_mark(b'*') {
                    self.sign(node, &format!("*{alias}"));
                }
                Ok(())
            }
            _ => {
                let start = self.cur.i;
                while let Some(byte) = self.cur.byte() {
                    if matches!(byte, b',' | b'}' | b']' | b'\n' | b'#') {
                        break;
                    }
                    self.cur.i += 1;
                }
                let text = self.cur.slice(start, self.cur.i).trim().to_string();
                self.cite(node, &text, start)
            }
        }
    }
    pub(super) fn read_string(
        &mut self,
        node: Option<usize>,
        start: usize,
    ) -> Result<(), LexError> {
        if let Some(text) = self.quoted() {
            self.cite(node, &text, start)?;
        }
        Ok(())
    }
    pub(super) fn sign(&mut self, node: Option<usize>, text: &str) {
        if let Some(index) = node {
            self.draft.set_signature(index, text);
        }
    }
    pub(super) fn cite(
        &mut self,
        node: Option<usize>,
        text: &str,
        start: usize,
    ) -> Result<(), LexError> {
        let Some(index) = node else { return Ok(()) };
        self.draft.set_signature(index, text);
        self.draft.string_ref(index, text, start, self.cur.i)
    }
    pub(super) fn make(
        &mut self,
        parent: Option<usize>,
        key: &str,
        container: bool,
        depth: u32,
        start: usize,
    ) -> Result<Option<usize>, LexError> {
        let Some(parent) = parent else {
            if depth > self.max_depth {
                return Err(LexError::TooDeep {
                    line: self.draft.line(start),
                    limit: self.max_depth,
                });
            }
            return Ok(None);
        };
        Ok(self
            .draft
            .put(
                parent,
                key,
                Kind::Value(container),
                start,
                self.cur.i.max(start),
                depth,
            )?
            .index())
    }
    fn enter(&self, depth: u32) -> Result<(), LexError> {
        if depth > self.max_depth {
            Err(LexError::TooDeep {
                line: self.draft.line(self.cur.i),
                limit: self.max_depth,
            })
        } else {
            Ok(())
        }
    }
    pub(super) fn close(&mut self, node: Option<usize>) {
        if let Some(index) = node {
            self.draft.extend(index, self.cur.i);
        }
    }
    pub(super) fn doc(&mut self, node: Option<usize>, text: Option<&str>) {
        if let (Some(index), Some(text)) = (node, text) {
            self.draft.set_doc(index, text);
        }
    }
    pub(super) fn opens_nest(&self, indent: usize) -> bool {
        match self.cur.byte() {
            Some(b'{' | b'[') => true,
            Some(b'\n' | b'\r' | b'#') | None => {
                self.same_indent_sequence(indent) || self.peek_nested_block(indent)
            }
            _ => false,
        }
    }
    pub(super) fn indent(&self) -> usize {
        let bytes = self.cur.src.as_bytes();
        let end = self.cur.end.min(bytes.len());
        let at = self.cur.i.min(end);
        let mut start = at;
        while start > 0 && bytes[start - 1] != b'\n' {
            start -= 1;
        }
        let mut cursor = start;
        while cursor < at && matches!(bytes.get(cursor), Some(b' ' | b'\t')) {
            cursor += 1;
        }
        visual_columns(&bytes[start..cursor])
    }
    pub(super) fn cursor_column(&self) -> usize {
        let bytes = self.cur.src.as_bytes();
        let end = self.cur.end.min(bytes.len());
        let at = self.cur.i.min(end);
        let mut start = at;
        while start > 0 && bytes[start - 1] != b'\n' {
            start -= 1;
        }
        visual_columns(&bytes[start..at])
    }
    pub(super) fn at_dash(&self) -> bool {
        self.dash_at(self.cur.i)
    }
    fn dash_at(&self, index: usize) -> bool {
        let bytes = self.cur.src.as_bytes();
        bytes.get(index) == Some(&b'-')
            && matches!(
                bytes.get(index + 1),
                None | Some(b' ' | b'\t' | b'\n' | b'\r' | b'#')
            )
    }
    pub(super) fn looks_like_key(&self) -> bool {
        match self.cur.byte() {
            Some(b'"' | b'\'') => self.quoted_key_or_broken(self.cur.i),
            _ => self.find_colon(self.cur.i).is_some(),
        }
    }
    /// A closed quote is a key only when `:` follows. An unclosed quote still
    /// counts so the string scanner can report it as unterminated.
    fn quoted_key_or_broken(&self, start: usize) -> bool {
        let Some(end) = self.closed_quote_end(start) else {
            return true;
        };
        let bytes = self.cur.src.as_bytes();
        let limit = self.cur.end.min(bytes.len());
        let mut index = end;
        while index < limit && matches!(bytes.get(index), Some(b' ' | b'\t')) {
            index += 1;
        }
        index < limit && bytes.get(index) == Some(&b':') && self.colon_value(index)
    }
    fn closed_quote_end(&self, start: usize) -> Option<usize> {
        let bytes = self.cur.src.as_bytes();
        let limit = self.cur.end.min(bytes.len());
        let quote = *bytes.get(start)?;
        if quote != b'"' && quote != b'\'' {
            return None;
        }
        let mut index = start + 1;
        while index < limit {
            let byte = bytes[index];
            if byte == b'\n' {
                return None;
            }
            if byte == b'\\' && quote == b'"' {
                index += 1;
                if index >= limit || bytes[index] == b'\n' {
                    return None;
                }
                index += 1;
                continue;
            }
            if byte == quote {
                index += 1;
                if quote == b'\'' && bytes.get(index) == Some(&b'\'') {
                    index += 1;
                    continue;
                }
                return Some(index);
            }
            index += 1;
        }
        None
    }
    pub(super) fn directive(&self) -> bool {
        let rest = self.cur.rest();
        rest.starts_with("%YAML") || rest.starts_with("%TAG")
    }
    pub(super) fn same_indent_sequence(&self, indent: usize) -> bool {
        self.peek_line()
            .is_some_and(|(next, ahead)| next == indent && matches!(ahead, Ahead::Dash))
    }
    pub(super) fn peek_nested_block(&self, indent: usize) -> bool {
        self.peek_line()
            .is_some_and(|(next, ahead)| next > indent && matches!(ahead, Ahead::Dash | Ahead::Key))
    }
    pub(super) fn parse_key(&mut self) -> Option<String> {
        self.cur.skip_spaces();
        if matches!(self.cur.byte(), Some(b'"' | b'\'')) {
            let key = self.quoted()?;
            self.cur.skip_spaces();
            if !self.eat_colon() {
                self.broke = true;
                self.diagnose(self.cur.i, DiagnosticReason::InvalidSyntax);
                return None;
            }
            return Some(key);
        }
        let start = self.cur.i;
        let colon = self.find_colon(start)?;
        let key = self.cur.slice(start, colon).trim().to_string();
        if key.is_empty() {
            return None;
        }
        self.cur.i = colon + 1;
        Some(key)
    }
    fn find_colon(&self, start: usize) -> Option<usize> {
        let bytes = self.cur.src.as_bytes();
        let end = self.cur.end.min(bytes.len());
        let mut index = start;
        while index < end {
            if bytes[index] == b'\n' || bytes[index] == b'#' {
                return None;
            }
            if bytes[index] == b':' && self.colon_value(index) {
                return Some(index);
            }
            index += 1;
        }
        None
    }
    fn colon_value(&self, index: usize) -> bool {
        matches!(
            self.cur.src.as_bytes().get(index + 1),
            None | Some(
                b' ' | b'\t'
                    | b'\n'
                    | b'\r'
                    | b'#'
                    | b'{'
                    | b'['
                    | b'|'
                    | b'>'
                    | b'&'
                    | b'*'
                    | b'!'
                    | b'"'
                    | b'\''
            )
        )
    }
    fn eat_colon(&mut self) -> bool {
        if self.cur.byte() == Some(b':') && self.colon_value(self.cur.i) {
            self.cur.i += 1;
            true
        } else {
            false
        }
    }
    pub(super) fn peek_indent(&self) -> Option<usize> {
        self.peek_line().map(|(indent, _)| indent)
    }
    fn peek_line(&self) -> Option<(usize, Ahead)> {
        let bytes = self.cur.src.as_bytes();
        let end = self.cur.end.min(bytes.len());
        let mut index = self.cur.i;
        while index < end && bytes[index] != b'\n' {
            index += 1;
        }
        loop {
            if index >= end {
                return None;
            }
            if bytes[index] == b'\n' {
                index += 1;
            }
            let line_start = index;
            while index < end && matches!(bytes.get(index), Some(b' ' | b'\t')) {
                index += 1;
            }
            if index >= end {
                return None;
            }
            if matches!(bytes.get(index), Some(b'\n' | b'\r' | b'#')) {
                while index < end && bytes[index] != b'\n' {
                    index += 1;
                }
                continue;
            }
            let indent = visual_columns(&bytes[line_start..index]);
            let ahead = if self.dash_at(index) {
                Ahead::Dash
            } else if self.key_from(index) {
                Ahead::Key
            } else {
                Ahead::Plain
            };
            return Some((indent, ahead));
        }
    }
    fn key_from(&self, index: usize) -> bool {
        let bytes = self.cur.src.as_bytes();
        if matches!(bytes.get(index), Some(b'"' | b'\'')) {
            return self.quoted_key_or_broken(index);
        }
        self.find_colon(index).is_some()
    }
    fn plain(&mut self) -> String {
        let bytes = self.cur.src.as_bytes();
        let limit = self.cur.end.min(bytes.len());
        let start = self.cur.i;
        let mut end = start;
        while end < limit {
            let byte = bytes[end];
            if byte == b'\n'
                || byte == b'\r'
                || (byte == b'#' && end > start && matches!(bytes[end - 1], b' ' | b'\t'))
            {
                break;
            }
            end += 1;
        }
        let text = self.cur.slice(start, end).trim().to_string();
        self.cur.i = end;
        text
    }
    pub(super) fn fold_plain(&mut self, parent_indent: usize) -> String {
        let mut text = self.plain();
        while self.plain_more(parent_indent) {
            let before = self.cur.i;
            while self.cur.byte().is_some_and(|byte| byte != b'\n') {
                self.cur.i += 1;
            }
            if !self.cur.eat(b'\n') {
                break;
            }
            self.cur.skip_spaces();
            let more = self.plain();
            if more.is_empty() || self.cur.i == before {
                break;
            }
            if !text.is_empty() {
                text.push(' ');
            }
            text.push_str(&more);
        }
        text
    }
    pub(super) fn plain_more(&self, parent_indent: usize) -> bool {
        let Some((indent, ahead)) = self.peek_line() else {
            return false;
        };
        indent > parent_indent && matches!(ahead, Ahead::Plain)
    }
    pub(super) fn block_scalar(&mut self, parent_indent: usize) -> String {
        self.cur.bump();
        let mut keep_tail = false;
        while let Some(byte) = self.cur.byte() {
            match byte {
                b'+' => {
                    keep_tail = true;
                    self.cur.bump();
                }
                b'-' => {
                    keep_tail = false;
                    self.cur.bump();
                }
                b'0'..=b'9' => {
                    self.cur.bump();
                }
                _ => break,
            }
        }
        self.cur.consume_line();
        let mut parts = Vec::new();
        let mut content_indent: Option<usize> = None;
        while !self.cur.eof() {
            let (indent, empty) = self.line_shape();
            if empty {
                if !self.block_resumes(parent_indent, content_indent) {
                    break;
                }
                self.cur.consume_line();
                if keep_tail {
                    parts.push(String::new());
                }
                continue;
            }
            if indent <= parent_indent {
                break;
            }
            if content_indent.is_some_and(|min| indent < min) {
                break;
            }
            if content_indent.is_none() {
                content_indent = Some(indent);
            }
            while matches!(self.cur.byte(), Some(b' ' | b'\t')) {
                self.cur.bump();
            }
            let start = self.cur.i;
            while self.cur.byte().is_some_and(|byte| byte != b'\n') {
                self.cur.i += 1;
            }
            parts.push(
                self.cur
                    .slice(start, self.cur.i)
                    .trim_end_matches('\r')
                    .trim()
                    .to_string(),
            );
            self.cur.eat(b'\n');
        }
        if !keep_tail {
            while parts.last().is_some_and(std::string::String::is_empty) {
                parts.pop();
            }
        }
        parts
            .into_iter()
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }
    fn line_shape(&self) -> (usize, bool) {
        let bytes = self.cur.src.as_bytes();
        let end = self.cur.end.min(bytes.len());
        let start = self.cur.i.min(end);
        let mut index = start;
        while index < end && matches!(bytes[index], b' ' | b'\t') {
            index += 1;
        }
        let indent = visual_columns(&bytes[start..index]);
        let empty = index >= end || matches!(bytes.get(index), Some(b'\n' | b'\r'));
        (indent, empty)
    }
    fn block_resumes(&self, parent_indent: usize, content_indent: Option<usize>) -> bool {
        let bytes = self.cur.src.as_bytes();
        let end = self.cur.end.min(bytes.len());
        let mut index = self.cur.i;
        loop {
            if index >= end {
                return false;
            }
            let line_start = index;
            while index < end && matches!(bytes[index], b' ' | b'\t') {
                index += 1;
            }
            if index >= end {
                return false;
            }
            if matches!(bytes[index], b'\n' | b'\r') {
                while index < end && bytes[index] != b'\n' {
                    index += 1;
                }
                if index < end {
                    index += 1;
                }
                continue;
            }
            let indent = visual_columns(&bytes[line_start..index]);
            if indent <= parent_indent {
                return false;
            }
            if content_indent.is_some_and(|min| indent < min) {
                return false;
            }
            return true;
        }
    }
}
