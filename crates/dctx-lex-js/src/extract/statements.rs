//! Statement lists: boundaries (including ASI), declarations, functions and variables.
//! Implements SPEC-MAP-MODE 5.1 declarations with dctx-map-parse naming and visibility:
//! `export` makes a declaration public, `declare` does not, `export default` names `default`.
//! Uses expressions, classes, typedecl and modules; called for every block body.
use super::{Binding, Item, Scope, Walker};
use crate::lexer::{Kind, NONE};
use dctx_map_model::{NodeKind, SymbolKind};

/// How a declaration was introduced.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Decl {
    pub public: bool,
    pub anchor: Option<usize>,
    pub first: Option<usize>,
    pub default: bool,
}

impl Walker<'_> {
    pub fn statements(&mut self, begin: usize, end: usize, scope: &Scope) {
        if self.enter(begin) {
            let mut i = begin;
            let mut head = begin;
            while i < end && self.ok() {
                if i == begin || self.boundary(i, head) {
                    head = i;
                    if self.is(i, "{") {
                        let close = self.close(i, end);
                        self.statements(i + 1, close, scope);
                        i = (close + 1).min(end);
                        continue;
                    }
                    if let Some(next) = self.declaration(i, end, scope) {
                        i = next.max(i + 1);
                        continue;
                    }
                }
                i = self.expr_token(i, end, scope).max(i + 1);
            }
        }
        self.leave();
    }

    /// Whether token `i` starts a statement, given the current statement's `head`.
    fn boundary(&self, i: usize, head: usize) -> bool {
        let previous = i - 1;
        let explicit = match (self.kind(previous), self.text(previous)) {
            (Some(Kind::Punct), ":") => {
                self.is(head, "case")
                    || self.is(head, "default")
                    || (previous == head + 1 && self.word(head))
            }
            (Some(Kind::Punct), ")") => {
                let open = self.partner[previous];
                open != NONE
                    && (open as usize) < previous
                    && open > 0
                    && matches!(
                        self.text(open as usize - 1),
                        "if" | "for" | "while" | "with" | "catch" | "switch"
                    )
            }
            (Some(Kind::Punct), ";" | "}") | (Some(Kind::Ident), "else" | "do" | "try" | "catch" | "finally") => true,
            _ => false,
        };
        explicit || self.asi(i)
    }

    fn declaration(&mut self, i: usize, end: usize, scope: &Scope) -> Option<usize> {
        match self.text(i) {
            "@" if self.kind(i) == Some(Kind::Punct) => {
                let mut k = i;
                while self.is(k, "@") && k < end {
                    k = self.decorator_end(k, end);
                }
                let decl = Decl {
                    anchor: Some(i),
                    first: Some(i),
                    ..Decl::default()
                };
                if self.is(k, "export") {
                    self.expr(i, k, scope);
                    return self.export(k, end, scope, i);
                }
                if self.is(k, "class") || (self.is(k, "abstract") && self.is(k + 1, "class")) {
                    return self.class_declaration(k, end, scope, decl);
                }
                None
            }
            "export" if self.ident(i) => self.export(i, end, scope, i),
            "import" if self.ident(i) && !self.is(i + 1, "(") && !self.is(i + 1, ".") => {
                Some(self.import(i, end, scope))
            }
            _ if self.ident(i) => {
                let decl = Decl {
                    anchor: Some(i),
                    ..Decl::default()
                };
                self.declare(i, end, scope, decl)
            }
            _ => None,
        }
    }

    fn export(&mut self, i: usize, end: usize, scope: &Scope, anchor: usize) -> Option<usize> {
        let j = i + 1;
        let decl = Decl {
            public: true,
            anchor: Some(anchor),
            first: None,
            default: false,
        };
        if self.is(j, "default") {
            let k = j + 1;
            let decl = Decl {
                default: true,
                ..decl
            };
            if self.is(k, "function") || (self.is(k, "async") && self.is(k + 1, "function")) {
                return self.function_declaration(k, end, scope, decl).or(Some(k));
            }
            if self.is(k, "class") || (self.is(k, "abstract") && self.is(k + 1, "class")) {
                return self.class_declaration(k, end, scope, decl).or(Some(k));
            }
            if self.ts && self.is(k, "interface") {
                return self.interface(k, end, scope, decl).or(Some(k));
            }
            if let Some(shape) = self.function_shape(k, end) {
                let item = Item {
                    kind: NodeKind::Symbol,
                    symbol: SymbolKind::Function,
                    local: "default".into(),
                    first: shape.start,
                    last: shape.last(end),
                    sig_end: self.start_byte(shape.body),
                    body: shape.body_bytes(self, end),
                    public: true,
                    doc: Some(anchor),
                };
                let inner = self.item(item, scope);
                self.walk_function(&shape, &inner);
                return Some(shape.end);
            }
            return Some(k);
        }
        let clause = |w: &Self, k: usize| w.is(k, "{") || w.is(k, "*");
        if clause(self, j) || (self.is(j, "type") && clause(self, j + 1)) {
            return Some(self.export_clause(i, end, scope));
        }
        if self.is(j, "=") {
            return Some(j);
        }
        if self.is(j, "as") || self.is(j, "import") {
            return Some(self.statement_end(j, end));
        }
        self.declare(j, end, scope, decl).or(Some(j))
    }

    pub fn declare(&mut self, i: usize, end: usize, scope: &Scope, decl: Decl) -> Option<usize> {
        let next_word = self.ident(i + 1) && !self.nl(i + 1);
        match self.text(i) {
            "declare" if self.ts && next_word => {
                let decl = Decl {
                    public: false,
                    anchor: None,
                    ..decl
                };
                self.declare(i + 1, end, scope, decl)
            }
            "async" if self.is(i + 1, "function") && !self.nl(i + 1) => {
                self.function_declaration(i, end, scope, decl)
            }
            "function" => self.function_declaration(i, end, scope, decl),
            "class" => self.class_declaration(i, end, scope, decl),
            "abstract" if self.ts && self.is(i + 1, "class") => {
                self.class_declaration(i, end, scope, decl)
            }
            "const" if self.ts && self.is(i + 1, "enum") => self.enumeration(i, end, scope, decl),
            "const" | "let" | "var"
                if self.ident(i + 1) || self.is(i + 1, "{") || self.is(i + 1, "[") =>
            {
                Some(self.variables(i, end, scope, decl))
            }
            "interface" if self.ts && self.word(i + 1) => self.interface(i, end, scope, decl),
            "type" if self.ts && self.ident(i + 1) && (self.is(i + 2, "=") || self.is(i + 2, "<")) => {
                self.type_alias(i, end, scope, decl)
            }
            "enum" if self.ts && self.ident(i + 1) => self.enumeration(i, end, scope, decl),
            "namespace" | "module"
                if self.ts
                    && !self.nl(i + 1)
                    && (self.ident(i + 1) || self.kind(i + 1) == Some(Kind::Str)) =>
            {
                Some(self.namespace(i + 1, end, scope))
            }
            "global" if self.ts && self.is(i + 1, "{") => Some(self.namespace(i + 1, end, scope)),
            _ => None,
        }
    }

    /// `const|let|var` declarator lists; arrow and function values become items.
    fn variables(&mut self, i: usize, end: usize, scope: &Scope, decl: Decl) -> usize {
        let mut j = i + 1;
        loop {
            let pattern = j;
            if self.is(j, "{") || self.is(j, "[") {
                j = self.after(j, end);
            } else if self.ident(j) {
                j += 1;
            } else {
                break;
            }
            let pattern_end = j;
            if self.ts && self.is(j, "!") {
                j += 1;
            }
            let annotation = (self.ts && self.is(j, ":")).then_some(j);
            if annotation.is_some() {
                j = self.type_end(j + 1, end);
            }
            let value = self.is(j, "=").then_some(j + 1);
            let shape = value
                .filter(|_| self.ident(pattern))
                .and_then(|v| self.function_value(v, end));
            let value_end = match &shape {
                Some(shape) => shape.end,
                None => value.map_or(j, |v| self.expr_end(v, end)),
            };
            if let Some(shape) = shape {
                let item = Item {
                    kind: NodeKind::Symbol,
                    symbol: SymbolKind::Function,
                    local: self.text(pattern).to_owned(),
                    first: pattern,
                    last: value_end - 1,
                    sig_end: self.start_byte(shape.body),
                    body: shape.body_bytes(self, end),
                    public: decl.public,
                    doc: decl.anchor,
                };
                let inner = self.item(item, scope);
                if let Some(colon) = annotation {
                    self.scan_type(colon + 1, end, &inner);
                }
                self.walk_function(&shape, &inner);
            } else {
                if pattern_end > pattern + 1 {
                    self.expr(pattern, pattern_end, scope);
                }
                if let Some(colon) = annotation {
                    self.scan_type(colon + 1, end, scope);
                }
                if let Some(v) = value {
                    let binding = Binding {
                        pattern,
                        value: v,
                        end: value_end,
                    };
                    let saved = self.binding.replace(binding);
                    self.expr(v, value_end, scope);
                    self.binding = saved;
                }
            }
            j = value_end;
            if self.is(j, ",") {
                j += 1;
                continue;
            }
            break;
        }
        j + usize::from(self.is(j, ";"))
    }

    pub fn statement_end(&self, j: usize, end: usize) -> usize {
        let e = self.expr_end(j, end);
        e + usize::from(self.is(e, ";"))
    }

    /// Last token of a group closed at `close`, or of the range when unclosed.
    pub fn last(close: usize, end: usize) -> usize {
        if close < end {
            close
        } else {
            end.saturating_sub(1)
        }
    }

    pub fn decorator_end(&self, i: usize, end: usize) -> usize {
        let mut j = i + 1;
        if self.is(j, "(") {
            return self.after(j, end);
        }
        if self.ident(j) {
            j += 1;
            while self.is(j, ".") && self.ident(j + 1) {
                j += 2;
            }
            if self.is(j, "(") {
                j = self.after(j, end);
            }
        }
        j.min(end).max(i + 1)
    }
}

