// SPDX-License-Identifier: Apache-2.0
//! Classes, modules, methods, singleton classes and constants.
//! Implements SPEC-MAP-MODE 5.1 declaration spans for Ruby `class`, `module` and `def`.
//! Metaprogramming is not a declaration: only a statement-level keyword is extracted.
use super::{OutRef, Scope, Sym, Vis, Walker, qualify};
use crate::domain::{DiagnosticReason, Precision, collapse};
use crate::lexer::Kind;
use dctx_map_model::{EdgeKind, SymbolKind};

pub(super) struct Head {
    name: String,
    singleton: bool,
    explicit: Option<ConstPath>,
}

pub(super) struct ConstPath {
    parts: Vec<String>,
    absolute: bool,
}

impl Walker<'_> {
    pub(super) fn declaration(&mut self, scope: &Scope) -> bool {
        if self.at_word("class") || self.at_word("module") {
            self.type_decl(scope);
            true
        } else if self.at_word("def") {
            self.method_decl(scope, None, false);
            true
        } else {
            false
        }
    }

    pub(super) fn const_stmt(&mut self, scope: &Scope) -> bool {
        if !self.is_const() && !self.at_punct("::") {
            return false;
        }
        let save = self.i;
        let start = self.tok_start();
        let Some(path) = self.const_path() else {
            self.i = save;
            return false;
        };
        if !self.at_punct("=") {
            self.i = save;
            return false;
        }
        self.bump();
        self.scan_statement(scope);
        let end = self.prev_end().max(start);
        let name = qualify_type(scope, &path);
        let _ = self.emit_symbol(
            scope,
            Sym {
                name,
                symbol: SymbolKind::Const,
                signature: collapse(self.slice(start, end)),
                start,
                end,
                public: true,
                precision: Precision::Structural,
                doc: self.docs(start),
            },
        );
        true
    }

    fn type_decl(&mut self, scope: &Scope) {
        let keyword = self.cur().to_owned();
        let start = self.tok_start();
        self.bump();
        if keyword == "class" && self.at_punct("<<") {
            self.singleton_class(scope, start);
            return;
        }
        let Some(path) = self.const_path() else {
            self.problem(start, DiagnosticReason::Malformed);
            self.recover(scope);
            return;
        };
        let mut super_name = None;
        if keyword == "class" && self.at_punct("<") {
            self.bump();
            if let Some(parent) = self.const_path() {
                super_name = Some(path_text(&parent));
            } else {
                self.skip_header(scope);
            }
        }
        let header_end = self.prev_end().max(start);
        let name = qualify_type(scope, &path);
        let symbol = if keyword == "module" {
            SymbolKind::Type
        } else {
            SymbolKind::Class
        };
        let idx = self.emit_symbol(
            scope,
            Sym {
                name: name.clone(),
                symbol,
                signature: collapse(self.slice(start, header_end)),
                start,
                end: header_end,
                public: true,
                precision: Precision::Structural,
                doc: self.docs(start),
            },
        );
        if let (Some(index), Some(parent)) = (idx, super_name) {
            self.emit_ref(OutRef {
                owner: index,
                scope: &name,
                kind: EdgeKind::UsesType,
                name: parent,
                start,
                end: header_end,
                precision: Precision::Structural,
            });
        }
        self.push_vis();
        let child = Scope {
            owner: idx.unwrap_or(scope.owner),
            name: if idx.is_some() {
                name
            } else {
                scope.name.clone()
            },
            defining: true,
        };
        if self.nest() {
            self.until_end(&child, true);
            self.unnest();
        }
        self.pop_vis();
        if let Some(index) = idx {
            self.patch(index);
        }
    }

    fn singleton_class(&mut self, scope: &Scope, start: u32) {
        self.bump();
        let is_self = self.at_word("self");
        if is_self {
            self.bump();
        } else {
            self.skip_header(scope);
        }
        let node_name = qualify(scope, if is_self { "<<self" } else { "<<expr" });
        let methods = if is_self {
            if scope.name.is_empty() {
                "self".to_owned()
            } else {
                scope.name.clone()
            }
        } else {
            node_name.clone()
        };
        let header_end = self.prev_end().max(start);
        let idx = self.emit_symbol(
            scope,
            Sym {
                name: node_name,
                symbol: SymbolKind::Class,
                signature: collapse(self.slice(start, header_end)),
                start,
                end: header_end,
                public: true,
                precision: if is_self {
                    Precision::Structural
                } else {
                    Precision::Convention
                },
                doc: self.docs(start),
            },
        );
        self.push_vis();
        let child = Scope {
            owner: idx.unwrap_or(scope.owner),
            name: methods,
            defining: true,
        };
        if self.nest() {
            self.until_end(&child, true);
            self.unnest();
        }
        self.pop_vis();
        if let Some(index) = idx {
            self.patch(index);
        }
    }

    pub(super) fn method_decl(&mut self, scope: &Scope, vis: Option<Vis>, forced_modfn: bool) {
        let start = self.tok_start();
        self.bump();
        let Some(head) = self.method_head() else {
            self.problem(start, DiagnosticReason::Malformed);
            self.recover(scope);
            return;
        };
        // `def name = expr` and `def name() = expr` have no `end`.
        let eq_before_params = self.at_punct("=");
        if !eq_before_params {
            self.skip_params();
        }
        let endless = eq_before_params || self.at_punct("=");
        let header_end = self.prev_end().max(start);
        let name = method_name(scope, &head);
        let kind = if scope.name.is_empty() && !head.singleton && head.explicit.is_none() {
            SymbolKind::Function
        } else {
            SymbolKind::Method
        };
        let idx = self.emit_symbol(
            scope,
            Sym {
                name: name.clone(),
                symbol: kind,
                signature: collapse(self.slice(start, header_end)),
                start,
                end: header_end,
                public: self.method_public(vis, forced_modfn),
                precision: Precision::Structural,
                doc: self.docs(start),
            },
        );
        let child = Scope {
            owner: idx.unwrap_or(scope.owner),
            name: if idx.is_some() {
                name
            } else {
                scope.name.clone()
            },
            defining: false,
        };
        if endless {
            self.bump();
            self.scan_statement(&child);
        } else if self.nest() {
            self.until_end(&child, true);
            self.unnest();
        }
        if let Some(index) = idx {
            self.patch(index);
        }
    }

    fn method_head(&mut self) -> Option<Head> {
        if self.at_word("self") && (self.peek_punct(".") || self.peek_punct("::")) {
            self.bump();
            self.bump();
            return Some(Head {
                name: self.raw_method_name()?,
                singleton: true,
                explicit: None,
            });
        }
        if self.is_const() || self.at_punct("::") {
            let save = self.i;
            if let Some(path) = self.const_path()
                && (self.at_punct(".") || self.at_punct("::"))
            {
                self.bump();
                if let Some(name) = self.raw_method_name() {
                    return Some(Head {
                        name,
                        singleton: true,
                        explicit: Some(path),
                    });
                }
            }
            self.i = save;
        }
        Some(Head {
            name: self.raw_method_name()?,
            singleton: false,
            explicit: None,
        })
    }

    fn raw_method_name(&mut self) -> Option<String> {
        if self.at_kind(Kind::Ident) && self.cur() != "end" {
            let name = self.cur().to_owned();
            self.bump();
            return Some(name);
        }
        if self.at_punct("[") {
            let from = self.i;
            self.bump();
            if self.at_punct("]") {
                self.bump();
            }
            if self.at_punct("=") {
                self.bump();
            }
            return Some(self.joined(from, self.i));
        }
        if self.at_kind(Kind::Punct) && is_method_op(self.cur()) {
            let name = self.cur().to_owned();
            self.bump();
            return Some(name);
        }
        None
    }

    pub(super) fn const_path(&mut self) -> Option<ConstPath> {
        let absolute = if self.at_punct("::") {
            self.bump();
            true
        } else {
            false
        };
        if !self.is_const() {
            return None;
        }
        let mut parts = vec![self.cur().to_owned()];
        self.bump();
        while self.at_punct("::") {
            let save = self.i;
            self.bump();
            if self.is_const() {
                parts.push(self.cur().to_owned());
                self.bump();
            } else {
                self.i = save;
                break;
            }
        }
        Some(ConstPath { parts, absolute })
    }

    pub(super) fn skip_params(&mut self) {
        if self.at_punct("(") {
            let mut depth = 0i32;
            while !self.eof() {
                if self.at_punct("(") {
                    depth += 1;
                    self.bump();
                    continue;
                }
                if self.at_punct(")") {
                    depth -= 1;
                    self.bump();
                    if depth == 0 {
                        break;
                    }
                    continue;
                }
                self.bump();
            }
            return;
        }
        let mut brackets = 0i32;
        while !self.eof() {
            if brackets == 0 && (self.is_sep() || self.at_word("end")) {
                break;
            }
            if matches!(self.cur(), "(" | "[" | "{") && self.at_kind(Kind::Punct) {
                brackets += 1;
            }
            if matches!(self.cur(), ")" | "]" | "}") && self.at_kind(Kind::Punct) && brackets > 0 {
                brackets -= 1;
            }
            self.bump();
        }
    }

    fn recover(&mut self, scope: &Scope) {
        if self.nest() {
            let inner = Scope {
                defining: false,
                ..scope.clone()
            };
            self.until_end(&inner, true);
            self.unnest();
        }
    }

    fn joined(&self, from: usize, to: usize) -> String {
        let mut text = String::new();
        for index in from..to {
            text.push_str(self.text_at(index));
        }
        text
    }
}

