//! Token cursor: spelling, brackets, declaration keywords and fact emission.
//! Implements the Java token walk for SPEC-MAP-MODE 5.1–5.3 scopes, symbols and references.
//! Depends on the draft in `extract` and on lexer partners and line starts.
//! Declaration and reference modules call these [`super::Walker`] methods.

use super::{DeclKind, Scope, SymbolDraft, Walker};
use crate::domain::{
    body_fingerprint, javadoc_summary, normalize, DiagnosticReason, LexError, Precision,
};
use crate::lexer::{line_of, Kind, NONE};
use dctx_map_model::{EdgeKind, Node, NodeKind, StableId, Summary, SummarySource};

impl<'a> Walker<'a> {
    pub(super) fn enter(&mut self, at: usize) -> bool {
        if self.depth >= self.max_depth {
            self.refuse_depth(at);
            return false;
        }
        self.depth += 1;
        true
    }

    pub(super) fn leave(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    pub(crate) fn refuse_depth(&mut self, at: usize) {
        if self.refused.is_none() {
            self.refused = Some(LexError::TooDeep {
                line: self.line_of_token(at),
                limit: self.max_depth,
            });
        }
    }

    pub(super) fn ok(&self) -> bool {
        self.refused.is_none()
    }

    pub(crate) fn kind(&self, index: usize) -> Option<Kind> {
        self.toks.get(index).map(|token| token.kind)
    }

    pub(crate) fn text(&self, index: usize) -> &'a str {
        let Some(token) = self.toks.get(index) else {
            return "";
        };
        self.src
            .get(token.start as usize..token.end as usize)
            .unwrap_or("")
    }

    pub(crate) fn word(&self, index: usize) -> bool {
        self.kind(index) == Some(Kind::Word)
    }

    pub(crate) fn is_word(&self, index: usize, spelling: &str) -> bool {
        self.word(index) && lexical_eq(self.text(index), spelling)
    }

    pub(crate) fn is_punct(&self, index: usize, spelling: &str) -> bool {
        self.kind(index) == Some(Kind::Punct) && self.text(index) == spelling
    }

    pub(crate) fn close(&self, index: usize) -> Option<usize> {
        let partner = *self.partner.get(index)?;
        if partner == NONE {
            return None;
        }
        let partner = partner as usize;
        (partner > index).then_some(partner)
    }

    #[allow(clippy::cast_possible_truncation, reason = "Source offsets, line counts and token counts fit u32 because the public extractor enforces the 8 MiB input ceiling.")]
    pub(crate) fn start_byte(&self, index: usize) -> u32 {
        self.toks
            .get(index).map_or_else(|| self.src.len() as u32, |token| token.start)
    }

    #[allow(clippy::cast_possible_truncation, reason = "Source offsets, line counts and token counts fit u32 because the public extractor enforces the 8 MiB input ceiling.")]
    pub(crate) fn end_byte(&self, index: usize) -> u32 {
        self.toks
            .get(index).map_or_else(|| self.src.len() as u32, |token| token.end)
    }

    fn line_of_byte(&self, byte: u32) -> u32 {
        line_of(self.line_starts, byte)
    }

    pub(crate) fn line_of_token(&self, index: usize) -> u32 {
        self.line_of_byte(self.start_byte(index))
    }

    pub(super) fn src_span(&self, start: u32, end: u32) -> &str {
        if end < start {
            return "";
        }
        self.src.get(start as usize..end as usize).unwrap_or("")
    }

    pub(crate) fn qualified(&self, from: usize, to: usize) -> String {
        if to < from {
            return String::new();
        }
        self.src_span(self.start_byte(from), self.end_byte(to))
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect()
    }

    pub(crate) fn signature(&self, first: usize, sig_end: u32) -> String {
        normalize(self.src_span(self.start_byte(first), sig_end))
    }

    fn doc_text(&self, token: usize) -> Option<String> {
        let doc = self.toks.get(token)?.doc;
        if doc == 0 {
            return None;
        }
        let comment = self.comments.get(doc as usize - 1)?;
        if !comment.javadoc {
            return None;
        }
        let text = self.src.get(comment.start as usize..comment.end as usize)?;
        javadoc_summary(text)
    }

    fn comment_start(&self, token: usize) -> Option<u32> {
        let doc = self.toks.get(token)?.doc;
        if doc == 0 {
            return None;
        }
        Some(self.comments.get(doc as usize - 1)?.start)
    }

    pub(crate) fn problem_at(&mut self, token: usize, reason: DiagnosticReason) {
        self.draft.problems.push((self.start_byte(token), reason));
    }

    pub(crate) fn decl_kind(&self, index: usize) -> Option<DeclKind> {
        if self.is_punct(index, "@") && self.is_word(index + 1, "interface") {
            return Some(DeclKind::Annotation);
        }
        if self.is_word(index, "class") {
            return Some(DeclKind::Class);
        }
        if self.is_word(index, "interface") {
            return Some(DeclKind::Interface);
        }
        if self.is_word(index, "enum") {
            return Some(DeclKind::Enum);
        }
        if self.is_word(index, "record") {
            return Some(DeclKind::Record);
        }
        None
    }

