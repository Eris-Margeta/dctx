//! Swift member boundaries, enum cases, bindings and doc summaries.
//! Implements lexical signatures and tier-zero summaries from SPEC-MAP-MODE.
//! Used by declaration dispatch; balanced parameter groups stay opaque here.
use super::{Item, Walker, normalize};
use crate::domain::Precision;
use dctx_map_model::SymbolKind;

impl Walker<'_> {
    pub(super) fn boundary(&self, mut i: usize, end: usize) -> (usize, Option<usize>, u32) {
        while i < end {
            let text = self.text(i);
            if text == "{" {
                let last = self.close(i).unwrap_or(end.saturating_sub(1)).min(end - 1);
                return (last, Some(i), self.start(i));
            }
            if matches!(text, ";" | "}") {
                let last = i.saturating_sub(1);
                return (last, None, self.end(last));
            }
            if i > 0
                && self.lex.line(self.start(i)) > self.lex.line(self.end(i - 1))
                && !continues(self.text(i - 1))
                && !continues(text)
                && text != "where"
            {
                return (i - 1, None, self.end(i - 1));
            }
            if matches!(text, "(" | "[") {
                if let Some(close) = self.close(i) {
                    i = close + 1;
                    continue;
                }
                return (end - 1, None, self.end(end - 1));
            }
            i += 1;
        }
        let last = end.saturating_sub(1);
        (last, None, self.end(last))
    }

    pub(super) fn cases(
        &mut self,
        first: usize,
        at: usize,
        end: usize,
        owner: usize,
        public: bool,
    ) -> Result<usize, crate::LexError> {
        let (last, _, _) = self.boundary(at + 1, end);
        let mut i = at + 1;
        while i <= last && i < end {
            if !self.ident(i) {
                self.problem(i, crate::DiagnosticReason::UnsupportedName);
                break;
            }
            let name = self.text(i).to_string();
            let begin = i;
            i += 1;
            while i <= last && self.text(i) != "," {
                i = self.close(i).map_or(i + 1, |j| j + 1);
            }
            let item = Item {
                name,
                kind: Some(SymbolKind::Const),
                first: if begin == at + 1 { first } else { begin },
                last: i.saturating_sub(1),
                signature: self.end(i.saturating_sub(1)),
                public,
                precision: Precision::Structural,
            };
            self.add(&item, owner)?;
            i += 1;
        }
        Ok(last + 1)
    }

    pub(super) fn additional_bindings(
        &mut self,
        mut i: usize,
        end: usize,
        owner: usize,
        public: bool,
    ) -> Result<(), crate::LexError> {
        while i < end {
            if matches!(self.text(i), "(" | "[" | "{") {
                i = self.close(i).map_or(end, |j| j + 1);
                continue;
            }
            if self.text(i) == "," && self.ident(i + 1) {
                let first = i + 1;
                let mut last = first;
                while last + 1 < end && self.text(last + 1) != "," {
                    last = self.close(last + 1).unwrap_or(last + 1);
                }
                let item = Item {
                    name: self.text(first).into(),
                    kind: None,
                    first,
                    last,
                    signature: self.end(last),
                    public,
                    precision: Precision::Structural,
                };
                self.add(&item, owner)?;
                i = last + 1;
            } else {
                i += 1;
            }
        }
        Ok(())
    }

    pub(super) fn doc(&self, first: usize) -> Option<String> {
        let mut before = self.start(first);
        let mut parts = Vec::new();
        let prior = self.lex.comments.partition_point(|&(_, end)| end <= before);
        for &(start, end) in self.lex.comments[..prior].iter().rev() {
            let gap = &self.src[end as usize..before as usize];
            if !gap.chars().all(char::is_whitespace)
                || gap.bytes().filter(|b| *b == b'\n').count() > 1
            {
                break;
            }
            let raw = &self.src[start as usize..end as usize];
            let body = if let Some(body) = raw.strip_prefix("///") {
                body.trim().to_string()
            } else if let Some(body) = raw.strip_prefix("/**").and_then(|s| s.strip_suffix("*/")) {
                body.lines()
                    .map(|s| s.trim().trim_start_matches('*').trim())
                    .collect::<Vec<_>>()
                    .join(" ")
            } else {
                break;
            };
            parts.push(body);
            before = start;
        }
        parts.reverse();
        let doc = normalize(&parts.join(" "));
        if doc.is_empty() {
            None
        } else {
            Some(
                doc.find(". ")
                    .map_or_else(|| doc.clone(), |end| doc[..=end].to_string()),
            )
        }
    }
}

fn continues(text: &str) -> bool {
    matches!(
        text,
        "=" | "," | ":" | "->" | "where" | "<" | ">" | "&" | "." | "throws" | "async"
    )
}