#[cfg(test)]
mod tests {
    use crate::JsLexer;

    fn names(path: &str, source: &str) -> Vec<String> {
        let facts = JsLexer::default().extract(path, source.as_bytes()).unwrap();
        facts.nodes.iter().skip(1).map(|n| n.fact.name.clone()).collect()
    }

    #[test]
    fn optional_catch_and_labels_keep_inner_declarations() {
        let source = "function outer() {\n  loop: for (;;) { break loop }\n  try { throw 1 } catch { function inner() { return 1 } }\n  try { throw 2 } catch (e) { function bound() { return e } }\n}";
        assert_eq!(names("x.js", source), ["outer", "outer::inner", "outer::bound"]);
    }

    #[test]
    fn asi_and_control_parens_start_statements() {
        let source = "run()\nfunction a() {}\nif (x) function b() {}\nlet c = () => 1\nconst d = function () {}.bind(this)";
        assert_eq!(names("x.js", source), ["a", "b", "c"]);
    }

    #[test]
    fn exports_defaults_and_declare_follow_the_oracle() {
        let facts = JsLexer::default()
            .extract(
                "x.ts",
                b"export default () => 1;\ndeclare function f(): void;\nexport function g(a: A): void;",
            )
            .unwrap();
        let summary: Vec<_> = facts
            .nodes
            .iter()
            .skip(1)
            .map(|n| (n.fact.name.as_str(), n.fact.public, n.fact.signature.as_str()))
            .collect();
        assert_eq!(
            summary,
            [
                ("default", true, "() =>"),
                ("f", false, "function f(): void;"),
                ("g", true, "function g(a: A): void;")
            ]
        );
    }
}
