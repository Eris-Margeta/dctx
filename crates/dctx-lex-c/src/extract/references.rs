// SPDX-License-Identifier: Apache-2.0
//! Call, member and type references inside already-recognised spans.
//! Implements SPEC-MAP-MODE 5.3 calls and uses-type. A call whose callee is a
//! macro defined in this file is abstained. Bare `a.b` and `p->b` reads are
//! kept as convention-level type-name references because the model has no field edge.
use super::{Scope, Walker};
use crate::domain::Precision;
use crate::lexer::{keyword, non_callee, type_word, Kind};
use dctx_map_model::EdgeKind;

impl Walker<'_> {
    pub(crate) fn scan_step(&mut self, index: usize, _end: usize, scope: &Scope) -> usize {
        if self.kind(index) == Some(Kind::Directive) {
            return self.directive(index, scope);
        }
        if self.is(index, "(") {
            self.consider_call(index, scope);
        } else if self.ident(index) {
            self.consider_member(index, scope);
            self.consider_intro_type(index, scope);
        }
        index + 1
    }

    pub(crate) fn emit_types(&mut self, start: usize, end: usize, skip_lo: usize, skip_hi: usize, scope: &Scope) {
        let mut index = start;
        let end = end.min(self.toks.len());
        while index < end && !self.stopped() {
            if index >= skip_lo && index < skip_hi {
                index += 1;
                continue;
            }
            if self.is(index, "<")
                && let Some(after) = self.skip_angles(index, end)
            {
                self.emit_types(index + 1, after.saturating_sub(1), skip_lo, skip_hi, scope);
                index = after;
                continue;
            }
            if self.ident(index) && !Self::is_noise(self.text(index)) {
                let (name, next) = self.render_chain(index);
                let bare_at = next.saturating_sub(1);
                if bare_at >= skip_lo && bare_at < skip_hi {
                    index = next.max(index + 1);
                    continue;
                }
                if self.is(next, "(") || self.bound_name(bare_at) || self.known_macro(self.text(bare_at)) {
                    index = next.max(index + 1);
                    continue;
                }
                if name.contains("::") || type_word(self.text(index)) || self.can_name(index) {
                    self.refer(
                        scope.owner,
                        scope,
                        EdgeKind::UsesType,
                        name,
                        (index, next.saturating_sub(1).max(index)),
                        Precision::Structural,
                    );
                }
                index = next.max(index + 1);
                continue;
            }
            index += 1;
        }
    }

    pub(crate) fn qual_name(&self, start: usize, end: usize) -> Option<String> {
        if !self.ident(start) {
            return None;
        }
        let (name, _) = self.render_chain(start);
        if name.is_empty() || start >= end {
            None
        } else {
            Some(name)
        }
    }

    fn consider_call(&mut self, paren: usize, scope: &Scope) {
        let Some(end_name) = self.callee_end(paren) else {
            return;
        };
        let start = self.chain_start(end_name);
        let (name, next) = self.render_chain(start);
        if next != paren && !(next < paren && self.tokens_are_angles(next, paren)) {
            return;
        }
        let bare = self.text(end_name);
        let qualified = name.contains('.') || name.contains("->") || name.contains("::");
        if !qualified && (keyword(bare) || non_callee(bare) || self.known_macro(bare)) {
            return;
        }
        let close = self.close(paren).unwrap_or(paren);
        self.refer(scope.owner, scope, EdgeKind::Calls, name, (start, close), Precision::Structural);
    }

    fn consider_member(&mut self, index: usize, scope: &Scope) {
        if index > 0 && matches!(self.text(index - 1), "." | "->" | "::") {
            return;
        }
        let (name, next) = self.render_chain(index);
        if self.is(next, "(") || self.is(next, "<") {
            return;
        }
        if !(name.contains('.') || name.contains("->") || name.contains("::")) {
            return;
        }
        let precision = if name.contains('.') || name.contains("->") {
            Precision::Convention
        } else {
            Precision::Structural
        };
        self.refer(
            scope.owner,
            scope,
            EdgeKind::UsesType,
            name,
            (index, next.saturating_sub(1).max(index)),
            precision,
        );
    }

    fn consider_intro_type(&mut self, index: usize, scope: &Scope) {
        if !matches!(self.text(index), "struct" | "class" | "union" | "enum") {
            return;
        }
        let name_at = if matches!(self.text(index + 1), "class" | "struct") && self.is(index, "enum") {
            index + 2
        } else {
            index + 1
        };
        if self.can_name(name_at) && !self.is(name_at + 1, "{") && !self.known_macro(self.text(name_at)) {
            self.refer(
                scope.owner,
                scope,
                EdgeKind::UsesType,
                self.text(name_at).to_string(),
                (name_at, name_at),
                Precision::Structural,
            );
        }
    }

    fn callee_end(&self, paren: usize) -> Option<usize> {
        let prev = paren.checked_sub(1)?;
        if self.ident(prev) {
            return Some(prev);
        }
        if self.is(prev, ">") {
            let lt = self.find_lt_back(prev)?;
            let ident = lt.checked_sub(1)?;
            return self.ident(ident).then_some(ident);
        }
        None
    }

    fn chain_start(&self, mut end_ident: usize) -> usize {
        while end_ident >= 2 {
            let op = end_ident - 1;
            if matches!(self.text(op), "." | "->" | "::") && self.ident(op - 1) {
                end_ident = op - 1;
                continue;
            }
            if self.is(op, "::")
                && self.is(op - 1, ">")
                && let Some(lt) = self.find_lt_back(op - 1)
                && lt > 0
                && self.ident(lt - 1)
            {
                end_ident = lt - 1;
                continue;
            }
            break;
        }
        end_ident
    }

    pub(crate) fn render_chain(&self, start: usize) -> (String, usize) {
        if !self.ident(start) {
            return (String::new(), start);
        }
        let mut name = self.text(start).to_string();
        let mut index = start + 1;
        if self.is(index, "<")
            && let Some(after) = self.skip_angles(index, self.toks.len())
        {
            index = after;
        }
        while index + 1 < self.toks.len()
            && matches!(self.text(index), "." | "->" | "::")
            && self.ident(index + 1)
        {
            name.push_str(self.text(index));
            name.push_str(self.text(index + 1));
            index += 2;
            if self.is(index, "<") {
                if let Some(after) = self.skip_angles(index, self.toks.len()) {
                    index = after;
                } else {
                    break;
                }
            }
        }
        (name, index)
    }

    fn tokens_are_angles(&self, start: usize, paren: usize) -> bool {
        self.is(start, "<") && self.skip_angles(start, paren + 1) == Some(paren)
    }

    fn bound_name(&self, index: usize) -> bool {
        let stops = matches!(self.text(index + 1), "," | ")" | ";" | "=" | "[" | "{" | ":");
        if !stops || index == 0 {
            return false;
        }
        let prev = index - 1;
        self.is(prev, "*")
            || self.is(prev, "&")
            || self.is(prev, "&&")
            || self.is(prev, ">")
            || self.ident(prev)
    }

    fn is_noise(word: &str) -> bool {
        matches!(
            word,
            "const" | "volatile" | "static" | "extern" | "inline" | "constexpr" | "consteval"
                | "virtual" | "explicit" | "friend" | "mutable" | "register" | "typename"
                | "public" | "private" | "protected" | "unsigned" | "signed" | "restrict"
        ) && !type_word(word)
    }
}

#[cfg(test)]
mod tests {
    use crate::{CLexer, Completion};
    use dctx_map_model::EdgeKind;

    #[test]
    fn calls_members_and_macro_calls_follow_the_abstain_rule() {
        let src = b"#define SHOW(x) (x)\nint add(int a, int b);\nint main(void) {\n  struct Box box;\n  box.width = add(1, SHOW(2));\n  return box.width;\n}\n";
        let facts = CLexer::default().extract("r.c", src).unwrap();
        assert_eq!(facts.completion, Completion::Complete, "{:?}", facts.diagnostics);
        let calls: Vec<_> = facts
            .references
            .iter()
            .filter(|r| r.fact.kind == EdgeKind::Calls)
            .map(|r| r.fact.name.as_str())
            .collect();
        assert_eq!(calls, ["add"]);
        let members: Vec<_> = facts
            .references
            .iter()
            .filter(|r| r.fact.name.contains('.'))
            .map(|r| r.fact.name.as_str())
            .collect();
        assert_eq!(members, ["box.width", "box.width"]);
        assert!(facts.references.iter().any(|r| r.fact.kind == EdgeKind::UsesType && r.fact.name == "Box"));
    }
}
