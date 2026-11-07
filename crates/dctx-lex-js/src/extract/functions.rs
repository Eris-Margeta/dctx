// SPDX-License-Identifier: Apache-2.0
//! Functions: declarations, TS signatures, function expressions and arrows.
//! Implements SPEC-MAP-MODE 5.1 function facts with dctx-map-parse spans and signatures
//! (`function f()` up to the body; whole text for TS overloads and `declare`).
//! Uses statements, params and types; called by every site that may hold a function.
use super::statements::Decl;
use super::{Item, Scope, Walker};
use dctx_map_model::{NodeKind, SymbolKind};

/// A function or arrow expression: `start` is its first token, `end` is one past it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct FnShape {
    pub start: usize,
    pub generics: Option<usize>,
    pub params: Option<(usize, usize)>,
    pub returns: Option<usize>,
    pub body: usize,
    pub close: usize,
    pub block: bool,
    pub end: usize,
}

impl FnShape {
    pub fn last(&self, end: usize) -> usize {
        if self.block {
            Walker::last(self.close, end)
        } else {
            self.end.saturating_sub(1).max(self.body)
        }
    }

    pub fn body_bytes(&self, w: &Walker<'_>, end: usize) -> (u32, u32) {
        (w.start_byte(self.body), w.end_byte(self.last(end)))
    }
}

impl Walker<'_> {
    pub fn function_declaration(
        &mut self,
        k: usize,
        end: usize,
        scope: &Scope,
        decl: Decl,
    ) -> Option<usize> {
        let mut j = k + usize::from(self.is(k, "async")) + 1;
        if self.is(j, "*") {
            j += 1;
        }
        let name = (self.word(j) || (self.ident(j) && !self.is(j + 1, "."))) && !self.is(j, "(");
        let local = match (name, decl.default) {
            (true, _) => self.text(j).to_owned(),
            (false, true) => "default".to_owned(),
            (false, false) => return None,
        };
        j += usize::from(name);
        let generics = (self.ts && self.is(j, "<")).then_some(j);
        if generics.is_some() {
            j = self.type_params_end(j, end);
        }
        if !self.is(j, "(") {
            return None;
        }
        let params = j;
        let params_close = self.close(j, end);
        j = (params_close + 1).min(end);
        let returns = (self.ts && self.is(j, ":")).then_some(j);
        if returns.is_some() {
            j = self.type_end(j + 1, end);
        }
        let first = decl.first.unwrap_or(k);
        let block = self.is(j, "{");
        if !block && !self.ts {
            return None;
        }
        let close = if block { self.close(j, end) } else { j };
        let last = if block {
            Self::last(close, end)
        } else if self.is(j, ";") {
            j
        } else {
            j.saturating_sub(1).max(params)
        };
        let (sig_end, body) = if block {
            (
                self.start_byte(j),
                (self.start_byte(j), self.end_byte(last)),
            )
        } else {
            let whole = (self.start_byte(first), self.end_byte(last));
            (whole.1, whole)
        };
        let item = Item {
            kind: NodeKind::Symbol,
            symbol: SymbolKind::Function,
            local,
            first,
            last,
            sig_end,
            body,
            public: decl.public,
            doc: decl.anchor,
        };
        let inner = self.item(item, scope);
        if let Some(open) = generics {
            self.type_params(open, params, &inner);
        }
        self.params(params + 1, params_close, &inner);
        if let Some(colon) = returns {
            self.scan_type(colon + 1, end, &inner);
        }
        if block {
            self.statements(j + 1, close, &inner);
            Some((close + 1).min(end))
        } else {
            Some(last + 1)
        }
    }

    /// Recognises a function or arrow expression starting exactly at `v`.
    pub fn function_shape(&mut self, v: usize, end: usize) -> Option<FnShape> {
        let mut k = v;
        if self.is(k, "async")
            && !self.nl(k + 1)
            && (self.is(k + 1, "function")
                || self.is(k + 1, "(")
                || (self.ts && self.is(k + 1, "<"))
                || (self.word(k + 1) && self.is(k + 2, "=>")))
        {
            k += 1;
        }
        let mut generics = None;
        let mut returns = None;
        let params;
        let body;
        if self.is(k, "function") {
            k += 1 + usize::from(self.is(k + 1, "*"));
            if self.ident(k) && !self.is(k, "(") {
                k += 1;
            }
            if self.ts && self.is(k, "<") {
                generics = Some(k);
                k = self.type_params_end(k, end);
            }
            if !self.is(k, "(") {
                return None;
            }
            let close = self.close(k, end);
            params = Some((k, close));
            k = (close + 1).min(end);
            if self.ts && self.is(k, ":") {
                returns = Some(k);
                k = self.type_end(k + 1, end);
            }
            if !self.is(k, "{") {
                return None;
            }
            body = k;
        } else {
            let arrow = if self.word(k) && self.is(k + 1, "=>") {
                params = None;
                k + 1
            } else {
                if self.ts && self.is(k, "<") {
                    generics = Some(k);
                    k = self.type_params_end(k, end);
                }
                if !self.is(k, "(") {
                    return None;
                }
                let close = self.close(k, end);
                let arrow = self.arrow_after(close, end)?;
                params = Some((k, close));
                returns = self.is(close + 1, ":").then_some(close + 1);
                arrow
            };
            body = arrow + 1;
            if body >= end {
                return None;
            }
            if !self.is(body, "{") {
                let stop = self.expr_end(body, end);
                if stop <= body {
                    return None;
                }
                return Some(FnShape {
                    start: v,
                    generics,
                    params,
                    returns,
                    body,
                    close: stop,
                    block: false,
                    end: stop,
                });
            }
        }
        let close = self.close(body, end);
        Some(FnShape {
            start: v,
            generics,
            params,
            returns,
            body,
            close,
            block: true,
            end: (close + 1).min(end),
        })
    }

    /// Recognises a function value that is the whole expression starting at `v`.
    pub fn function_value(&mut self, v: usize, end: usize) -> Option<FnShape> {
        let shape = self.function_shape(v, end)?;
        let k = shape.end;
        (k >= end || self.asi(k) || self.expr_end(k, end) == k).then_some(shape)
    }

    pub fn walk_function(&mut self, shape: &FnShape, scope: &Scope) {
        if let Some(open) = shape.generics {
            self.type_params(open, shape.end, scope);
        }
        if let Some((open, close)) = shape.params {
            self.params(open + 1, close, scope);
        }
        if let Some(colon) = shape.returns {
            self.scan_type(colon + 1, shape.end, scope);
        }
        if shape.block {
            self.statements(shape.body + 1, shape.close, scope);
        } else {
            self.expr(shape.body, shape.close, scope);
        }
    }
}
