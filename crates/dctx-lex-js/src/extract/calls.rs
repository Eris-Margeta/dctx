// SPDX-License-Identifier: Apache-2.0
//! Call sites: callee spelling, node:test/vitest blocks and `CommonJS` `require` imports.
//! Implements SPEC-MAP-MODE 5.3 calls/tests with name confidence only; dynamic or
//! computed callees are abstained rather than guessed (PARSERS-v1 JavaScript row).
//! Uses expressions for arguments; called from `Walker::paren`.
use super::{Item, Scope, Walker};
use crate::domain::{Precision, compact};
use crate::lexer::{Kind, NONE};
use dctx_map_model::{EdgeKind, NodeKind, SymbolKind};

impl Walker<'_> {
    /// Whether the `(` at `i` applies a preceding expression.
    pub fn call_position(&self, i: usize) -> bool {
        let Some(p) = i.checked_sub(1) else {
            return false;
        };
        match self.kind(p) {
            Some(Kind::Ident) => {
                self.word(p)
                    || matches!(self.text(p), "super" | "import" | "this")
                    || (p > 0 && (self.is(p - 1, ".") || self.is(p - 1, "?.")))
            }
            Some(Kind::Private | Kind::Template | Kind::TemplateTail) => true,
            Some(Kind::Punct) => match self.text(p) {
                "]" | "?." => true,
                ")" => !self.control_paren(p),
                ">" => self.angles.contains_key(&p),
                "!" => self.ts && p > 0 && self.ends_expr(p - 1),
                _ => false,
            },
            _ => false,
        }
    }

    fn control_paren(&self, close: usize) -> bool {
        let open = self.partner[close];
        open != NONE
            && (open as usize) < close
            && open > 0
            && matches!(
                self.text(open as usize - 1),
                "if" | "for" | "while" | "with" | "switch" | "catch"
            )
    }

    pub fn call(&mut self, i: usize, close: usize, end: usize, scope: &Scope) -> usize {
        let last = Self::last(close, end);
        if let Some((start, callee_last, member)) = self.callee(i) {
            let raw = &self.src[self.start_byte(start) as usize..self.end_byte(callee_last) as usize];
            let literal = self.kind(i + 1) == Some(Kind::Str)
                && (self.is(i + 2, ",") || self.is(i + 2, ")"));
            if !member && raw == "require" && literal {
                self.require(start, i, close, last, scope);
            } else if let Some(name) = literal.then(|| test_name(raw, self.text(i + 1))).flatten() {
                let item = Item {
                    kind: NodeKind::Test,
                    symbol: SymbolKind::Function,
                    local: name,
                    first: start,
                    last,
                    sig_end: self.end_byte(last),
                    body: (self.start_byte(start), self.end_byte(last)),
                    public: false,
                    doc: None,
                };
                let inner = self.item(item, scope);
                self.expr(i + 1, close, &inner);
                return (close + 1).min(end);
            } else {
                let name = compact(raw);
                let site = (start, last);
                self.refer(
                    scope.owner,
                    scope,
                    EdgeKind::Calls,
                    name.clone(),
                    None,
                    site,
                    site,
                    Precision::Structural,
                );
                if scope.test {
                    self.refer(
                        scope.owner,
                        scope,
                        EdgeKind::Tests,
                        name,
                        None,
                        site,
                        site,
                        Precision::Convention,
                    );
                }
            }
        }
        self.expr(i + 1, close, scope);
        (close + 1).min(end)
    }

    /// (first token, last token, is member) of an identifier or member callee.
    fn callee(&self, i: usize) -> Option<(usize, usize, bool)> {
        let mut q = i.checked_sub(1)?;
        if self.is(q, "?.") {
            q = q.checked_sub(1)?;
        }
        if self.is(q, ">") {
            q = self.angles.get(&q)?.checked_sub(1)?;
        }
        if !matches!(self.kind(q), Some(Kind::Ident | Kind::Private)) {
            return None;
        }
        if q >= 2 && (self.is(q - 1, ".") || self.is(q - 1, "?.")) {
            return Some((self.chain_start(q - 2)?, q, true));
        }
        if !self.word(q) || (q > 0 && self.is(q - 1, "new")) {
            return None;
        }
        Some((q, q, false))
    }

    /// First token of the object expression ending at `q`; `None` abstains.
    fn chain_start(&self, mut q: usize) -> Option<usize> {
        let mut applied = false;
        loop {
            let start = match self.kind(q)? {
                Kind::Ident | Kind::Private | Kind::Number | Kind::Str | Kind::Regex | Kind::Template => q,
                Kind::Punct => match self.text(q) {
                    ")" | "]" => {
                        let open = self.partner[q];
                        if open == NONE || open as usize >= q {
                            return None;
                        }
                        open as usize
                    }
                    ">" => {
                        q = self.angles.get(&q)?.checked_sub(1)?;
                        continue;
                    }
                    "!" if self.ts => {
                        q = q.checked_sub(1)?;
                        continue;
                    }
                    _ => return None,
                },
                _ => return None,
            };
            let Some(p) = start.checked_sub(1) else {
                return Some(start);
            };
            let bracket = start != q;
            let property = matches!(self.kind(start), Some(Kind::Ident | Kind::Private));
            if property && (self.is(p, ".") || self.is(p, "?.")) {
                q = p.checked_sub(1)?;
                continue;
            }
            if (bracket || self.kind(q) == Some(Kind::Template)) && self.postfix_base(p) {
                applied |= self.is(start, "(");
                q = p;
                continue;
            }
            if self.is(p, "new") {
                return applied.then_some(p);
            }
            return Some(start);
        }
    }

    /// Whether token `p` can end an expression that a call, index or tag applies to.
    fn postfix_base(&self, p: usize) -> bool {
        match self.kind(p) {
            Some(Kind::Ident) => self.word(p) || matches!(self.text(p), "this" | "super"),
            Some(Kind::Private | Kind::Str | Kind::Template | Kind::TemplateTail) => true,
            Some(Kind::Punct) => match self.text(p) {
                "]" => true,
                ")" => !self.control_paren(p),
                ">" => self.angles.contains_key(&p),
                "!" => self.ts,
                _ => false,
            },
            _ => false,
        }
    }

    /// `require('m')`: binds `const x`, `const { a, b: c }` or nothing.
    fn require(&mut self, start: usize, open: usize, close: usize, last: usize, scope: &Scope) {
        let module = self.content(open + 1);
        let key = (start, last);
        let binding = self
            .binding
            .filter(|b| b.value == start && b.end == close + 1);
        let convention = Precision::Convention;
        match binding {
            Some(b) if self.is(b.pattern, "{") => {
                let pattern_close = self.close(b.pattern, b.value);
                for (s, e) in self.segments(b.pattern + 1, pattern_close) {
                    let keyed = matches!(
                        self.kind(s),
                        Some(Kind::Ident | Kind::Str | Kind::Number)
                    ) && self.is(s + 1, ":")
                        && e > s + 2;
                    if keyed {
                        let alias = self.src
                            [self.start_byte(s + 2) as usize..self.end_byte(e - 1) as usize]
                            .to_owned();
                        let name = format!("{module}::{}", self.text(s));
                        self.refer(0, scope, EdgeKind::Imports, name, Some(alias), (s, e - 1), key, convention);
                    } else if e == s + 1 && self.ident(s) {
                        let name = format!("{module}::{}", self.text(s));
                        self.refer(0, scope, EdgeKind::Imports, name, None, (s, s), key, convention);
                    }
                }
            }
            Some(b) => {
                let pattern_last = if self.ident(b.pattern) {
                    b.pattern
                } else {
                    self.close(b.pattern, b.value)
                };
                let alias = self.src
                    [self.start_byte(b.pattern) as usize..self.end_byte(pattern_last) as usize]
                    .to_owned();
                self.refer(0, scope, EdgeKind::Imports, module, Some(alias), key, key, convention);
            }
            None => self.refer(0, scope, EdgeKind::Imports, module, None, key, key, convention),
        }
    }

    /// Text between the quotes of a string token.
    pub fn content(&self, i: usize) -> String {
        let text = self.text(i);
        text.get(1..text.len().saturating_sub(1))
            .unwrap_or_default()
            .to_owned()
    }
}

