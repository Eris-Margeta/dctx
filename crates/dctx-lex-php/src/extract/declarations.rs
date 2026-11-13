// SPDX-License-Identifier: Apache-2.0
//! Declaration dispatch, namespace scopes and class-like PHP declarations.
//! Implements decision 077 structural facts without inferring runtime declarations.
//! Uses the bounded token cursor and fact assembly in the parent module.
use super::{Item, Scope, Walker};
use crate::LexError;
use crate::domain::{DiagnosticReason, Precision};
use crate::lexer::Kind;
use dctx_map_model::{EdgeKind, SymbolKind};

impl Walker<'_> {
    pub(super) fn range(
        &mut self,
        mut i: usize,
        end: usize,
        mut scope: Scope,
    ) -> Result<(), LexError> {
        while i < end {
            let first = i;
            let mut public = true;
            loop {
                if self.is(i, "#[") {
                    i = self.attributes(i, end, &scope)?;
                } else if self.kind(i) == Some(Kind::Ident) && modifier(self.text(i)) {
                    if self.is(i, "private") || self.is(i, "protected") {
                        public = false;
                    }
                    i += 1;
                } else {
                    break;
                }
            }
            if i >= end {
                self.problem(first, DiagnosticReason::Unrecognized);
                break;
            }
            let word = if matches!(self.kind(i), Some(Kind::Ident | Kind::Punct)) {
                self.text(i).to_ascii_lowercase()
            } else {
                String::new()
            };
            i = match word.as_str() {
                "namespace" if !self.is(i + 1, "\\") && !scope.executable => {
                    self.namespace(first, i, end, &mut scope)?
                }
                "class" | "interface" | "trait" | "enum" if !self.is(i.saturating_sub(1), "::") => {
                    self.class_decl(first, i, end, &scope, public)?
                }
                "function" => self.function(first, i, end, &scope, public)?,
                "use" if !scope.executable => self.use_decl(i, end, &scope)?,
                "const" => self.values(first, i + 1, end, &scope, public, true)?,
                "case" if scope.class => self.values(first, i + 1, end, &scope, true, true)?,
                "require" | "require_once" | "include" | "include_once" => {
                    self.include(i, end, &scope)?
                }
                _ if scope.class && !scope.executable && self.property_start(i, end) => {
                    self.values(first, i, end, &scope, public, false)?
                }
                "{" => {
                    let close = self.close(i, end).unwrap_or(end);
                    let mut nested = scope.clone();
                    nested.executable = true;
                    self.range(i + 1, close, nested)?;
                    (close + 1).min(end)
                }
                "#[" => self.after(i, end),
                _ => self.expression(i, end, &scope)?,
            };
            if i <= first {
                i = first + 1;
            }
        }
        Ok(())
    }

    fn namespace(
        &mut self,
        first: usize,
        at: usize,
        end: usize,
        scope: &mut Scope,
    ) -> Result<usize, LexError> {
        let (name, next) = self.name(at + 1, end);
        if !matches!(self.text(next), ";" | "{") {
            self.problem(at, DiagnosticReason::Unrecognized);
            return Ok(next.max(at + 1));
        }
        let block = self.is(next, "{");
        let last = if block {
            self.close(next, end).unwrap_or(end.saturating_sub(1))
        } else {
            next
        };
        let root = Scope::default();
        let mut namespace = if name.is_empty() {
            root.clone()
        } else {
            self.item(
                Item {
                    local: name,
                    symbol: None,
                    first,
                    last,
                    sig_end: self.start(next),
                    public: true,
                },
                &root,
            )?
        };
        namespace.class = false;
        if block {
            let close = self.close(next, end).unwrap_or(end);
            self.range(next + 1, close, namespace)?;
            Ok((close + 1).min(end))
        } else {
            *scope = namespace;
            Ok(next + 1)
        }
    }

    fn class_decl(
        &mut self,
        first: usize,
        at: usize,
        end: usize,
        scope: &Scope,
        public: bool,
    ) -> Result<usize, LexError> {
        let symbol = match self.text(at).to_ascii_lowercase().as_str() {
            "interface" => SymbolKind::Interface,
            "trait" => SymbolKind::Trait,
            "enum" => SymbolKind::Enum,
            _ => SymbolKind::Class,
        };
        let named = self.kind(at + 1) == Some(Kind::Ident)
            && !self.is(at + 1, "extends")
            && !self.is(at + 1, "implements");
        let mut cursor = at + if named { 2 } else { 1 };
        while cursor < end && !matches!(self.text(cursor), "{" | ";" | "}") {
            cursor = self.after(cursor, end);
        }
        let close = if self.is(cursor, "{") {
            self.close(cursor, end).unwrap_or(end)
        } else {
            cursor
        };
        if !self.is(cursor, "{") {
            self.problem(at, DiagnosticReason::Unrecognized);
        }
        let mut child = if named {
            self.item(
                Item {
                    local: self.text(at + 1).to_string(),
                    symbol: Some(symbol),
                    first,
                    last: close.min(end.saturating_sub(1)),
                    sig_end: self.start(cursor),
                    public,
                },
                scope,
            )?
        } else {
            self.problem(at, DiagnosticReason::UnsupportedName);
            scope.clone()
        };
        let mut j = at + if named { 2 } else { 1 };
        let mut relation = None;
        while j < cursor {
            if self.is(j, "extends") {
                relation = Some(if symbol == SymbolKind::Interface {
                    EdgeKind::Implements
                } else {
                    EdgeKind::UsesType
                });
                j += 1;
            } else if self.is(j, "implements") {
                relation = Some(EdgeKind::Implements);
                j += 1;
            } else if self.is(j, ",") {
                j += 1;
            } else {
                let (name, next) = self.name(j, cursor);
                if let Some(kind) = relation {
                    self.reference(&child, kind, name, None, (j, Precision::Structural))?;
                }
                j = next.max(j + 1);
            }
        }
        child.class = named;
        child.executable = !named;
        if self.is(cursor, "{") {
            self.range(cursor + 1, close, child)?;
        }
        Ok((close + 1).min(end))
    }
}

fn modifier(word: &str) -> bool {
    matches!(
        word.to_ascii_lowercase().as_str(),
        "public" | "protected" | "private" | "static" | "abstract" | "final" | "readonly" | "var"
    )
}