    pub(crate) fn name_at(keyword: usize, kind: DeclKind) -> usize {
        match kind {
            DeclKind::Annotation => keyword + 2,
            _ => keyword + 1,
        }
    }

    /// Emits a symbol and returns the scope of its body. `None` abstains.
    pub(crate) fn emit(&mut self, draft: SymbolDraft<'_>, parent: &Scope) -> Option<Scope> {
        if self.draft.overflow || !self.ok() {
            return None;
        }
        let qualified = if parent.name.is_empty() {
            draft.local.to_string()
        } else {
            format!("{}::{}", parent.name, draft.local)
        };
        if !valid_name(&qualified) {
            self.problem_at(draft.first, DiagnosticReason::UnsupportedName);
            return None;
        }
        if !self.draft.accept() {
            return None;
        }
        let path = self.draft.path.clone();
        let Ok(id) = StableId::new(draft.node_kind, &path, Some(&qualified), None) else {
            self.problem_at(draft.first, DiagnosticReason::UnsupportedName);
            return None;
        };
        let signature = self.signature(draft.first, draft.sig_end);
        let summary = match self.doc_text(draft.first) {
            Some(text) => Summary {
                text,
                source: SummarySource::Doc,
            },
            None => Summary {
                text: signature.clone(),
                source: SummarySource::Signature,
            },
        };
        let fingerprint = body_fingerprint(self.src_span(draft.body.0, draft.body.1));
        let start = self
            .comment_start(draft.first)
            .unwrap_or_else(|| self.start_byte(draft.first));
        let end = self.end_byte(draft.last);
        let lines = [
            self.line_of_byte(start),
            self.line_of_byte(end.saturating_sub(1).max(start)),
        ];
        let node = Node {
            id,
            kind: draft.node_kind,
            symbol_kind: Some(draft.symbol),
            path,
            lines,
            name: qualified.clone(),
            signature,
            fingerprint,
            public: draft.is_public,
            summary,
        };
        self.draft.nodes.push(super::DraftNode {
            node,
            parent: parent.owner,
            start,
            end,
            precision: draft.precision,
        });
        let mut type_params = parent.type_params.clone();
        type_params.extend(draft.type_params);
        Some(Scope {
            owner: self.draft.nodes.len() - 1,
            name: qualified,
            simple: draft.simple.to_string(),
            type_params,
            kind: draft.scope_kind,
            in_test: parent.in_test || draft.node_kind == NodeKind::Test,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn refer(
        &mut self,
        src: usize,
        scope: &Scope,
        kind: EdgeKind,
        name: String,
        alias: Option<String>,
        from: usize,
        to: usize,
        precision: Precision,
    ) {
        if name.is_empty() || !self.ok() || !self.draft.accept() {
            return;
        }
        let start = self.start_byte(from);
        let end = self.end_byte(to);
        let lines = [
            self.line_of_byte(start),
            self.line_of_byte(end.saturating_sub(1).max(start)),
        ];
        self.draft.refs.push(super::DraftRef {
            src,
            kind,
            name,
            scope: scope.name.clone(),
            alias,
            lines,
            start,
            end,
            precision,
        });
    }
}

fn valid_name(name: &str) -> bool {
    !name.is_empty() && !name.contains(['#', '~']) && !name.chars().any(char::is_control)
}

/// Keyword spelling uses the same `\u` translation as the scanner.
fn lexical_eq(raw: &str, spelling: &str) -> bool {
    if raw == spelling {
        return true;
    }
    if !raw.as_bytes().contains(&b'\\') {
        return false;
    }
    let mut pos = 0;
    for expected in spelling.chars() {
        let Some((ch, len)) = lexical_unit(raw, pos) else {
            return false;
        };
        if ch != expected {
            return false;
        }
        pos += len;
    }
    pos == raw.len()
}

fn lexical_unit(src: &str, pos: usize) -> Option<(char, usize)> {
    let bytes = src.as_bytes();
    if pos >= bytes.len() {
        return None;
    }
    if bytes[pos] == b'\\'
        && let Some(unit) = unicode_unit(bytes, pos)
    {
        return Some(unit);
    }
    let ch = src[pos..].chars().next()?;
    Some((ch, ch.len_utf8()))
}

fn unicode_unit(bytes: &[u8], pos: usize) -> Option<(char, usize)> {
    let mut cursor = pos + 1;
    if bytes.get(cursor) != Some(&b'u') {
        return None;
    }
    while bytes.get(cursor) == Some(&b'u') {
        cursor += 1;
    }
    let end = cursor.checked_add(4)?;
    if end > bytes.len() || !bytes[cursor..end].iter().all(u8::is_ascii_hexdigit) {
        return None;
    }
    let digits = std::str::from_utf8(&bytes[cursor..end]).ok()?;
    let code = u32::from_str_radix(digits, 16).ok()?;
    Some((char::from_u32(code)?, end - pos))
}
