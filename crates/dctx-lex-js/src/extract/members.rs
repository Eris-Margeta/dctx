// SPDX-License-Identifier: Apache-2.0
//! Object literals, parameter lists and interface/type-literal members.
//! Implements SPEC-MAP-MODE 5.1 for `key: () => ..` functions, object methods and
//! TypeScript method signatures; destructuring defaults are walked, never declared.
//! Uses classes (shared `method`), expressions and types.
use super::{Item, Scope, Walker};
use crate::domain::Precision;
use dctx_map_model::{EdgeKind, NodeKind, SymbolKind};

impl Walker<'_> {
    pub fn object_literal(&mut self, begin: usize, end: usize, scope: &Scope) {
        if self.enter(begin) {
            for (start, stop) in self.segments(begin, end) {
                if !self.ok() {
                    break;
                }
                self.object_member(start, stop, scope);
            }
        }
        self.leave();
    }

    fn object_member(&mut self, start: usize, stop: usize, scope: &Scope) {
        if self.is(start, "...") {
            self.expr(start + 1, stop, scope);
            return;
        }
        let mut k = start;
        while matches!(self.text(k), "async" | "get" | "set" | "*")
            && k + 1 < stop
            && (self.key_end(k + 1, stop).is_some() || self.is(k + 1, "*"))
        {
            k += 1;
        }
        let Some(key_end) = self.key_end(k, stop) else {
            self.expr(start, stop, scope);
            return;
        };
        let local = self.key_name(k, key_end);
        if self.is(key_end, "(") || (self.ts && self.is(key_end, "<")) {
            let next = self.method(start, local, key_end, stop, scope, &[]);
            self.expr(next, stop, scope);
            return;
        }
        if !self.is(key_end, ":") || k != start {
            self.expr(start, stop, scope);
            return;
        }
        let value = key_end + 1;
        match self.function_value(value, stop) {
            Some(shape) => {
                let value_end = shape.end;
                let item = Item {
                    kind: NodeKind::Symbol,
                    symbol: SymbolKind::Function,
                    local,
                    first: start,
                    last: value_end - 1,
                    sig_end: self.start_byte(shape.body),
                    body: shape.body_bytes(self, stop),
                    public: false,
                    doc: Some(start),
                };
                let inner = self.item(item, scope);
                self.walk_function(&shape, &inner);
                self.expr(value_end, stop, scope);
            }
            None => self.expr(value, stop, scope),
        }
    }

    /// Formal parameters: modifiers, bindings, optional marks, types and defaults.
    pub fn params(&mut self, begin: usize, end: usize, scope: &Scope) {
        for (start, stop) in self.segments(begin, end) {
            if !self.ok() {
                break;
            }
            let mut k = start;
            while self.is(k, "@") && k < stop {
                let next = self.decorator_end(k, stop);
                self.expr(k, next, scope);
                k = next;
            }
            while self.ts
                && matches!(
                    self.text(k),
                    "public" | "private" | "protected" | "readonly" | "override"
                )
                && (self.ident(k + 1) || self.is(k + 1, "{") || self.is(k + 1, "["))
            {
                k += 1;
            }
            k += usize::from(self.is(k, "..."));
            if self.is(k, "{") || self.is(k, "[") {
                let next = self.after(k, stop);
                self.expr(k, next, scope);
                k = next;
            } else if self.ident(k) {
                k += 1;
            }
            k += usize::from(self.is(k, "?"));
            if self.ts && self.is(k, ":") {
                k = self.scan_type(k + 1, stop, scope);
            }
            if self.is(k, "=") {
                k += 1;
            }
            if k < stop {
                self.expr(k, stop, scope);
            }
        }
    }

    /// Interface bodies and object type literals.
    pub fn type_body(&mut self, begin: usize, end: usize, scope: &Scope) {
        if self.enter(begin) {
            let mut i = begin;
            while i < end && self.ok() {
                i = self.type_member(i, end, scope).max(i + 1);
            }
        }
        self.leave();
    }

    fn type_member(&mut self, i: usize, end: usize, scope: &Scope) -> usize {
        if self.is(i, ";") || self.is(i, ",") {
            return i + 1;
        }
        let mut k = i;
        loop {
            let modifier = matches!(
                self.text(k),
                "readonly"
                    | "static"
                    | "public"
                    | "private"
                    | "protected"
                    | "declare"
                    | "abstract"
                    | "override"
                    | "get"
                    | "set"
                    | "async"
            ) && self.key_end(k + 1, end).is_some();
            let mapped = (self.is(k, "+") || self.is(k, "-")) && self.is(k + 1, "readonly");
            if !(modifier || mapped) {
                break;
            }
            k += 1;
        }
        if self.is(k, "[") && self.ident(k + 1) && (self.is(k + 2, "in") || self.is(k + 2, ":")) {
            let close = self.close(k, end);
            if self.is(k + 2, "in") {
                let name = self.text(k + 1).to_owned();
                let site = (k + 1, k + 1);
                self.refer(
                    scope.owner,
                    scope,
                    EdgeKind::UsesType,
                    name,
                    None,
                    site,
                    site,
                    Precision::Structural,
                );
            }
            let next = self.scan_type(k + 3, close, scope);
            if self.is(next, "as") {
                self.scan_type(next + 1, close, scope);
            }
            let mut m = self.after(k, end);
            while self.is(m, "?") || self.is(m, "-") || self.is(m, "+") {
                m += 1;
            }
            return if self.is(m, ":") {
                self.scan_type(m + 1, end, scope)
            } else {
                m
            };
        }
        if self.is(k, "new") && (self.is(k + 1, "(") || self.is(k + 1, "<")) {
            k += 1;
        }
        if self.is(k, "(") || self.is(k, "<") {
            return self.signature_tail(k, end, scope);
        }
        let Some(key_end) = self.key_end(k, end) else {
            return self.scan_type(i, end, scope);
        };
        let local = self.key_name(k, key_end);
        let m = key_end + usize::from(self.is(key_end, "?"));
        if self.is(m, "(") || self.is(m, "<") {
            return self.method(i, local, m, end, scope, &[]);
        }
        if self.is(m, ":") {
            return self.scan_type(m + 1, end, scope);
        }
        m
    }

    /// Call or construct signature: generics, parameters and return type.
    fn signature_tail(&mut self, mut k: usize, end: usize, scope: &Scope) -> usize {
        if self.is(k, "<") {
            k = self.type_params(k, end, scope);
        }
        if !self.is(k, "(") {
            return k;
        }
        let close = self.close(k, end);
        self.params(k + 1, close, scope);
        let after = self.after(k, end);
        if self.is(after, ":") {
            self.scan_type(after + 1, end, scope)
        } else {
            after
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::JsLexer;

    fn names(path: &str, source: &str) -> Vec<String> {
        let facts = JsLexer::default().extract(path, source.as_bytes()).unwrap();
        facts
            .nodes
            .iter()
            .skip(1)
            .map(|n| n.fact.name.clone())
            .collect()
    }

    #[test]
    fn object_members_name_functions_and_methods_only() {
        let source = "const api = { a: () => 1, 'b-c': function () {}, d() {}, async *e() {}, f: 1, [k]: x => x, ...rest };\nfunction g({ h = () => 1 }) {}";
        assert_eq!(names("o.js", source), ["a", "b-c", "d", "e", "[k]", "g"]);
    }

    #[test]
    fn interface_and_type_literal_methods_are_signatures() {
        let source = "interface I { run(x: A): B; readonly v: C; new (): I; [k: string]: D; get w(): E }\ntype T = { go?(): void };";
        assert_eq!(names("i.ts", source), ["I", "I::run", "I::w", "T", "T::go"]);
    }
}
