//! Classes: declarations, expressions, heritage and members (methods, fields, signatures).
//! Implements SPEC-MAP-MODE 5.1 class/method facts with dctx-map-parse spans: JS method
//! spans include decorators, TS spans start at modifiers; arrow-valued fields are methods.
//! Uses statements, expressions and types; also serves object-literal methods.
use super::statements::Decl;
use super::{Item, Scope, Walker};
use crate::domain::Precision;
use crate::lexer::Kind;
use dctx_map_model::{EdgeKind, NodeKind, SymbolKind};

const MODIFIERS: [&str; 13] = [
    "static", "public", "private", "protected", "readonly", "abstract", "override", "declare",
    "accessor", "async", "get", "set", "*",
];

impl Walker<'_> {
    /// (name token, index after the name, body `{`) of a class at `k`.
    fn class_parts(&self, k: usize, end: usize) -> Option<(Option<usize>, usize, usize)> {
        let mut j = k + usize::from(self.is(k, "abstract")) + 1;
        let name = (self.ident(j) && !matches!(self.text(j), "extends" | "implements"))
            .then_some(j);
        j += usize::from(name.is_some());
        let after_name = j;
        while j < end {
            match self.text(j) {
                "{" if self.kind(j) == Some(Kind::Punct) => return Some((name, after_name, j)),
                "(" | "[" if self.kind(j) == Some(Kind::Punct) => j = self.after(j, end),
                ";" => return None,
                _ => j += 1,
            }
        }
        None
    }

    pub fn class_declaration(
        &mut self,
        k: usize,
        end: usize,
        scope: &Scope,
        decl: Decl,
    ) -> Option<usize> {
        let (name, after_name, body) = self.class_parts(k, end)?;
        let local = match (name, decl.default) {
            (Some(n), _) => self.text(n).to_owned(),
            (None, true) => "default".to_owned(),
            (None, false) => return None,
        };
        let close = self.close(body, end);
        let last = self.last(close, end);
        let first = decl.first.unwrap_or(k);
        let item = Item {
            kind: NodeKind::Symbol,
            symbol: SymbolKind::Class,
            local,
            first,
            last,
            sig_end: self.start_byte(body),
            body: (self.start_byte(body), self.end_byte(last)),
            public: decl.public,
            doc: decl.anchor,
        };
        let inner = self.item(item, scope);
        if first < k {
            self.expr(first, k, &inner);
        }
        self.heritage(after_name, body, &inner);
        self.class_body(body + 1, close, &inner);
        Some((close + 1).min(end))
    }

    pub fn class_expression(&mut self, i: usize, end: usize, scope: &Scope) -> usize {
        let Some((name, after_name, body)) = self.class_parts(i, end) else {
            return i + 1;
        };
        if self.ts
            && let Some(n) = name
        {
            let name = self.text(n).to_owned();
            self.refer(scope.owner, scope, EdgeKind::UsesType, name, None, (n, n), (n, n), Precision::Structural);
        }
        let close = self.close(body, end);
        self.heritage(after_name, body, scope);
        self.class_body(body + 1, close, scope);
        (close + 1).min(end)
    }

    fn heritage(&mut self, begin: usize, body: usize, scope: &Scope) {
        let mut j = begin;
        if self.ts && self.is(j, "<") {
            j = self.type_params(j, body, scope);
        }
        let mut implements = false;
        while j < body && self.ok() {
            if self.is(j, "implements") || self.is(j, "extends") || self.is(j, ",") {
                implements |= self.is(j, "implements");
                j += 1;
            } else if implements {
                j = self.scan_type(j, body, scope).max(j + 1);
            } else if self.ts && self.is(j, "<") {
                j = self.type_args(j, body, scope).max(j + 1);
            } else {
                j = self.expr_token(j, body, scope).max(j + 1);
            }
        }
    }

    pub fn class_body(&mut self, begin: usize, end: usize, scope: &Scope) {
        if self.enter(begin) {
            let mut i = begin;
            while i < end && self.ok() {
                i = self.class_member(i, end, scope).max(i + 1);
            }
        }
        self.leave();
    }

    fn class_member(&mut self, mut i: usize, end: usize, scope: &Scope) -> usize {
        if self.is(i, ";") || self.is(i, ",") {
            return i + 1;
        }
        let first = i;
        let mut decorators = Vec::new();
        while self.is(i, "@") && i < end {
            let next = self.decorator_end(i, end);
            decorators.push((i, next));
            i = next;
        }
        if self.is(i, "static") && self.is(i + 1, "{") {
            self.decorate(&decorators, scope);
            let close = self.close(i + 1, end);
            self.statements(i + 2, close, scope);
            return (close + 1).min(end);
        }
        let modifiers = i;
        while self.member_modifier(i) {
            i += 1;
        }
        let node_first = if self.ts { modifiers } else { first };
        if self.ts && self.is(i, "[") && self.ident(i + 1) && self.is(i + 2, ":") {
            self.decorate(&decorators, scope);
            let close = self.close(i, end);
            self.params(i + 1, close, scope);
            let after = self.after(i, end);
            return if self.is(after, ":") {
                self.scan_type(after + 1, end, scope)
            } else {
                after
            };
        }
        let Some(key_end) = self.key_end(i, end) else {
            self.decorate(&decorators, scope);
            return self.expr_token(i, end, scope);
        };
        let local = self.key_name(i, key_end);
        let k = key_end + usize::from(self.is(key_end, "?") || self.is(key_end, "!"));
        if self.is(k, "(") || (self.ts && self.is(k, "<")) {
            return self.method(node_first, local, k, end, scope, &decorators);
        }
        self.field(node_first, local, k, end, scope, &decorators)
    }

    fn decorate(&mut self, decorators: &[(usize, usize)], scope: &Scope) {
        for &(start, stop) in decorators {
            self.expr(start, stop, scope);
        }
    }

    /// Method definition or signature whose parameters (or generics) start at `k`.
    pub fn method(
        &mut self,
        first: usize,
        local: String,
        mut k: usize,
        end: usize,
        scope: &Scope,
        decorators: &[(usize, usize)],
    ) -> usize {
        let generics = self.is(k, "<").then_some(k);
        if generics.is_some() {
            k = self.type_params_end(k, end);
        }
        if !self.is(k, "(") {
            self.decorate(decorators, scope);
            return k;
        }
        let params_close = self.close(k, end);
        let mut j = (params_close + 1).min(end);
        let returns = (self.ts && self.is(j, ":")).then_some(j);
        if returns.is_some() {
            j = self.type_end(j + 1, end);
        }
        let block = self.is(j, "{");
        let body_close = if block { self.close(j, end) } else { j };
        let last = if block {
            self.last(body_close, end)
        } else {
            j.saturating_sub(1).max(k)
        };
        let item = Item {
            kind: NodeKind::Symbol,
            symbol: SymbolKind::Method,
            local,
            first,
            last,
            sig_end: if block {
                self.start_byte(j)
            } else {
                self.end_byte(last)
            },
            body: if block {
                (self.start_byte(j), self.end_byte(last))
            } else {
                (self.start_byte(first), self.end_byte(last))
            },
            public: false,
            doc: Some(first),
        };
        let inner = self.item(item, scope);
        self.decorate(decorators, if self.ts { scope } else { &inner });
        if let Some(open) = generics {
            self.type_params(open, k, &inner);
        }
        self.params(k + 1, params_close, &inner);
        if let Some(colon) = returns {
            self.scan_type(colon + 1, end, &inner);
        }
        if block {
            self.statements(j + 1, body_close, &inner);
            (body_close + 1).min(end)
        } else {
            j
        }
    }

    fn field(
        &mut self,
        first: usize,
        local: String,
        k: usize,
        end: usize,
        scope: &Scope,
        decorators: &[(usize, usize)],
    ) -> usize {
        let mut j = k;
        let annotation = (self.ts && self.is(j, ":")).then_some(j);
        if annotation.is_some() {
            j = self.type_end(j + 1, end);
        }
        let mut owner = scope.clone();
        if self.is(j, "=") {
            let value = j + 1;
            if let Some(shape) = self.function_value(value, end) {
                let value_end = shape.end;
                let item = Item {
                    kind: NodeKind::Symbol,
                    symbol: SymbolKind::Method,
                    local,
                    first,
                    last: value_end - 1,
                    sig_end: self.start_byte(shape.body),
                    body: shape.body_bytes(self, end),
                    public: false,
                    doc: Some(first),
                };
                owner = self.item(item, scope);
                self.decorate(decorators, if self.ts { scope } else { &owner });
                if let Some(colon) = annotation {
                    self.scan_type(colon + 1, end, &owner);
                }
                self.walk_function(&shape, &owner);
                return value_end + usize::from(self.is(value_end, ";"));
            }
            let value_end = self.expr_end(value, end);
            self.expr(value, value_end, scope);
            j = value_end;
        }
        self.decorate(decorators, &owner);
        if let Some(colon) = annotation {
            self.scan_type(colon + 1, end, &owner);
        }
        j + usize::from(self.is(j, ";"))
    }

    fn member_modifier(&self, i: usize) -> bool {
        let word = self.text(i);
        if !matches!(self.kind(i), Some(Kind::Ident | Kind::Punct)) || !MODIFIERS.contains(&word) {
            return false;
        }
        if word == "async" && self.nl(i + 1) {
            return false;
        }
        let n = i + 1;
        matches!(
            self.kind(n),
            Some(Kind::Ident | Kind::Str | Kind::Number | Kind::Private)
        ) || self.is(n, "[")
            || self.is(n, "*")
    }

    /// Index after a property key at `i`, if one starts there.
    pub fn key_end(&self, i: usize, end: usize) -> Option<usize> {
        match self.kind(i)? {
            Kind::Ident | Kind::Str | Kind::Number | Kind::Private => Some(i + 1),
            Kind::Punct if self.is(i, "[") => Some(self.after(i, end)),
            _ => None,
        }
    }

    pub fn key_name(&self, i: usize, key_end: usize) -> String {
        if self.kind(i) == Some(Kind::Str) {
            return self.content(i);
        }
        let last = key_end.saturating_sub(1).max(i);
        self.src[self.start_byte(i) as usize..self.end_byte(last) as usize].to_owned()
    }
}