/// node:test and vitest block names: `test|it|describe` with modifier chains.
fn test_name(callee: &str, literal: &str) -> Option<String> {
    let mut parts = callee.split('.');
    if !matches!(parts.next(), Some("test" | "it" | "describe"))
        || !parts.all(|part| matches!(part, "only" | "skip" | "todo" | "concurrent"))
    {
        return None;
    }
    literal.get(1..literal.len().checked_sub(1)?).map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::test_name;
    use crate::JsLexer;
    use dctx_map_model::EdgeKind;

    fn refs(source: &str) -> Vec<(EdgeKind, String, Option<String>)> {
        let facts = JsLexer::default().extract("x.js", source.as_bytes()).unwrap();
        facts
            .references
            .iter()
            .map(|r| (r.fact.kind, r.fact.name.clone(), r.fact.alias.clone()))
            .collect()
    }

    #[test]
    fn member_chains_new_and_dynamic_callees_follow_the_oracle() {
        let found = refs("a.b(c).d(); new Foo(x).bar(); new A.B(); (f)(); xs[0](); super.m();");
        let calls: Vec<_> = found.iter().map(|r| r.1.as_str()).collect();
        assert_eq!(calls, ["a.b(c).d", "a.b", "newFoo(x).bar", "super.m"]);
    }

    #[test]
    fn require_bindings_and_test_modifiers() {
        let found = refs("const { a, b: c } = require('m'); const d = require('n').x; it.skip('t', () => g());");
        assert_eq!(
            found,
            [
                (EdgeKind::Imports, "m::a".into(), None),
                (EdgeKind::Imports, "m::b".into(), Some("c".into())),
                (EdgeKind::Imports, "n".into(), None),
                (EdgeKind::Calls, "g".into(), None),
                (EdgeKind::Tests, "g".into(), None),
            ]
        );
        assert_eq!(test_name("describe.only", "'x'"), Some("x".into()));
        assert_eq!(test_name("describe.each", "'x'"), None);
        assert_eq!(test_name("foo.it", "'x'"), None);
    }
}
