//! PHP functions, methods, constant declarations and property lists.
//! Implements structural signatures and visibility for decision 077/PARSERS-v1.
//! The model has no property kind, so properties use an unclassified symbol.
use super::{Item, Scope, Walker, normalize};
use crate::LexError;
use crate::domain::{DiagnosticReason, Precision};
use crate::lexer::Kind;
use dctx_map_model::{EdgeKind, SymbolKind};

impl Walker<'_> {
    pub(super) fn doc_before(&self, token: usize) -> Option<String> {
        let start = self.start(token);
        let index = self
            .lexed
            .comments
            .partition_point(|&(_, to)| to <= start)
            .checked_sub(1)?;
        let (from, to) = self.lexed.comments[index];
        if !self.src[to..start].trim().is_empty() || !self.src[from..to].starts_with("/**") {
            return None;
        }
        let body = self.src[from..to].strip_prefix("/**")?.strip_suffix("*/")?;
        let text = body
            .lines()
            .map(|s| s.trim().trim_start_matches('*').trim())
            .take_while(|s| !s.starts_with('@'))
            .collect::<Vec<_>>()
            .join(" ");
        let text = normalize(&text);
        let text = text
            .find(". ")
            .map_or(text.clone(), |end| text[..=end].to_string());
        (!text.is_empty()).then_some(text)
    }

    pub(super) fn function(
        &mut self,
        first: usize,
        at: usize,
        end: usize,
        scope: &Scope,
        public: bool,
    ) -> Result<usize, LexError> {
        let mut name_at = at + 1;
        if self.is(name_at, "&") {
            name_at += 1;
        }
        let named = self.kind(name_at) == Some(Kind::Ident);
        let params = name_at + usize::from(named);
        if !self.is(params, "(") {
            self.problem(at, DiagnosticReason::Unrecognized);
            return Ok(params.max(at + 1));
        }
        let param_close = self.close(params, end).unwrap_or(end);
        let mut body = (param_close + 1).min(end);
        while body < end && !matches!(self.text(body), "{" | ";" | "}") {
            body = self.after(body, end);
        }
        let close = if self.is(body, "{") {
            self.close(body, end).unwrap_or(end)
        } else {
            body
        };
        if param_close == end || body == end {
            self.problem(at, DiagnosticReason::Unrecognized);
        }
        let mut child = if named {
            self.item(
                Item {
                    local: self.text(name_at).to_string(),
                    symbol: Some(if scope.class && !scope.executable {
                        SymbolKind::Method
                    } else {
                        SymbolKind::Function
                    }),
                    first,
                    last: close.min(end.saturating_sub(1)),
                    sig_end: self.start(body),
                    public,
                },
                scope,
            )?
        } else {
            scope.clone()
        };
        self.signature_types(params + 1, param_close.min(end), &child)?;
        if named && self.is(name_at, "__construct") && scope.class && !scope.executable {
            self.promoted(params + 1, param_close.min(end), scope)?;
        }
        if self.is(param_close + 1, ":") {
            self.signature_types(param_close + 2, body, &child)?;
        }
        child.executable = true;
        if self.is(body, "{") {
            self.range(body + 1, close, child)?;
        }
        Ok((close + 1).min(end))
    }

    pub(super) fn property_start(&self, mut i: usize, end: usize) -> bool {
        while i < end {
            if self.kind(i) == Some(Kind::Variable) {
                return true;
            }
            if !matches!(self.kind(i), Some(Kind::Ident))
                && !matches!(self.text(i), "\\" | "?" | "|" | "&")
            {
                return false;
            }
            i += 1;
        }
        false
    }

    fn promoted(&mut self, mut i: usize, end: usize, scope: &Scope) -> Result<(), LexError> {
        while i < end {
            let first = i;
            let mut visibility = None;
            let mut variable = None;
            let mut default = false;
            while i < end && !self.is(i, ",") {
                if self.is(i, "=") {
                    default = true;
                }
                if !default {
                    if self.is(i, "public") {
                        visibility = Some(true);
                    }
                    if self.is(i, "private") || self.is(i, "protected") {
                        visibility = Some(false);
                    }
                    if self.kind(i) == Some(Kind::Variable) {
                        variable = Some(i);
                    }
                }
                i = self.after(i, end);
            }
            if let (Some(public), Some(at)) = (visibility, variable) {
                self.item(
                    Item {
                        local: self.text(at).to_string(),
                        symbol: None,
                        first,
                        last: at,
                        sig_end: self.end(at),
                        public,
                    },
                    scope,
                )?;
            }
            i += 1;
        }
        Ok(())
    }

    pub(super) fn values(
        &mut self,
        first: usize,
        mut i: usize,
        end: usize,
        scope: &Scope,
        public: bool,
        constant: bool,
    ) -> Result<usize, LexError> {
        let stop = self.statement_end(i, end);
        if !self.is(stop, ";") {
            self.problem(first, DiagnosticReason::Unrecognized);
        }
        while i < stop {
            let segment = i;
            let mut candidate = None;
            while i < stop && !matches!(self.text(i), "=" | ",") {
                if (constant && self.kind(i) == Some(Kind::Ident))
                    || (!constant && self.kind(i) == Some(Kind::Variable))
                {
                    candidate = Some(i);
                }
                i = self.after(i, stop);
            }
            let Some(name_at) = candidate else {
                self.problem(segment, DiagnosticReason::Unrecognized);
                return Ok((stop + 1).min(end));
            };
            let init = if self.is(i, "=") { i + 1 } else { i };
            let sig_end = self.start(i);
            while i < stop && !self.is(i, ",") {
                i = self.after(i, stop);
            }
            let child = self.item(
                Item {
                    local: self.text(name_at).to_string(),
                    symbol: constant.then_some(SymbolKind::Const),
                    first,
                    last: i.saturating_sub(1).max(name_at),
                    sig_end,
                    public,
                },
                scope,
            )?;
            self.signature_types(segment, name_at, &child)?;
            let mut at = init;
            while at < i {
                at = self.expression(at, i, &child)?;
            }
            i += 1;
        }
        Ok((stop + 1).min(end))
    }

    fn signature_types(&mut self, mut i: usize, end: usize, scope: &Scope) -> Result<(), LexError> {
        let mut default = false;
        while i < end {
            if self.is(i, ",") {
                default = false;
                i += 1;
                continue;
            }
            if self.is(i, "=") {
                default = true;
                i += 1;
                continue;
            }
            if matches!(self.text(i), "(" | "[" | "#[") {
                i = self.after(i, end);
                continue;
            }
            if !default && (self.kind(i) == Some(Kind::Ident) || self.is(i, "\\")) {
                let (name, next) = self.name(i, end);
                if !builtin(&name) {
                    self.reference(
                        scope,
                        EdgeKind::UsesType,
                        name,
                        None,
                        (i, Precision::Structural),
                    )?;
                }
                i = next.max(i + 1);
            } else {
                i += 1;
            }
        }
        Ok(())
    }
}

fn builtin(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "int"
            | "float"
            | "string"
            | "bool"
            | "array"
            | "object"
            | "mixed"
            | "void"
            | "never"
            | "null"
            | "false"
            | "true"
            | "callable"
            | "iterable"
            | "self"
            | "static"
            | "parent"
            | "public"
            | "private"
            | "protected"
            | "readonly"
    )
}
