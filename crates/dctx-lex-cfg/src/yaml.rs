//! YAML block mappings, sequences, flow collections, anchors and multi-document files.
//! Implements the configuration extractor for `.yaml` and `.yml` (PARSERS-v1).
//! Anchors and aliases are recorded and never expanded. The draft in `path` owns facts.
use crate::domain::{DiagnosticReason, Dialect, LexError, LexedFile, Limits};
use crate::path::{hash_comment, skip_hash_comments, Cur, Draft, Kind};

pub(crate) fn extract(path: &str, source: &str, limits: &Limits) -> Result<LexedFile, LexError> {
    let mut parser = Parser {
        cur: Cur::new(source),
        draft: Draft::open(path, source, limits.max_facts, limits.max_depth)?,
        max_depth: limits.max_depth,
        broke: false,
    };
    parser.walk()?;
    parser.draft.finish(Dialect::Yaml)
}

struct Parser<'a> {
    cur: Cur<'a>,
    draft: Draft<'a>,
    max_depth: u32,
    broke: bool,
}

impl<'a> Parser<'a> {
    fn diagnose(&mut self, byte: usize, reason: DiagnosticReason) {
        self.draft.diagnose(byte, reason);
    }
    fn walk(&mut self) -> Result<(), LexError> {
        let docs = documents(self.cur.src);
        let multi = docs.len() > 1;
        for (ordinal, (start, end)) in docs.into_iter().enumerate() {
            self.cur.i = start;
            self.cur.end = end;
            let parent = if multi {
                self.draft.put(0, &ordinal.to_string(), Kind::Table, start, end, 1)?.index()
            } else {
                Some(0)
            };
            self.parse_block(parent, 0, if multi { 2 } else { 1 })?;
        }
        Ok(())
    }
    fn parse_block(&mut self, parent: Option<usize>, min_indent: usize, depth: u32) -> Result<(), LexError> {
        loop {
            let mark = self.cur.i;
            let lead = skip_hash_comments(&mut self.cur);
            if self.cur.eof() { return Ok(()); }
            let indent = self.indent();
            if indent < min_indent {
                self.cur.i = mark;
                return Ok(());
            }
            if self.at_dash() {
                self.parse_sequence(parent, indent, depth, lead)?;
            } else if self.looks_like_key() {
                self.parse_mappings(parent, indent, depth, lead)?;
            } else {
                self.diagnose(self.cur.i, DiagnosticReason::InvalidSyntax);
                self.cur.consume_line();
                if self.cur.i == mark { self.cur.bump(); }
            }
        }
    }
    fn row_break(&mut self, indent: usize, dash: bool) -> bool {
        self.cur.eof() || self.indent() != indent || self.at_dash() != dash
    }
    fn take_lead(&mut self, indent: usize, dash: bool, first: &mut Option<String>) -> Option<Option<String>> {
        if first.is_some() { return Some(first.take()); }
        let mark = self.cur.i;
        let lead = skip_hash_comments(&mut self.cur);
        if self.row_break(indent, dash) {
            self.cur.i = mark;
            None
        } else {
            Some(lead)
        }
    }
    fn parse_mappings(&mut self, parent: Option<usize>, indent: usize, depth: u32, mut first: Option<String>) -> Result<(), LexError> {
        while let Some(lead) = self.take_lead(indent, false, &mut first) {
            let start = self.cur.i;
            self.broke = false;
            let Some(key) = self.parse_key() else {
                if !self.broke { self.diagnose(start, DiagnosticReason::InvalidSyntax); }
                self.cur.consume_line();
                if self.cur.eof() { return Ok(()); }
                continue;
            };
            self.mapped_value(parent, &key, indent, depth, start, lead)?;
        }
        Ok(())
    }
    fn parse_sequence(&mut self, parent: Option<usize>, indent: usize, depth: u32, mut first: Option<String>) -> Result<(), LexError> {
        let mut index = 0u32;
        while let Some(lead) = self.take_lead(indent, true, &mut first) {
            let start = self.cur.i;
            self.cur.bump();
            if self.cur.byte() == Some(b' ') { self.cur.bump(); }
            let segment = index.to_string();
            index = index.saturating_add(1);
            self.seq_item(parent, &segment, indent, depth, start, lead)?;
            if index == u32::MAX { return Ok(()); }
        }
        Ok(())
    }
    fn seq_item(&mut self, parent: Option<usize>, segment: &str, dash_indent: usize, depth: u32, start: usize, lead: Option<String>) -> Result<(), LexError> {
        self.cur.skip_spaces();
        let child_depth = depth.saturating_add(1);
        if matches!(self.cur.byte(), Some(b'{' | b'[')) {
            let node = self.make(parent, segment, true, child_depth, start)?;
            self.doc(node, lead.as_deref());
            self.flow(node, child_depth)?;
            self.close(node);
            return Ok(());
        }
        if self.looks_like_key() {
            let node = self.make(parent, segment, true, child_depth, start)?;
            self.doc(node, lead.as_deref());
            self.parse_mappings(node, self.indent(), child_depth, None)?;
            self.close(node);
            return Ok(());
        }
        let alias = self.take_mark(b'*');
        if alias.is_none() && matches!(self.cur.byte(), None | Some(b'\n' | b'\r' | b'#')) {
            let node = self.make(parent, segment, true, child_depth, start)?;
            self.doc(node, lead.as_deref());
            if let Some(next) = self.peek_indent().filter(|&next| next > dash_indent) {
                self.parse_block(node, next, child_depth)?;
            }
            self.close(node);
            return Ok(());
        }
        let node = self.make(parent, segment, false, child_depth, start)?;
        self.doc(node, lead.as_deref());
        if let Some(alias) = alias {
            self.sign(node, &format!("*{alias}"));
            self.cur.consume_line();
        } else if matches!(self.cur.byte(), Some(b'"' | b'\'')) {
            self.read_string(node, start)?;
        } else {
            let text = self.plain();
            self.cite(node, &text, start)?;
        }
        self.close(node);
        Ok(())
    }
    fn mapped_value(&mut self, parent: Option<usize>, key: &str, indent: usize, depth: u32, start: usize, lead: Option<String>) -> Result<(), LexError> {
        self.cur.skip_spaces();
        let anchor = self.take_mark(b'&');
        self.cur.skip_spaces();
        let alias = self.take_mark(b'*');
        self.cur.skip_spaces();
        self.skip_tag();
        self.cur.skip_spaces();
        if let Some(alias) = alias {
            let node = self.make(parent, key, false, depth, start)?;
            self.doc(node, lead.as_deref());
            let signature = match anchor {
                Some(anchor) => format!("&{anchor} *{alias}"),
                None => format!("*{alias}"),
            };
            self.sign(node, &signature);
            self.cur.consume_line();
            self.close(node);
            return Ok(());
        }
        let nested = self.opens_nest(indent);
        let node = self.make(parent, key, nested, depth, start)?;
        self.doc(node, lead.as_deref());
        if let Some(anchor) = anchor.as_deref() { self.sign(node, &format!("&{anchor}")); }
        if matches!(self.cur.byte(), Some(b'{' | b'[')) {
            self.flow(node, depth.saturating_add(1))?;
        } else if matches!(self.cur.byte(), Some(b'|' | b'>')) {
            let text = self.block_scalar(indent);
            self.sign(node, &text);
        } else if matches!(self.cur.byte(), Some(b'"' | b'\'')) {
            self.read_string(node, start)?;
            if let Some(text) = self.trailing() { self.doc(node, Some(&text)); }
        } else {
            let text = self.plain();
            if let Some(next) = text.is_empty().then(|| self.peek_indent()).flatten().filter(|&next| next > indent) {
                self.parse_block(node, next, depth.saturating_add(1))?;
            } else if !text.is_empty() {
                let signature = match &anchor {
                    Some(anchor) => format!("&{anchor} {text}"),
                    None => text.clone(),
                };
                self.sign(node, &signature);
                if let Some(index) = node {
                    self.draft.string_ref(index, &text, start, self.cur.i)?;
                    if let Some(comment) = self.trailing() { self.draft.set_doc(index, &comment); }
                }
            }
        }
        self.close(node);
        Ok(())
    }
    fn flow(&mut self, parent: Option<usize>, depth: u32) -> Result<(), LexError> {
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
            if self.cur.eat(close) { break; }
            if self.cur.eof() {
                self.diagnose(self.cur.i, DiagnosticReason::UnbalancedBracket);
                break;
            }
            let start = self.cur.i;
            let Some(key) = self.flow_key(map, &mut index) else { break };
            let nested = matches!(self.cur.byte(), Some(b'{' | b'['));
            let node = self.make(parent, &key, nested, depth, start)?;
            self.flow_value(node, depth)?;
            self.close(node);
            let _ = skip_hash_comments(&mut self.cur);
            if self.cur.eat(b',') { continue; }
            if self.cur.eat(close) { break; }
            if self.cur.eof() && !map {
                self.diagnose(self.cur.i, DiagnosticReason::UnbalancedBracket);
            } else {
                self.diagnose(self.cur.i, DiagnosticReason::InvalidSyntax);
                if !map && self.cur.i == start { self.cur.bump(); }
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
        if !self.broke { self.diagnose(start, DiagnosticReason::InvalidSyntax); }
        if self.cur.i == start { self.cur.bump(); }
        None
    }
    fn flow_value(&mut self, node: Option<usize>, depth: u32) -> Result<(), LexError> {
        let _ = skip_hash_comments(&mut self.cur);
        match self.cur.byte() {
            Some(b'{') => self.flow(node, depth.saturating_add(1)),
            Some(b'[') => self.flow(node, depth.saturating_add(1)),
            Some(b'"' | b'\'') => self.read_string(node, self.cur.i),
            Some(b'*') => {
                if let Some(alias) = self.take_mark(b'*') { self.sign(node, &format!("*{alias}")); }
                Ok(())
            }
            _ => {
                let start = self.cur.i;
                while let Some(byte) = self.cur.byte() {
                    if matches!(byte, b',' | b'}' | b']' | b'\n' | b'#') { break; }
                    self.cur.i += 1;
                }
                let text = self.cur.slice(start, self.cur.i).trim().to_string();
                self.cite(node, &text, start)
            }
        }
    }
    fn read_string(&mut self, node: Option<usize>, start: usize) -> Result<(), LexError> {
        if let Some(text) = self.quoted() { self.cite(node, &text, start)?; }
        Ok(())
    }
    fn sign(&mut self, node: Option<usize>, text: &str) {
        if let Some(index) = node { self.draft.set_signature(index, text); }
    }
    fn cite(&mut self, node: Option<usize>, text: &str, start: usize) -> Result<(), LexError> {
        let Some(index) = node else { return Ok(()) };
        self.draft.set_signature(index, text);
        self.draft.string_ref(index, text, start, self.cur.i)
    }
    fn make(&mut self, parent: Option<usize>, key: &str, container: bool, depth: u32, start: usize) -> Result<Option<usize>, LexError> {
        let Some(parent) = parent else {
            if depth > self.max_depth {
                return Err(LexError::TooDeep { line: self.draft.line(start), limit: self.max_depth });
            }
            return Ok(None);
        };
        Ok(self.draft.put(parent, key, Kind::Value(container), start, self.cur.i.max(start), depth)?.index())
    }
    fn enter(&self, depth: u32) -> Result<(), LexError> {
        if depth > self.max_depth {
            Err(LexError::TooDeep { line: self.draft.line(self.cur.i), limit: self.max_depth })
        } else {
            Ok(())
        }
    }
    fn close(&mut self, node: Option<usize>) {
        if let Some(index) = node { self.draft.extend(index, self.cur.i); }
    }
    fn doc(&mut self, node: Option<usize>, text: Option<&str>) {
        if let (Some(index), Some(text)) = (node, text) { self.draft.set_doc(index, text); }
    }
    fn opens_nest(&self, indent: usize) -> bool {
        match self.cur.byte() {
            Some(b'{' | b'[') => true,
            Some(b'\n' | b'\r' | b'#') | None => self.peek_indent().is_some_and(|next| next > indent),
            _ => false,
        }
    }
    fn indent(&self) -> usize { self.column(true).0 }
    fn column(&self, cursor_limit: bool) -> (usize, bool) {
        let bytes = self.cur.src.as_bytes();
        let end = self.cur.end.min(bytes.len());
        let mut start = self.cur.i.min(end);
        while start > 0 && bytes[start - 1] != b'\n' { start -= 1; }
        let limit = if cursor_limit { self.cur.i.min(end) } else { end };
        let mut cursor = start;
        while cursor < limit && matches!(bytes.get(cursor), Some(b' ' | b'\t')) { cursor += 1; }
        let blank = cursor >= end || matches!(bytes.get(cursor), Some(b'\n' | b'\r' | b'#'));
        (cursor - start, blank)
    }
    fn at_dash(&self) -> bool {
        self.cur.byte() == Some(b'-')
            && matches!(self.cur.src.as_bytes().get(self.cur.i + 1), None | Some(b' ' | b'\n' | b'\r' | b'#'))
    }
    fn looks_like_key(&self) -> bool {
        matches!(self.cur.byte(), Some(b'"' | b'\'')) || self.find_colon(self.cur.i).is_some()
    }
    fn parse_key(&mut self) -> Option<String> {
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
        if key.is_empty() { return None; }
        self.cur.i = colon + 1;
        Some(key)
    }
    fn find_colon(&self, start: usize) -> Option<usize> {
        let bytes = self.cur.src.as_bytes();
        let end = self.cur.end.min(bytes.len());
        let mut index = start;
        while index < end {
            if bytes[index] == b'\n' || bytes[index] == b'#' { return None; }
            if bytes[index] == b':' && self.colon_value(index) { return Some(index); }
            index += 1;
        }
        None
    }
    fn colon_value(&self, index: usize) -> bool {
        matches!(
            self.cur.src.as_bytes().get(index + 1),
            None | Some(b' ' | b'\t' | b'\n' | b'\r' | b'#' | b'{' | b'[' | b'|' | b'>' | b'&' | b'*' | b'!' | b'"' | b'\'')
        )
    }
    fn eat_colon(&mut self) -> bool {
        if self.cur.byte() == Some(b':') && self.colon_value(self.cur.i) { self.cur.i += 1; true } else { false }
    }
    fn peek_indent(&self) -> Option<usize> {
        let bytes = self.cur.src.as_bytes();
        let end = self.cur.end.min(bytes.len());
        let mut index = self.cur.i;
        while index < end && bytes[index] != b'\n' { index += 1; }
        loop {
            if index >= end { return None; }
            if bytes[index] == b'\n' { index += 1; }
            let line_start = index;
            while index < end && matches!(bytes.get(index), Some(b' ' | b'\t')) { index += 1; }
            if index >= end { return None; }
            if matches!(bytes.get(index), Some(b'\n' | b'\r' | b'#')) {
                while index < end && bytes[index] != b'\n' { index += 1; }
                continue;
            }
            return Some(index - line_start);
        }
    }
    fn plain(&mut self) -> String {
        let bytes = self.cur.src.as_bytes();
        let limit = self.cur.end.min(bytes.len());
        let start = self.cur.i;
        let mut end = start;
        while end < limit {
            let byte = bytes[end];
            if byte == b'\n' || byte == b'\r' || (byte == b'#' && end > start && bytes[end - 1] == b' ') { break; }
            end += 1;
        }
        let text = self.cur.slice(start, end).trim().to_string();
        self.cur.i = end;
        text
    }
    fn block_scalar(&mut self, parent_indent: usize) -> String {
        self.cur.bump();
        while matches!(self.cur.byte(), Some(b'+' | b'-' | b'0'..=b'9')) { self.cur.bump(); }
        self.cur.consume_line();
        let mut parts = Vec::new();
        while !self.cur.eof() {
            let (indent, blank) = self.column(false);
            if blank || indent <= parent_indent { break; }
            while matches!(self.cur.byte(), Some(b' ' | b'\t')) { self.cur.bump(); }
            let start = self.cur.i;
            while self.cur.byte().is_some_and(|byte| byte != b'\n') { self.cur.i += 1; }
            parts.push(self.cur.slice(start, self.cur.i).trim().to_string());
            self.cur.eat(b'\n');
        }
        parts.join(" ")
    }
    fn quoted(&mut self) -> Option<String> {
        let quote = self.cur.byte()?;
        if quote != b'"' && quote != b'\'' { return None; }
        self.cur.i += 1;
        let mut out = String::new();
        loop {
            match self.cur.byte() {
                None | Some(b'\n') => return self.bad_string(),
                Some(byte) if byte == quote => {
                    self.cur.i += 1;
                    if quote == b'\'' && self.cur.eat(b'\'') { out.push('\''); continue; }
                    return Some(out);
                }
                Some(b'\\') if quote == b'"' => {
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
    fn bad_string(&mut self) -> Option<String> {
        self.broke = true;
        self.diagnose(self.cur.i, DiagnosticReason::UnterminatedString);
        None
    }
    fn take_mark(&mut self, mark: u8) -> Option<String> {
        if self.cur.byte() != Some(mark) { return None; }
        let save = self.cur.i;
        self.cur.i += 1;
        let start = self.cur.i;
        while matches!(self.cur.byte(), Some(b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-')) { self.cur.i += 1; }
        if self.cur.i == start { self.cur.i = save; None } else { Some(self.cur.slice(start, self.cur.i).to_string()) }
    }
    fn skip_tag(&mut self) {
        if self.cur.byte() != Some(b'!') { return; }
        while self.cur.byte().is_some_and(|byte| !byte.is_ascii_whitespace()) { self.cur.i += 1; }
    }
    fn trailing(&mut self) -> Option<String> {
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

fn documents(source: &str) -> Vec<(usize, usize)> {
    let bytes = source.as_bytes();
    let mut ranges = Vec::new();
    let mut start = 0usize;
    let mut line = 0usize;
    for (index, byte) in bytes.iter().enumerate() {
        if *byte != b'\n' { continue; }
        if separator(&source[line..index]) {
            push_doc(&mut ranges, source, start, line);
            start = index + 1;
        }
        line = index + 1;
    }
    let end = if separator(source.get(line..).unwrap_or("")) { line } else { source.len() };
    push_doc(&mut ranges, source, start, end);
    if ranges.is_empty() { ranges.push((0, source.len())); }
    ranges
}
fn separator(line: &str) -> bool { matches!(line.trim(), "---" | "...") }
fn push_doc(ranges: &mut Vec<(usize, usize)>, source: &str, start: usize, end: usize) {
    let Some(body) = source.get(start..end) else { return };
    if body.lines().any(|line| { let text = line.trim(); !text.is_empty() && !text.starts_with('#') }) {
        ranges.push((start, end));
    }
}
