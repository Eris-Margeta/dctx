//! Reference scan for calls, type uses, class literals and anonymous classes.
//! Implements SPEC-MAP-MODE 5.3 for Java calls, implements and uses-type.
//! Uses declaration scopes from the walker; bodies stay inside their owner.
//! Field reads and `::` method references are left unresolved on purpose.
use super::{is_modifier_word, is_non_callee, is_primitive, Prefix, Scope, ScopeKind, SymbolDraft, Walker};
use crate::domain::Precision;
use dctx_map_model::{EdgeKind, NodeKind, SymbolKind};

impl Walker<'_> {
    pub(crate) fn scan_block(&mut self, begin: usize, end: usize, scope: &Scope) {
        if !self.enter(begin.min(end)) {
            return;
        }
        let mut index = begin;
        while index < end && self.ok() {
            let next = self.scan_one(index, end, scope);
            index = if next > index { next } else { index + 1 };
        }
        self.leave();
    }

    pub(crate) fn scan_initializer(&mut self, mut index: usize, end: usize, scope: &Scope) -> usize {
        while index < end && self.ok() {
            if self.is_punct(index, ",") || self.is_punct(index, ";") {
                return index;
            }
            let next = self.scan_one(index, end, scope);
            index = if next > index { next } else { index + 1 };
        }
        index
    }

    fn scan_one(&mut self, index: usize, end: usize, scope: &Scope) -> usize {
        if let Some(kind) = self.local_type(index, end) {
            let prefix = self.prefix(index, end);
            return self.type_decl(index, &prefix, kind, end, scope);
        }
        if self.is_word(index, "new") {
            return self.anonymous_or_ctor(index, end, scope);
        }
        if self.is_punct(index, "(") {
            self.emit_paren_call(index, scope);
            return self.scan_group(index, end, scope);
        }
        if self.is_punct(index, "{") || self.is_punct(index, "[") {
            return self.scan_group(index, end, scope);
        }
        self.class_literal(index, end, scope).unwrap_or(index + 1)
    }

    fn local_type(&self, index: usize, end: usize) -> Option<super::DeclKind> {
        let prefix = self.prefix(index, end);
        let kind = self.decl_kind(prefix.next)?;
        self.word(self.name_at(prefix.next, kind)).then_some(kind)
    }

    fn emit_paren_call(&mut self, paren: usize, scope: &Scope) {
        let Some((from, name)) = self.callee(paren) else {
            return;
        };
        let to = self.close(paren).unwrap_or(paren);
        self.emit_call(scope, name, from, to);
    }

    fn callee(&self, paren: usize) -> Option<(usize, String)> {
        let prev = paren.checked_sub(1)?;
        if !self.word(prev) {
            return None;
        }
        let dotted = prev > 0 && self.is_punct(prev - 1, ".");
        if is_non_callee(self.text(prev)) && !dotted {
            return None;
        }
        let mut start = prev;
        while start >= 2 && self.is_punct(start - 1, ".") && self.word(start - 2) {
            start -= 2;
        }
        let name = self.qualified(start, prev);
        (!name.is_empty()).then_some((start, name))
    }

    fn emit_call(&mut self, scope: &Scope, name: String, from: usize, to: usize) {
        self.refer(
            scope.owner,
            scope,
            EdgeKind::Calls,
            name.clone(),
            None,
            from,
            to,
            Precision::Structural,
        );
        if scope.in_test {
            self.refer(
                scope.owner,
                scope,
                EdgeKind::Tests,
                name,
                None,
                from,
                to,
                Precision::Convention,
            );
        }
    }

    fn anonymous_or_ctor(&mut self, index: usize, end: usize, scope: &Scope) -> usize {
        let Some(found) = self.new_type(index, end) else {
            return index + 1;
        };
        if found.array {
            return index + 1;
        }
        let Some(paren) = found.paren else {
            return found.after.max(index + 1);
        };
        let close = self.close(paren);
        let to = close.unwrap_or(paren);
        self.emit_call(scope, format!("new {}", found.qualified), index, to);
        let cursor = if let Some(close) = close {
            self.scan_block(paren + 1, close.min(end), scope);
            close + 1
        } else {
            return (paren + 1).min(end);
        };
        if self.is_punct(cursor, "{") {
            return self.anonymous_body(index, cursor, end, scope, &found, close);
        }
        cursor.min(end)
    }

    fn new_type(&mut self, index: usize, end: usize) -> Option<NewType> {
        let mut cursor = index + 1;
        if self.is_punct(cursor, "<") {
            cursor = self.skip_angles(cursor, end);
        }
        if !self.ok() || !self.word(cursor) {
            return None;
        }
        let type_start = cursor;
        cursor += 1;
        while cursor + 1 < end && self.is_punct(cursor, ".") && self.word(cursor + 1) {
            cursor += 2;
        }
        let type_end = cursor - 1;
        if self.is_punct(cursor, "<") {
            cursor = self.skip_angles(cursor, end);
        }
        if !self.ok() {
            return None;
        }
        if self.is_punct(cursor, "[") {
            return Some(NewType::array());
        }
        Some(NewType {
            qualified: self.qualified(type_start, type_end),
            simple: self.text(type_end).to_string(),
            paren: self.is_punct(cursor, "(").then_some(cursor),
            after: cursor,
            array: false,
        })
    }

    fn anonymous_body(
        &mut self,
        new_at: usize,
        brace: usize,
        end: usize,
        scope: &Scope,
        found: &NewType,
        paren_close: Option<usize>,
    ) -> usize {
        let Some(close) = self.close(brace) else {
            return brace + 1;
        };
        let local = format!("new {}", found.qualified);
        let sig_end = paren_close.map(|token| self.end_byte(token)).unwrap_or_else(|| self.start_byte(brace));
        if let Some(child) = self.emit(
            SymbolDraft {
                node_kind: NodeKind::Symbol,
                symbol: SymbolKind::Class,
                local: &local,
                simple: &found.simple,
                scope_kind: ScopeKind::Class,
                type_params: Vec::new(),
                first: new_at,
                last: close.min(end.saturating_sub(1)),
                sig_end,
                body: (self.start_byte(brace), self.end_byte(close)),
                is_public: false,
                precision: Precision::Structural,
            },
            scope,
        ) {
            self.class_body(brace + 1, close.min(end), &child, false);
        }
        (close + 1).min(end)
    }

    fn class_literal(&mut self, index: usize, end: usize, scope: &Scope) -> Option<usize> {
        if !self.word(index) {
            return None;
        }
        let mut cursor = index;
        loop {
            let class_at = cursor + 2;
            if self.is_punct(cursor + 1, ".")
                && self.is_word(class_at, "class")
                && !self.is_punct(class_at + 1, ".")
            {
                self.emit_class_literal(index, cursor, scope);
                return Some((class_at + 1).min(end));
            }
            if self.is_punct(cursor + 1, ".") && self.word(cursor + 2) {
                cursor += 2;
                continue;
            }
            return None;
        }
    }

    fn emit_class_literal(&mut self, from: usize, to: usize, scope: &Scope) {
        let name = self.type_words(from, to);
        if skips_type_word(&name) || !self.keep_type(&name, scope, &[]) {
            return;
        }
        self.refer(
            scope.owner,
            scope,
            EdgeKind::UsesType,
            name,
            None,
            from,
            to,
            Precision::Structural,
        );
    }

    pub(crate) fn scan_annotation_list(&mut self, prefix: &Prefix, scope: &Scope) {
        for ann in &prefix.annotations {
            if self.keep_type(&ann.name, scope, &[]) {
                self.refer(
                    scope.owner,
                    scope,
                    EdgeKind::UsesType,
                    ann.name.clone(),
                    None,
                    ann.at,
                    ann.at,
                    Precision::Structural,
                );
            }
            if let Some(open) = ann.args
                && let Some(close) = self.close(open)
            {
                self.scan_block(open + 1, close, scope);
            }
        }
    }

    pub(crate) fn type_uses(&mut self, from: usize, to: usize, scope: &Scope) {
        self.type_uses_skip(from, to, scope, &[]);
    }

    pub(crate) fn type_uses_skip(&mut self, from: usize, to: usize, scope: &Scope, extra: &[String]) {
        let mut cursor = from;
        let mut part = None;
        while cursor < to && self.ok() {
            if self.is_annotation(cursor) {
                self.flush_type(&mut part, scope, extra);
                self.use_annotation(cursor, scope, extra);
                let next = self.skip_annotation_tokens(cursor, to);
                cursor = if next > cursor { next } else { cursor + 1 };
                continue;
            }
            if self.word(cursor) {
                cursor = self.take_type_word(cursor, &mut part, scope, extra);
                continue;
            }
            if !self.is_punct(cursor, ".") {
                self.flush_type(&mut part, scope, extra);
            }
            cursor += 1;
        }
        self.flush_type(&mut part, scope, extra);
    }

    fn take_type_word(
        &mut self,
        cursor: usize,
        part: &mut Option<(usize, usize)>,
        scope: &Scope,
        extra: &[String],
    ) -> usize {
        if skips_type_word(self.text(cursor)) {
            self.flush_type(part, scope, extra);
            return cursor + 1;
        }
        if part.is_some() && cursor > 0 && self.is_punct(cursor - 1, ".") {
            if let Some(span) = part.as_mut() {
                span.1 = cursor;
            }
            return cursor + 1;
        }
        self.flush_type(part, scope, extra);
        *part = Some((cursor, cursor));
        cursor + 1
    }

    fn use_annotation(&mut self, at: usize, scope: &Scope, extra: &[String]) {
        let name = self.annotation_name(at);
        if skips_type_word(&name) || !self.keep_type(&name, scope, extra) {
            return;
        }
        self.refer(
            scope.owner,
            scope,
            EdgeKind::UsesType,
            name,
            None,
            at,
            at,
            Precision::Structural,
        );
    }

    fn flush_type(&mut self, part: &mut Option<(usize, usize)>, scope: &Scope, extra: &[String]) {
        let Some((from, to)) = part.take() else {
            return;
        };
        let name = self.type_words(from, to);
        if !self.keep_type(&name, scope, extra) {
            return;
        }
        self.refer(
            scope.owner,
            scope,
            EdgeKind::UsesType,
            name,
            None,
            from,
            to,
            Precision::Structural,
        );
    }

    fn type_words(&self, from: usize, to: usize) -> String {
        let mut name = String::new();
        let mut cursor = from;
        while cursor <= to && cursor < self.toks.len() {
            if self.word(cursor) {
                if !name.is_empty() {
                    name.push('.');
                }
                name.push_str(self.text(cursor));
            }
            cursor += 1;
        }
        name
    }

    fn keep_type(&self, name: &str, scope: &Scope, extra: &[String]) -> bool {
        !name.is_empty()
            && !scope.type_params.iter().any(|param| param == name)
            && !extra.iter().any(|param| param == name)
    }
}

