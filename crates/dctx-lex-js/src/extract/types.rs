//! TypeScript type expressions: references, generics, function/object/tuple types.
//! Implements SPEC-MAP-MODE 5.3 `uses-type` spellings as dctx-map-parse emits them
//! (qualified names compacted, predefined types and declared names excluded).
//! Dry scans (`dry > 0`) only measure extent; called by every annotation site.
use super::{Scope, Walker};
use crate::domain::{Precision, compact};
use crate::lexer::Kind;
use dctx_map_model::EdgeKind;

const PREDEFINED: [&str; 16] = [
    "any", "number", "boolean", "string", "symbol", "void", "unknown", "never", "object",
    "bigint", "undefined", "null", "true", "false", "this", "const",
];

impl Walker<'_> {
    /// Scans one type from `i`; returns the first index after it.
    pub fn scan_type(&mut self, i: usize, end: usize, scope: &Scope) -> usize {
        let mut k = i;
        if self.enter(i) {
            let mut operand = true;
            let mut pending = 0_u32;
            while k < end && self.ok() {
                let next = if operand {
                    self.type_operand(k, end, scope, &mut operand)
                } else {
                    self.type_operator(k, end, scope, &mut operand, &mut pending)
                };
                match next {
                    Some(next) => k = next.max(k + 1),
                    None => break,
                }
            }
        }
        self.leave();
        k.min(end)
    }

    #[allow(clippy::too_many_lines, reason = "Keep the ordered TypeScript type-operand grammar dispatch and its shared cursor updates together.")]
    fn type_operand(
        &mut self,
        k: usize,
        end: usize,
        scope: &Scope,
        operand: &mut bool,
    ) -> Option<usize> {
        let text = self.text(k);
        match self.kind(k)? {
            Kind::Ident => {
                if self.is(k + 1, "is") && !self.nl(k + 1) {
                    return Some(k + 2);
                }
                match text {
                    "asserts" if self.ident(k + 1) && !self.is(k + 2, "is") => {
                        *operand = false;
                        Some(k + 2)
                    }
                    "keyof" | "readonly" | "unique" | "infer" | "new" | "abstract" | "asserts" => Some(k + 1),
                    "typeof" => {
                        *operand = false;
                        let mut j = k + 2;
                        while self.is(j, ".") && self.ident(j + 1) {
                            j += 2;
                        }
                        if self.is(j, "<")
                            && let Some(close) = self.angle_end(j, end)
                        {
                            j = close + 1;
                        }
                        Some(j)
                    }
                    "import" if self.is(k + 1, "(") => {
                        *operand = false;
                        let mut j = self.after(k + 1, end);
                        while self.is(j, ".") && self.ident(j + 1) {
                            j += 2;
                        }
                        Some(if self.is(j, "<") { self.type_args(j, end, scope) } else { j })
                    }
                    word if PREDEFINED.contains(&word) => {
                        *operand = false;
                        Some(k + 1)
                    }
                    _ => {
                        *operand = false;
                        let mut j = k + 1;
                        while self.is(j, ".") && self.ident(j + 1) {
                            j += 2;
                        }
                        let site = (k, j - 1);
                        let name = compact(
                            &self.src[self.start_byte(k) as usize..self.end_byte(j - 1) as usize],
                        );
                        self.refer(scope.owner, scope, EdgeKind::UsesType, name, None, site, site, Precision::Structural);
                        Some(if self.is(j, "<") { self.type_args(j, end, scope) } else { j })
                    }
                }
            }
            Kind::Str | Kind::Number | Kind::BadStr | Kind::Template => {
                *operand = false;
                Some(k + 1)
            }
            Kind::TemplateHead => {
                *operand = false;
                let mut open = k;
                loop {
                    let close = self.close(open, end);
                    self.scan_type(open + 1, close, scope);
                    if close >= end {
                        return Some(end);
                    }
                    if self.kind(close) == Some(Kind::TemplateMiddle) {
                        open = close;
                        continue;
                    }
                    return Some(close + 1);
                }
            }
            Kind::Punct => match text {
                "|" | "&" | "-" | "..." => Some(k + 1),
                "(" => {
                    *operand = false;
                    let close = self.close(k, end);
                    if self.is(close + 1, "=>") {
                        self.params(k + 1, close, scope);
                        Some(self.scan_type(close + 2, end, scope))
                    } else {
                        self.scan_type(k + 1, close, scope);
                        Some((close + 1).min(end))
                    }
                }
                "<" => Some(self.type_params(k, end, scope)),
                "{" | "[" => {
                    *operand = false;
                    let close = self.close(k, end);
                    if text == "{" {
                        self.type_body(k + 1, close, scope);
                    } else {
                        self.tuple(k + 1, close, scope);
                    }
                    Some((close + 1).min(end))
                }
                _ => None,
            },
            _ => None,
        }
    }

    fn type_operator(
        &mut self,
        k: usize,
        end: usize,
        scope: &Scope,
        operand: &mut bool,
        pending: &mut u32,
    ) -> Option<usize> {
        let text = self.text(k);
        if self.nl(k) && !matches!(text, "|" | "&" | "extends" | "?" | ":") {
            return None;
        }
        match (self.kind(k)?, text) {
            (Kind::Punct, "|" | "&") => {
                *operand = true;
                Some(k + 1)
            }
            (Kind::Punct, "[") => {
                if self.is(k + 1, "]") {
                    return Some(k + 2);
                }
                let close = self.close(k, end);
                self.scan_type(k + 1, close, scope);
                Some((close + 1).min(end))
            }
            (Kind::Ident, "extends") => {
                *pending += 1;
                *operand = true;
                Some(k + 1)
            }
            (Kind::Punct, "?") if *pending > 0 => {
                *operand = true;
                Some(k + 1)
            }
            (Kind::Punct, ":") if *pending > 0 => {
                *pending -= 1;
                *operand = true;
                Some(k + 1)
            }
            _ => None,
        }
    }

    fn tuple(&mut self, begin: usize, end: usize, scope: &Scope) {
        for (start, stop) in self.segments(begin, end) {
            let mut k = start + usize::from(self.is(start, "..."));
            if self.ident(k) && (self.is(k + 1, ":") || (self.is(k + 1, "?") && self.is(k + 2, ":"))) {
                k += if self.is(k + 1, ":") { 2 } else { 3 };
            }
            self.scan_type(k, stop, scope);
        }
    }

    /// `<T extends A = B, ..>`: names are declarations, constraints are references.
    pub fn type_params(&mut self, open: usize, end: usize, scope: &Scope) -> usize {
        let mut k = open + 1;
        while k < end && self.ok() {
            if self.is(k, ">") {
                return k + 1;
            }
            if self.is(k, ",") {
                k += 1;
                continue;
            }
            if matches!(self.text(k), "const" | "in" | "out") && self.ident(k + 1) {
                k += 1;
            }
            if !self.ident(k) {
                return k;
            }
            k += 1;
            if self.is(k, "extends") {
                k = self.scan_type(k + 1, end, scope);
            }
            if self.is(k, "=") {
                k = self.scan_type(k + 1, end, scope);
            }
            if !self.is(k, ",") && !self.is(k, ">") {
                return k;
            }
        }
        k.min(end)
    }

    /// `<A, B>` type arguments; returns the index after `>`.
    pub fn type_args(&mut self, open: usize, end: usize, scope: &Scope) -> usize {
        let mut k = open + 1;
        while k < end && self.ok() {
            if self.is(k, ">") {
                return k + 1;
            }
            let next = self.scan_type(k, end, scope);
            if self.is(next, ",") {
                k = next + 1;
            } else if self.is(next, ">") {
                return next + 1;
            } else {
                return next.max(k + 1);
            }
        }
        k.min(end)
    }

    pub fn type_end(&mut self, i: usize, end: usize) -> usize {
        self.dry += 1;
        let stop = self.scan_type(i, end, &Scope::default());
        self.dry -= 1;
        stop
    }

    pub fn type_params_end(&mut self, open: usize, end: usize) -> usize {
        self.dry += 1;
        let stop = self.type_params(open, end, &Scope::default());
        self.dry -= 1;
        stop
    }

    /// Lexical `<..>` match whose contents can only be types; `None` for comparisons.
    pub fn angle_end(&self, open: usize, end: usize) -> Option<usize> {
        let mut depth = 0_u32;
        let mut k = open;
        while k < end {
            match self.kind(k)? {
                Kind::Punct => match self.text(k) {
                    "<" => depth += 1,
                    ">" => {
                        depth -= 1;
                        if depth == 0 {
                            return Some(k);
                        }
                    }
                    "(" | "[" | "{" => {
                        k = self.after(k, end);
                        continue;
                    }
                    "," | "." | "|" | "&" | "=>" | ":" | "?" | "-" | "..." | "=" | "?." => {}
                    _ => return None,
                },
                Kind::Ident | Kind::Str | Kind::Number | Kind::Template => {}
                Kind::TemplateHead => {
                    k = self.template_end(k, end);
                    continue;
                }
                _ => return None,
            }
            k += 1;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use crate::JsLexer;
    use dctx_map_model::EdgeKind;

    fn types(source: &str) -> Vec<String> {
        let facts = JsLexer::default().extract("t.ts", source.as_bytes()).unwrap();
        facts
            .references
            .iter()
            .filter(|r| r.fact.kind == EdgeKind::UsesType)
            .map(|r| r.fact.name.clone())
            .collect()
    }

    #[test]
    fn predicates_queries_and_functions_follow_the_grammar() {
        let source = "function f(x: unknown, cb: (a: In) => Out): x is Guard { return g<Arg>(x as Cast, typeof y); }\nlet t: [label: L, ...R[]] = q satisfies S;";
        assert_eq!(
            types(source),
            ["In", "Out", "Guard", "Arg", "Cast", "L", "R", "S"]
        );
    }

    #[test]
    fn comparisons_are_not_generics_and_conditionals_continue() {
        let source = "if (a < b && c > d) {}\ntype C<T> = T extends A ? B : keyof D;";
        assert_eq!(types(source), ["T", "A", "B", "D"]);
    }
}