fn qualify_type(scope: &Scope, path: &ConstPath) -> String {
    if path.absolute {
        format!("::{}", path.parts.join("::"))
    } else if path.parts.len() == 1 {
        qualify(scope, &path.parts[0])
    } else {
        path.parts.join("::")
    }
}

fn path_text(path: &ConstPath) -> String {
    let shown = path.parts.join("::");
    if path.absolute {
        format!("::{shown}")
    } else {
        shown
    }
}

fn method_name(scope: &Scope, head: &Head) -> String {
    let owner = if let Some(path) = &head.explicit {
        path_text(path)
    } else if head.singleton {
        if scope.name.is_empty() {
            "self".to_owned()
        } else {
            scope.name.clone()
        }
    } else {
        scope.name.clone()
    };
    if owner.is_empty() {
        head.name.clone()
    } else {
        format!("{owner}::{}", head.name)
    }
}

fn is_method_op(text: &str) -> bool {
    matches!(
        text,
        "<<" | ">>"
            | "=="
            | "==="
            | "=~"
            | "!="
            | "!~"
            | "<="
            | ">="
            | "<=>"
            | "**"
            | "+"
            | "-"
            | "*"
            | "/"
            | "%"
            | "&"
            | "|"
            | "^"
            | "~"
            | "!"
            | "`"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_relative_constant_is_nested_and_a_path_is_not() {
        let scope = Scope {
            owner: 0,
            name: "Demo".into(),
            defining: true,
        };
        let simple = ConstPath {
            parts: vec!["Box".into()],
            absolute: false,
        };
        let path = ConstPath {
            parts: vec!["Foo".into(), "Bar".into()],
            absolute: false,
        };
        assert_eq!(qualify_type(&scope, &simple), "Demo::Box");
        assert_eq!(qualify_type(&scope, &path), "Foo::Bar");
    }
}