#[cfg(test)]
mod tests {
    use crate::JsLexer;

    fn names(path: &str, source: &str) -> Vec<(String, u32)> {
        let facts = JsLexer::default().extract(path, source.as_bytes()).unwrap();
        facts
            .nodes
            .iter()
            .skip(1)
            .map(|n| (n.fact.name.clone(), n.fact.lines[0]))
            .collect()
    }

    #[test]
    fn members_modifiers_and_fields_become_methods() {
        let source = "class A extends mix(B) {\n  static async *gen() {}\n  get value() { return 1 }\n  run = () => go()\n  plain = 1\n  'quoted'() {}\n  static { boot() }\n}";
        let found = names("a.js", source);
        assert_eq!(
            found,
            [
                ("A".into(), 1),
                ("A::gen".into(), 2),
                ("A::value".into(), 3),
                ("A::run".into(), 4),
                ("A::quoted".into(), 6)
            ]
        );
    }

    #[test]
    fn typescript_signatures_and_abstract_members() {
        let source = "export abstract class S<T> implements I<T> {\n  abstract run(x: T): void;\n  private load(): Promise<T> { return f<T>(); }\n  [key: string]: unknown;\n}";
        let found = names("s.ts", source);
        assert_eq!(found, [("S".into(), 1), ("S::run".into(), 2), ("S::load".into(), 3)]);
    }
}
