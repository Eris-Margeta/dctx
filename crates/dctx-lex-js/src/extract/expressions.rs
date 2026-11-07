// SPDX-License-Identifier: Apache-2.0
//! Expressions: groups, arrows, function and class expressions, templates and ASI ends.
//! Implements the PARSERS-v1 rule that calls and declarations inside templates, JSX
//! containers and arrow bodies are kept while unknown shapes are skipped token by token.
//! Uses calls, classes and types; called from statements and member walkers.
use super::{Scope, Walker};
use crate::lexer::Kind;

impl Walker<'_> {
    pub fn expr(&mut self, begin: usize, end: usize, scope: &Scope) {
        if self.enter(begin) {
            let mut i = begin;
            while i < end && self.ok() {
                i = self.expr_token(i, end, scope).max(i + 1);
            }
        }
        self.leave();
    }

    /// Handles the token at `i` and returns the next unhandled index.
    pub fn expr_token(&mut self, i: usize, end: usize, scope: &Scope) -> usize {
        match self.kind(i) {
            Some(Kind::Punct) => match self.text(i) {
                "(" => self.paren(i, end, scope),
                "[" | "{" => {
                    let close = self.close(i, end);
                    if self.is(i, "[") {
                        self.expr(i + 1, close, scope);
                    } else if i > 0 && self.is(i - 1, "=>") {
                        self.statements(i + 1, close, scope);
                    } else {
                        self.object_literal(i + 1, close, scope);
                    }
                    (close + 1).min(end)
                }
                "<" if self.ts => match self.function_shape(i, end) {
                    Some(shape) => {
                        self.walk_function(&shape, scope);
                        shape.end
                    }
                    None => i + 1,
                },
                _ => i + 1,
            },
            Some(Kind::JsxOpen) => {
                let close = self.close(i, end);
                self.expr(i + 1, close, scope);
                (close + 1).min(end)
            }
            Some(Kind::TemplateHead) => self.template(i, end, scope),
            Some(Kind::Ident) => self.word_token(i, end, scope),
            _ => i + 1,
        }
    }

    fn word_token(&mut self, i: usize, end: usize, scope: &Scope) -> usize {
        match self.text(i) {
            "function" | "async" => {
                if let Some(shape) = self.function_shape(i, end) {
                    self.walk_function(&shape, scope);
                    return shape.end;
                }
            }
            "class" => return self.class_expression(i, end, scope),
            "as" | "satisfies" if self.ts && i > 0 && self.ends_expr(i - 1) => {
                return self.scan_type(i + 1, end, scope);
            }
            _ => {}
        }
        if self.ts
            && self.is(i + 1, "<")
            && let Some(close) = self.angle_end(i + 1, end)
            && (self.is(close + 1, "(")
                || matches!(
                    self.kind(close + 1),
                    Some(Kind::Template | Kind::TemplateHead)
                ))
        {
            self.type_args(i + 1, end, scope);
            self.angles.insert(close, i + 1);
            return close + 1;
        }
        i + 1
    }

    fn paren(&mut self, i: usize, end: usize, scope: &Scope) -> usize {
        let close = self.close(i, end);
        if self.call_position(i) {
            return self.call(i, close, end, scope);
        }
        if let Some(arrow) = self.arrow_after(close, end) {
            self.params(i + 1, close, scope);
            if self.ts && self.is(close + 1, ":") {
                self.scan_type(close + 2, end, scope);
            }
            return arrow;
        }
        self.expr(i + 1, close, scope);
        (close + 1).min(end)
    }

    fn template(&mut self, i: usize, end: usize, scope: &Scope) -> usize {
        let mut open = i;
        loop {
            let close = self.close(open, end);
            self.expr(open + 1, close, scope);
            if close >= end {
                return end;
            }
            if self.kind(close) == Some(Kind::TemplateMiddle) {
                open = close;
                continue;
            }
            return close + 1;
        }
    }

    pub fn template_end(&self, i: usize, end: usize) -> usize {
        let mut open = i;
        loop {
            let close = self.close(open, end);
            if close >= end {
                return end;
            }
            if self.kind(close) == Some(Kind::TemplateMiddle) {
                open = close;
                continue;
            }
            return close + 1;
        }
    }

    /// Index of `=>` when the parameter group closed at `close` begins an arrow.
    pub fn arrow_after(&mut self, close: usize, end: usize) -> Option<usize> {
        if close >= end {
            return None;
        }
        if self.is(close + 1, "=>") {
            return Some(close + 1);
        }
        if self.ts && self.is(close + 1, ":") {
            let after = self.type_end(close + 2, end);
            return self.is(after, "=>").then_some(after);
        }
        None
    }

    /// First index after the assignment expression starting at `i`.
    pub fn expr_end(&self, i: usize, end: usize) -> usize {
        let mut k = i;
        let mut ternary = 0_u32;
        while k < end {
            if k > i && self.asi(k) {
                return k;
            }
            match self.kind(k) {
                Some(Kind::Punct) => match self.text(k) {
                    "," | ";" | ")" | "]" | "}" => return k,
                    "?" => ternary += 1,
                    ":" if ternary == 0 => return k,
                    ":" => ternary -= 1,
                    "(" | "[" | "{" => {
                        k = self.after(k, end);
                        continue;
                    }
                    "<" if self.ts => {
                        if let Some(close) = self.angle_end(k, end) {
                            k = close + 1;
                            continue;
                        }
                    }
                    _ => {}
                },
                Some(Kind::JsxClose | Kind::TemplateMiddle | Kind::TemplateTail) => return k,
                Some(Kind::JsxOpen) => {
                    k = self.after(k, end);
                    continue;
                }
                Some(Kind::TemplateHead) => {
                    k = self.template_end(k, end);
                    continue;
                }
                _ => {}
            }
            k += 1;
        }
        end
    }

    /// Automatic semicolon insertion before token `k`.
    pub fn asi(&self, k: usize) -> bool {
        k > 0 && self.nl(k) && self.ends_expr(k - 1) && !self.continues(k)
    }

    pub fn ends_expr(&self, j: usize) -> bool {
        match self.kind(j) {
            Some(Kind::Ident) => !matches!(
                self.text(j),
                "return"
                    | "typeof"
                    | "void"
                    | "delete"
                    | "new"
                    | "await"
                    | "yield"
                    | "in"
                    | "of"
                    | "instanceof"
                    | "as"
                    | "satisfies"
                    | "extends"
                    | "case"
                    | "throw"
                    | "else"
                    | "do"
                    | "keyof"
                    | "export"
                    | "default"
                    | "import"
            ),
            Some(
                Kind::Number
                | Kind::Str
                | Kind::BadStr
                | Kind::Template
                | Kind::TemplateTail
                | Kind::Regex
                | Kind::Private
                | Kind::Jsx,
            ) => true,
            Some(Kind::Punct) => matches!(self.text(j), ")" | "]" | "}" | "++" | "--"),
            _ => false,
        }
    }

    fn continues(&self, k: usize) -> bool {
        match self.kind(k) {
            Some(Kind::Punct) => !matches!(
                self.text(k),
                "{" | "!" | "~" | "++" | "--" | "@" | "#" | "..."
            ),
            Some(Kind::Ident) => {
                matches!(self.text(k), "in" | "instanceof" | "as" | "satisfies")
            }
            Some(Kind::Template | Kind::TemplateHead) => true,
            _ => false,
        }
    }

    /// Splits `begin..end` at top-level commas.
    pub fn segments(&self, begin: usize, end: usize) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        let mut start = begin;
        let mut k = begin;
        while k < end {
            match self.kind(k) {
                Some(Kind::Punct) if self.text(k) == "," => {
                    out.push((start, k));
                    start = k + 1;
                }
                Some(Kind::Punct) if matches!(self.text(k), "(" | "[" | "{") => {
                    k = self.after(k, end);
                    continue;
                }
                Some(Kind::JsxOpen) => {
                    k = self.after(k, end);
                    continue;
                }
                Some(Kind::TemplateHead) => {
                    k = self.template_end(k, end);
                    continue;
                }
                _ => {}
            }
            k += 1;
        }
        if start < end {
            out.push((start, end));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use crate::JsLexer;
    use dctx_map_model::EdgeKind;

    fn calls(path: &str, source: &str) -> Vec<(String, String)> {
        let facts = JsLexer::default().extract(path, source.as_bytes()).unwrap();
        facts
            .references
            .iter()
            .filter(|r| r.fact.kind == EdgeKind::Calls)
            .map(|r| (r.fact.scope.clone(), r.fact.name.clone()))
            .collect()
    }

    #[test]
    fn templates_and_arrow_bodies_keep_their_calls() {
        let source = "const f = (a) => `x${g(`y${h()}`)}z`;\nconst k = async x => x.run();";
        assert_eq!(
            calls("x.js", source),
            [
                ("f".into(), "g".into()),
                ("f".into(), "h".into()),
                ("k".into(), "x.run".into())
            ]
        );
    }

    #[test]
    fn regex_and_strings_never_create_calls_or_declarations() {
        let source = "const r = /function f\\(\\)/g; const s = 'g()'; t(r.test(s) / 2);";
        assert_eq!(
            calls("x.js", source),
            [
                (String::new(), "t".into()),
                (String::new(), "r.test".into())
            ]
        );
    }
}
