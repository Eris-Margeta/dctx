// SPDX-License-Identifier: Apache-2.0
//! Block structure: documents, mappings, sequences and block scalars for the YAML walker.
use super::text::documents;
use super::*;
use crate::domain::DiagnosticReason;
use crate::path::{Kind, hash_comment, skip_hash_comments};

impl Parser<'_> {
    pub(super) fn diagnose(&mut self, byte: usize, reason: DiagnosticReason) {
        self.draft.diagnose(byte, reason);
    }
    pub(super) fn walk(&mut self) -> Result<(), LexError> {
        let docs = documents(self.cur.src);
        let multi = docs.len() > 1;
        for (ordinal, (start, end)) in docs.into_iter().enumerate() {
            self.cur.i = start;
            self.cur.end = end;
            let parent = if multi {
                self.draft
                    .put(0, &ordinal.to_string(), Kind::Table, start, end, 1)?
                    .index()
            } else {
                Some(0)
            };
            self.parse_block(parent, 0, if multi { 2 } else { 1 })?;
        }
        Ok(())
    }
    fn parse_block(
        &mut self,
        parent: Option<usize>,
        min_indent: usize,
        depth: u32,
    ) -> Result<(), LexError> {
        loop {
            let mark = self.cur.i;
            let lead = skip_hash_comments(&mut self.cur);
            if self.cur.eof() {
                return Ok(());
            }
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
                if self.cur.i == mark {
                    self.cur.bump();
                }
            } else {
                self.diagnose(self.cur.i, DiagnosticReason::InvalidSyntax);
                self.cur.consume_line();
                if self.cur.i == mark {
                    self.cur.bump();
                }
            }
        }
    }
    fn row_break(&mut self, indent: usize, dash: bool) -> bool {
        self.cur.eof() || self.indent() != indent || self.at_dash() != dash
    }
    #[allow(
        clippy::option_option,
        reason = "The outer Option distinguishes the end of the current YAML block; the inner Option is the optional leading comment for a continuing item."
    )]
    fn take_lead(
        &mut self,
        indent: usize,
        dash: bool,
        first: &mut Option<String>,
    ) -> Option<Option<String>> {
        if first.is_some() {
            return Some(first.take());
        }
        let mark = self.cur.i;
        let lead = skip_hash_comments(&mut self.cur);
        if self.row_break(indent, dash) {
            self.cur.i = mark;
            None
        } else {
            Some(lead)
        }
    }
    fn parse_mappings(
        &mut self,
        parent: Option<usize>,
        indent: usize,
        depth: u32,
        mut first: Option<String>,
    ) -> Result<(), LexError> {
        while let Some(lead) = self.take_lead(indent, false, &mut first) {
            let start = self.cur.i;
            self.broke = false;
            let Some(key) = self.parse_key() else {
                if !self.broke {
                    self.diagnose(start, DiagnosticReason::InvalidSyntax);
                }
                self.cur.consume_line();
                if self.cur.eof() {
                    return Ok(());
                }
                continue;
            };
            self.mapped_value(parent, &key, indent, depth, start, lead)?;
        }
        Ok(())
    }
    fn parse_sequence(
        &mut self,
        parent: Option<usize>,
        indent: usize,
        depth: u32,
        mut first: Option<String>,
    ) -> Result<(), LexError> {
        let mut index = 0u32;
        while let Some(lead) = self.take_lead(indent, true, &mut first) {
            let start = self.cur.i;
            self.cur.bump();
            if self.cur.byte() == Some(b' ') {
                self.cur.bump();
            }
            let segment = index.to_string();
            index = index.saturating_add(1);
            self.seq_item(parent, &segment, indent, depth, start, lead)?;
            if index == u32::MAX {
                return Ok(());
            }
        }
        Ok(())
    }
    #[allow(
        clippy::needless_pass_by_value,
        reason = "Pending comment text is transferred at grammar dispatch boundaries and is not reusable by the caller after that item is handled."
    )]
    fn seq_item(
        &mut self,
        parent: Option<usize>,
        segment: &str,
        dash_indent: usize,
        depth: u32,
        start: usize,
        lead: Option<String>,
    ) -> Result<(), LexError> {
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
                if !self.broke {
                    self.diagnose(key_at, DiagnosticReason::InvalidSyntax);
                }
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
    #[allow(
        clippy::needless_pass_by_value,
        reason = "Pending comment text is transferred at grammar dispatch boundaries and is not reusable by the caller after that item is handled."
    )]
    fn mapped_value(
        &mut self,
        parent: Option<usize>,
        key: &str,
        indent: usize,
        depth: u32,
        start: usize,
        lead: Option<String>,
    ) -> Result<(), LexError> {
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
        if let Some(anchor) = anchor.as_deref() {
            self.sign(node, &format!("&{anchor}"));
        }
        if matches!(self.cur.byte(), Some(b'{' | b'[')) {
            self.flow(node, depth.saturating_add(1))?;
        } else if matches!(self.cur.byte(), Some(b'|' | b'>')) {
            let text = self.block_scalar(indent);
            self.sign(node, &text);
        } else if matches!(self.cur.byte(), Some(b'"' | b'\'')) {
            self.read_string(node, start)?;
            if let Some(text) = self.trailing() {
                self.doc(node, Some(&text));
            }
        } else {
            if self.cur.byte() == Some(b'#') {
                let comment = hash_comment(&mut self.cur);
                if !comment.is_empty() {
                    self.doc(node, Some(&comment));
                }
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
    fn nest_or_fold(
        &mut self,
        node: Option<usize>,
        indent: usize,
        depth: u32,
        anchor: Option<&str>,
        start: usize,
    ) -> Result<(), LexError> {
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
    fn finish_scalar(
        &mut self,
        node: Option<usize>,
        text: &str,
        anchor: Option<&str>,
        start: usize,
    ) -> Result<(), LexError> {
        let Some(index) = node else { return Ok(()) };
        if text.is_empty() {
            return Ok(());
        }
        let signature = match anchor {
            Some(anchor) => format!("&{anchor} {text}"),
            None => text.to_string(),
        };
        self.sign(node, &signature);
        self.draft.string_ref(index, text, start, self.cur.i)?;
        if let Some(comment) = self.trailing() {
            self.draft.set_doc(index, &comment);
        }
        Ok(())
    }
}
