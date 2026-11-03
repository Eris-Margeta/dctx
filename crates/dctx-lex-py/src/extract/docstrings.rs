// SPDX-License-Identifier: Apache-2.0
//! Module and suite docstrings, taken from the first statement's string literals.
//! Implements SPEC-MAP-MODE 5.4 summaries for Python docstrings and leading comments.
//! The walker asks for these before it consumes the suite they document.
use super::Walker;
use crate::domain::{decode_doc, sentence};
use crate::lexer::Kind;
use dctx_map_model::{Summary, SummarySource};

impl Walker<'_> {
    pub(super) fn set_module_summary(&mut self, text: String) {
        if let Some(module) = self.draft.nodes.first_mut() {
            module.node.summary = Summary {
                text,
                source: SummarySource::Doc,
            };
        }
    }

    pub(super) fn lookahead_module_doc(&self) -> Option<String> {
        let mut j = 0;
        while self.kind_at(j) == Some(Kind::Newline) {
            j += 1;
        }
        let hi = statement_end(self, j);
        self.doc_statement(j, hi)
    }

    pub(super) fn lookahead_suite_doc(&self) -> Option<String> {
        let mut j = self.i;
        if self.op_at(j) != Some(":") {
            return None;
        }
        j += 1;
        if self.kind_at(j) == Some(Kind::Newline) {
            j += 1;
        }
        if self.kind_at(j) == Some(Kind::Indent) {
            j += 1;
        }
        while self.kind_at(j) == Some(Kind::Newline) {
            j += 1;
        }
        let hi = statement_end(self, j);
        self.doc_statement(j, hi)
    }

    fn doc_statement(&self, mut lo: usize, mut hi: usize) -> Option<String> {
        if lo >= hi {
            return None;
        }
        if self.op_at(lo) == Some("(") && hi > lo && self.op_at(hi - 1) == Some(")") {
            lo += 1;
            hi -= 1;
        }
        if lo >= hi {
            return None;
        }
        let mut text = String::new();
        for token in self.toks.get(lo..hi)? {
            if token.kind != Kind::Str || !token.doc {
                return None;
            }
            text.push_str(&decode_doc(self.src, token.start, token.end)?);
        }
        let text = sentence(&text);
        if text.is_empty() { None } else { Some(text) }
    }
}

fn statement_end(walker: &Walker<'_>, lo: usize) -> usize {
    let mut j = lo;
    let mut depth = 0i32;
    while j < walker.toks.len() {
        if depth <= 0
            && (walker.op_at(j) == Some(";")
                || matches!(
                    walker.kind_at(j),
                    Some(Kind::Newline | Kind::Dedent | Kind::Indent)
                ))
        {
            break;
        }
        depth += walker.delta(j);
        j += 1;
    }
    j
}
