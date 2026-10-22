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

impl Parser<'_> {
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
            } else if self.directive() {
                self.cur.consume_line();
                if self.cur.i == mark { self.cur.bump(); }
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
    #[allow(clippy::option_option, reason = "The outer Option distinguishes the end of the current YAML block; the inner Option is the optional leading comment for a continuing item.")]
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
    #[allow(clippy::needless_pass_by_value, reason = "Pending comment text is transferred at grammar dispatch boundaries and is not reusable by the caller after that item is handled.")]
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
        if matches!(self.cur.byte(), Some(b'|' | b'>')) {
            let node = self.make(parent, segment, false, child_depth, start)?;
            self.doc(node, lead.as_deref());
            let text = self.block_scalar(dash_indent);
            self.sign(node, &text);
            self.close(node);
            return Ok(());
        }
        if self.looks_like_key() {
            // The dash column is the line indent; sibling keys align with the key text.
            let key_indent = self.cursor_column();
            let node = self.make(parent, segment, true, child_depth, start)?;
            self.doc(node, lead.as_deref());
            self.broke = false;
            let key_at = self.cur.i;
            if let Some(key) = self.parse_key() {
                self.mapped_value(node, &key, key_indent, child_depth, key_at, None)?;
                self.parse_mappings(node, key_indent, child_depth, None)?;
            } else {
                if !self.broke { self.diagnose(key_at, DiagnosticReason::InvalidSyntax); }
                self.cur.consume_line();
            }
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
            let text = self.fold_plain(dash_indent);
            self.cite(node, &text, start)?;
        }
        self.close(node);
        Ok(())
    }
    #[allow(clippy::needless_pass_by_value, reason = "Pending comment text is transferred at grammar dispatch boundaries and is not reusable by the caller after that item is handled.")]
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
            if self.cur.byte() == Some(b'#') {
                let comment = hash_comment(&mut self.cur);
                if !comment.is_empty() { self.doc(node, Some(&comment)); }
            }
            if matches!(self.cur.byte(), None | Some(b'\n' | b'\r' | b'#')) {
                self.nest_or_fold(node, indent, depth, anchor.as_deref(), start)?;
            } else {
                let text = self.fold_plain(indent);
                self.finish_scalar(node, &text, anchor.as_deref(), start)?;
            }
        }
        self.close(node);
        Ok(())
    }
    fn nest_or_fold(&mut self, node: Option<usize>, indent: usize, depth: u32, anchor: Option<&str>, start: usize) -> Result<(), LexError> {
        if self.same_indent_sequence(indent) {
            self.parse_sequence(node, indent, depth.saturating_add(1), None)?;
        } else if self.peek_nested_block(indent) {
            if let Some(next) = self.peek_indent().filter(|&next| next > indent) {
                self.parse_block(node, next, depth.saturating_add(1))?;
            }
        } else if self.plain_more(indent) {
            let text = self.fold_plain(indent);
            self.finish_scalar(node, &text, anchor, start)?;
        }
        Ok(())
    }
    fn finish_scalar(&mut self, node: Option<usize>, text: &str, anchor: Option<&str>, start: usize) -> Result<(), LexError> {
        let Some(index) = node else { return Ok(()) };
        if text.is_empty() { return Ok(()) }
        let signature = match anchor {
            Some(anchor) => format!("&{anchor} {text}"),
            None => text.to_string(),
        };
        self.sign(node, &signature);
        self.draft.string_ref(index, text, start, self.cur.i)?;
        if let Some(comment) = self.trailing() { self.draft.set_doc(index, &comment); }
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
            Some(b'{' | b'[') => self.flow(node, depth.saturating_add(1)),
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
            Some(b'\n' | b'\r' | b'#') | None => {
                self.same_indent_sequence(indent) || self.peek_nested_block(indent)
            }
            _ => false,
        }
    }
    fn indent(&self) -> usize {
        let bytes = self.cur.src.as_bytes();
        let end = self.cur.end.min(bytes.len());
        let at = self.cur.i.min(end);
        let mut start = at;
        while start > 0 && bytes[start - 1] != b'\n' { start -= 1; }
        let mut cursor = start;
        while cursor < at && matches!(bytes.get(cursor), Some(b' ' | b'\t')) { cursor += 1; }
        visual_columns(&bytes[start..cursor])
    }
    fn cursor_column(&self) -> usize {
        let bytes = self.cur.src.as_bytes();
        let end = self.cur.end.min(bytes.len());
        let at = self.cur.i.min(end);
        let mut start = at;
        while start > 0 && bytes[start - 1] != b'\n' { start -= 1; }
        visual_columns(&bytes[start..at])
    }
    fn at_dash(&self) -> bool { self.dash_at(self.cur.i) }
    fn dash_at(&self, index: usize) -> bool {
        let bytes = self.cur.src.as_bytes();
        bytes.get(index) == Some(&b'-')
            && matches!(bytes.get(index + 1), None | Some(b' ' | b'\t' | b'\n' | b'\r' | b'#'))
    }
    fn looks_like_key(&self) -> bool {
        match self.cur.byte() {
            Some(b'"' | b'\'') => self.quoted_key_or_broken(self.cur.i),
            _ => self.find_colon(self.cur.i).is_some(),
        }
    }
    /// A closed quote is a key only when `:` follows. An unclosed quote still
    /// counts so the string scanner can report it as unterminated.
    fn quoted_key_or_broken(&self, start: usize) -> bool {
        let Some(end) = self.closed_quote_end(start) else { return true };
        let bytes = self.cur.src.as_bytes();
        let limit = self.cur.end.min(bytes.len());
        let mut index = end;
        while index < limit && matches!(bytes.get(index), Some(b' ' | b'\t')) { index += 1; }
        index < limit && bytes.get(index) == Some(&b':') && self.colon_value(index)
    }
    fn closed_quote_end(&self, start: usize) -> Option<usize> {
        let bytes = self.cur.src.as_bytes();
        let limit = self.cur.end.min(bytes.len());
        let quote = *bytes.get(start)?;
        if quote != b'"' && quote != b'\'' { return None; }
        let mut index = start + 1;
        while index < limit {
            let byte = bytes[index];
            if byte == b'\n' { return None; }
            if byte == b'\\' && quote == b'"' {
                index += 1;
                if index >= limit || bytes[index] == b'\n' { return None; }
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
    fn directive(&self) -> bool {
        let rest = self.cur.rest();
        rest.starts_with("%YAML") || rest.starts_with("%TAG")
    }
    fn same_indent_sequence(&self, indent: usize) -> bool {
        self.peek_line().is_some_and(|(next, ahead)| next == indent && matches!(ahead, Ahead::Dash))
    }
    fn peek_nested_block(&self, indent: usize) -> bool {
        self.peek_line().is_some_and(|(next, ahead)| {
            next > indent && matches!(ahead, Ahead::Dash | Ahead::Key)
        })
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
    fn peek_indent(&self) -> Option<usize> { self.peek_line().map(|(indent, _)| indent) }
    fn peek_line(&self) -> Option<(usize, Ahead)> {
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
            if byte == b'\n' || byte == b'\r' || (byte == b'#' && end > start && matches!(bytes[end - 1], b' ' | b'\t')) {
                break;
            }
            end += 1;
        }
        let text = self.cur.slice(start, end).trim().to_string();
        self.cur.i = end;
        text
    }
    fn fold_plain(&mut self, parent_indent: usize) -> String {
        let mut text = self.plain();
        while self.plain_more(parent_indent) {
            let before = self.cur.i;
            while self.cur.byte().is_some_and(|byte| byte != b'\n') { self.cur.i += 1; }
            if !self.cur.eat(b'\n') { break; }
            self.cur.skip_spaces();
            let more = self.plain();
            if more.is_empty() || self.cur.i == before { break; }
            if !text.is_empty() { text.push(' '); }
            text.push_str(&more);
        }
        text
    }
    fn plain_more(&self, parent_indent: usize) -> bool {
        let Some((indent, ahead)) = self.peek_line() else { return false };
        indent > parent_indent && matches!(ahead, Ahead::Plain)
    }
    fn block_scalar(&mut self, parent_indent: usize) -> String {
        self.cur.bump();
        let mut keep_tail = false;
        while let Some(byte) = self.cur.byte() {
            match byte {
                b'+' => { keep_tail = true; self.cur.bump(); }
                b'-' => { keep_tail = false; self.cur.bump(); }
                b'0'..=b'9' => { self.cur.bump(); }
                _ => break,
            }
        }
        self.cur.consume_line();
        let mut parts = Vec::new();
        let mut content_indent: Option<usize> = None;
        while !self.cur.eof() {
            let (indent, empty) = self.line_shape();
            if empty {
                if !self.block_resumes(parent_indent, content_indent) { break; }
                self.cur.consume_line();
                if keep_tail { parts.push(String::new()); }
                continue;
            }
            if indent <= parent_indent { break; }
            if content_indent.is_some_and(|min| indent < min) { break; }
            if content_indent.is_none() { content_indent = Some(indent); }
            while matches!(self.cur.byte(), Some(b' ' | b'\t')) { self.cur.bump(); }
            let start = self.cur.i;
            while self.cur.byte().is_some_and(|byte| byte != b'\n') { self.cur.i += 1; }
            parts.push(self.cur.slice(start, self.cur.i).trim_end_matches('\r').trim().to_string());
            self.cur.eat(b'\n');
        }
        if !keep_tail {
            while parts.last().is_some_and(std::string::String::is_empty) { parts.pop(); }
        }
        parts.into_iter().filter(|line| !line.is_empty()).collect::<Vec<_>>().join(" ")
    }
    fn line_shape(&self) -> (usize, bool) {
        let bytes = self.cur.src.as_bytes();
        let end = self.cur.end.min(bytes.len());
        let start = self.cur.i.min(end);
        let mut index = start;
        while index < end && matches!(bytes[index], b' ' | b'\t') { index += 1; }
        let indent = visual_columns(&bytes[start..index]);
        let empty = index >= end || matches!(bytes.get(index), Some(b'\n' | b'\r'));
        (indent, empty)
    }
    fn block_resumes(&self, parent_indent: usize, content_indent: Option<usize>) -> bool {
        let bytes = self.cur.src.as_bytes();
        let end = self.cur.end.min(bytes.len());
        let mut index = self.cur.i;
        loop {
            if index >= end { return false; }
            let line_start = index;
            while index < end && matches!(bytes[index], b' ' | b'\t') { index += 1; }
            if index >= end { return false; }
            if matches!(bytes[index], b'\n' | b'\r') {
                while index < end && bytes[index] != b'\n' { index += 1; }
                if index < end { index += 1; }
                continue;
            }
            let indent = visual_columns(&bytes[line_start..index]);
            if indent <= parent_indent { return false; }
            if content_indent.is_some_and(|min| indent < min) { return false; }
            return true;
        }
    }
    fn quoted(&mut self) -> Option<String> {
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

enum Ahead {
    Dash,
    Key,
    Plain,
}

fn visual_columns(bytes: &[u8]) -> usize {
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

fn documents(source: &str) -> Vec<(usize, usize)> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Completion;

    fn names(file: &LexedFile) -> Vec<&str> {
        file.nodes.iter().map(|node| node.fact.name.as_str()).collect()
    }

    fn signature<'a>(file: &'a LexedFile, name: &str) -> &'a str {
        file.nodes
            .iter()
            .find(|node| node.fact.name == name)
            .map_or("", |node| node.fact.signature.as_str())
    }

    #[test]
    fn block_markers_stay_inside_the_scalar() {
        let limits = Limits::default();
        let literal = extract("a.yaml", "msg: |\n  ---\nb: 1\n", &limits).unwrap();
        assert_eq!(literal.completion, Completion::Complete, "{:?}", literal.diagnostics);
        assert_eq!(names(&literal), ["a.yaml", "msg", "b"]);
        assert_eq!(signature(&literal, "msg"), "---");
        assert_eq!(signature(&literal, "b"), "1");

        let folded = extract("a.yaml", "msg: >\n  ...\nb: 1\n", &limits).unwrap();
        assert_eq!(folded.completion, Completion::Complete, "{:?}", folded.diagnostics);
        assert_eq!(names(&folded), ["a.yaml", "msg", "b"]);
        assert_eq!(signature(&folded, "msg"), "...");

        let quoted = extract("a.yaml", "a: \"hello\n---\nworld\"\nb: 1\n", &limits).unwrap();
        assert_eq!(quoted.completion, Completion::Complete, "{:?}", quoted.diagnostics);
        assert_eq!(names(&quoted), ["a.yaml", "a", "b"]);
        assert_eq!(signature(&quoted, "a"), "hello --- world");

        let single = extract("a.yaml", "a: 'hello\n  ...\nworld'\nb: 1\n", &limits).unwrap();
        assert_eq!(single.completion, Completion::Complete, "{:?}", single.diagnostics);
        assert_eq!(names(&single), ["a.yaml", "a", "b"]);
        assert!(signature(&single, "a").contains("..."), "{}", signature(&single, "a"));

        let docs = extract("a.yaml", "---\na: 1\n---\nb: 2\n", &limits).unwrap();
        assert_eq!(docs.completion, Completion::Complete, "{:?}", docs.diagnostics);
        assert_eq!(names(&docs), ["a.yaml", "0", "0.a", "1", "1.b"]);
        let end = extract("a.yaml", "a: 1\n...\nb: 2\n", &limits).unwrap();
        assert_eq!(names(&end), ["a.yaml", "0", "0.a", "1", "1.b"]);
    }

    #[test]
    fn quoted_scalars_span_lines() {
        let limits = Limits::default();
        let double = extract("a.yaml", "a: \"hello\nworld\"\nb: 1\n", &limits).unwrap();
        assert_eq!(double.completion, Completion::Complete, "{:?}", double.diagnostics);
        assert_eq!(names(&double), ["a.yaml", "a", "b"]);
        assert_eq!(signature(&double, "a"), "hello world");
        assert_eq!(signature(&double, "b"), "1");

        let single = extract("a.yaml", "a: 'hello\nworld'\nb: 1\n", &limits).unwrap();
        assert_eq!(single.completion, Completion::Complete, "{:?}", single.diagnostics);
        assert_eq!(names(&single), ["a.yaml", "a", "b"]);
        assert_eq!(signature(&single, "a"), "hello world");

        let escaped = extract("a.yaml", "a: \"hello\\\nworld\"\nb: 1\n", &limits).unwrap();
        assert_eq!(escaped.completion, Completion::Complete, "{:?}", escaped.diagnostics);
        assert_eq!(signature(&escaped, "a"), "helloworld");
        assert_eq!(names(&escaped), ["a.yaml", "a", "b"]);

        let line = extract("a.yaml", "a: \"hello\"\nb: 1\n", &limits).unwrap();
        assert_eq!(line.completion, Completion::Complete, "{:?}", line.diagnostics);
        assert_eq!(signature(&line, "a"), "hello");
        assert_eq!(signature(&line, "b"), "1");
    }
}
