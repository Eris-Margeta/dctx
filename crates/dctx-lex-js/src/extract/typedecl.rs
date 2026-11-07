// SPDX-License-Identifier: Apache-2.0
//! TypeScript declarations: interfaces, type aliases, enums and namespaces.
//! Implements SPEC-MAP-MODE 5.1 interface/type/enum symbols; namespace bodies are
//! transparent (their members keep root names), as in dctx-map-parse.
//! Uses types for annotations; called from `Walker::declare`.
use super::statements::Decl;
use super::{Item, Scope, Walker};
use dctx_map_model::{NodeKind, SymbolKind};

impl Walker<'_> {
    pub fn interface(&mut self, k: usize, end: usize, scope: &Scope, decl: Decl) -> Option<usize> {
        let name = k + 1;
        let mut j = k + 2;
        if self.is(j, "<") {
            j = self.type_params_end(j, end);
        }
        let heritage = j;
        if self.is(j, "extends") {
            loop {
                j = self.type_end(j + 1, end);
                if !self.is(j, ",") {
                    break;
                }
            }
        }
        if !self.is(j, "{") {
            return None;
        }
        let close = self.close(j, end);
        let last = Self::last(close, end);
        let item = Item {
            kind: NodeKind::Symbol,
            symbol: SymbolKind::Interface,
            local: self.text(name).to_owned(),
            first: k,
            last,
            sig_end: self.start_byte(j),
            body: (self.start_byte(j), self.end_byte(last)),
            public: decl.public,
            doc: decl.anchor,
        };
        let inner = self.item(item, scope);
        if self.is(k + 2, "<") {
            self.type_params(k + 2, heritage, &inner);
        }
        self.type_list(heritage, j, &inner);
        self.type_body(j + 1, close, &inner);
        Some((close + 1).min(end))
    }

    pub fn type_alias(&mut self, k: usize, end: usize, scope: &Scope, decl: Decl) -> Option<usize> {
        let name = k + 1;
        let mut j = k + 2;
        let generics = self.is(j, "<").then_some(j);
        if generics.is_some() {
            j = self.type_params_end(j, end);
        }
        if !self.is(j, "=") {
            return None;
        }
        let stop = self.type_end(j + 1, end);
        let last = if self.is(stop, ";") {
            stop
        } else {
            stop.saturating_sub(1).max(j)
        };
        let item = Item {
            kind: NodeKind::Symbol,
            symbol: SymbolKind::Type,
            local: self.text(name).to_owned(),
            first: k,
            last,
            sig_end: self.end_byte(last),
            body: (
                self.start_byte(j + 1),
                self.end_byte(stop.saturating_sub(1).max(j)),
            ),
            public: decl.public,
            doc: decl.anchor,
        };
        let inner = self.item(item, scope);
        if let Some(open) = generics {
            self.type_params(open, j, &inner);
        }
        self.scan_type(j + 1, end, &inner);
        Some(last + 1)
    }

    pub fn enumeration(
        &mut self,
        k: usize,
        end: usize,
        scope: &Scope,
        decl: Decl,
    ) -> Option<usize> {
        let keyword = k + usize::from(self.is(k, "const"));
        let name = keyword + 1;
        let body = name + 1;
        if !self.ident(name) || !self.is(body, "{") {
            return None;
        }
        let close = self.close(body, end);
        let last = Self::last(close, end);
        let item = Item {
            kind: NodeKind::Symbol,
            symbol: SymbolKind::Enum,
            local: self.text(name).to_owned(),
            first: k,
            last,
            sig_end: self.start_byte(body),
            body: (self.start_byte(body), self.end_byte(last)),
            public: decl.public,
            doc: decl.anchor,
        };
        let inner = self.item(item, scope);
        self.expr(body + 1, close, &inner);
        Some((close + 1).min(end))
    }

    /// `namespace A.B { .. }`, `module 'm' { .. }` and `global { .. }` from the name on.
    pub fn namespace(&mut self, mut j: usize, end: usize, scope: &Scope) -> usize {
        if !self.is(j, "{") {
            j += 1;
            while self.is(j, ".") && self.ident(j + 1) {
                j += 2;
            }
        }
        if !self.is(j, "{") {
            return j + usize::from(self.is(j, ";"));
        }
        let close = self.close(j, end);
        self.statements(j + 1, close, scope);
        (close + 1).min(end)
    }

    /// Comma-separated heritage types between `begin` and `stop`.
    pub fn type_list(&mut self, mut j: usize, stop: usize, scope: &Scope) {
        while j < stop && self.ok() {
            if self.is(j, "extends") || self.is(j, "implements") || self.is(j, ",") {
                j += 1;
            } else {
                j = self.scan_type(j, stop, scope).max(j + 1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::JsLexer;
    use dctx_map_model::{EdgeKind, SymbolKind};

    #[test]
    fn declarations_own_their_type_references() {
        let source = "export interface W<T extends Base> extends Other<T> { run(): T }\ntype R<K> = Map<K, ns.Value>;\nconst enum E { A = f(), B }\nnamespace N.M { export function g(): void {} }";
        let facts = JsLexer::default()
            .extract("d.ts", source.as_bytes())
            .unwrap();
        let nodes: Vec<_> = facts
            .nodes
            .iter()
            .skip(1)
            .map(|n| (n.fact.name.as_str(), n.fact.symbol_kind, n.fact.public))
            .collect();
        assert_eq!(
            nodes,
            [
                ("W", Some(SymbolKind::Interface), true),
                ("W::run", Some(SymbolKind::Method), false),
                ("R", Some(SymbolKind::Type), false),
                ("E", Some(SymbolKind::Enum), false),
                ("g", Some(SymbolKind::Function), true)
            ]
        );
        let types: Vec<_> = facts
            .references
            .iter()
            .filter(|r| r.fact.kind == EdgeKind::UsesType)
            .map(|r| (r.fact.scope.as_str(), r.fact.name.as_str()))
            .collect();
        assert_eq!(
            types,
            [
                ("W", "Base"),
                ("W", "Other"),
                ("W", "T"),
                ("W::run", "T"),
                ("R", "Map"),
                ("R", "K"),
                ("R", "ns.Value")
            ]
        );
    }
}