struct NewType {
    qualified: String,
    simple: String,
    paren: Option<usize>,
    after: usize,
    array: bool,
}

impl NewType {
    fn array() -> Self {
        Self {
            qualified: String::new(),
            simple: String::new(),
            paren: None,
            after: 0,
            array: true,
        }
    }
}

fn skips_type_word(word: &str) -> bool {
    is_primitive(word)
        || is_modifier_word(word)
        || is_non_callee(word)
        || matches!(
            word,
            "var" | "true" | "false" | "null" | "this" | "super" | "extends" | "implements"
                | "permits" | "throws"
        )
}

#[cfg(test)]
mod tests {
    use crate::{Completion, JavaLexer, Precision};

    #[test]
    fn calls_keep_identifier_chains_and_skip_reads() {
        let source = "class A { A() { this(1); } void m() { super.run(); a.b(); c.d; e::f; } }";
        let facts = JavaLexer::default()
            .extract("A.java", source.as_bytes())
            .expect("extract");
        assert_eq!(facts.completion, Completion::Complete);
        let calls: Vec<_> = facts
            .references
            .iter()
            .filter(|reference| reference.fact.kind.as_str() == "calls")
            .map(|reference| reference.fact.name.as_str())
            .collect();
        assert_eq!(calls, ["this", "super.run", "a.b"]);
        assert!(facts.references.iter().all(|reference| reference.precision == Precision::Structural));
        assert!(facts.references.iter().all(|reference| reference.fact.name != "c.d"));
        assert!(facts.references.iter().all(|reference| !reference.fact.name.contains("::")));
    }
}
