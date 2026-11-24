// SPDX-License-Identifier: Apache-2.0
//! Static ES module syntax: `import` bindings and `export` clauses as raw references.
//! Implements SPEC-MAP-MODE 5.3 imports/reexports with dctx-map-parse spellings
//! (`module::name`, namespace aliases, side-effect imports); `import x = ..` abstains.
//! Called from statements; dynamic `import()` is an expression and yields nothing.
use super::{Scope, Walker};
use crate::lexer::Kind;
use crate::{RawReference, domain::Precision};
use dctx_map_model::EdgeKind;

type Binding = (String, Option<String>, (usize, usize));

impl Walker<'_> {
    pub fn import(&mut self, i: usize, end: usize, scope: &Scope) -> usize {
        let mut j = i + 1;
        if (self.is(j, "type") || self.is(j, "typeof"))
            && !self.is(j + 1, "from")
            && !self.is(j + 1, ",")
            && !self.is(j + 1, "=")
        {
            j += 1;
        }
        if self.kind(j) == Some(Kind::Str) {
            let last = self.module_tail(j + 1, end);
            let module = self.module_literal(j);
            self.emit_modules(i, last, scope, EdgeKind::Imports, module, Vec::new());
            return last + 1;
        }
        let mut bindings: Vec<Binding> = Vec::new();
        loop {
            if self.ident(j) && !self.is(j, "from") {
                if self.is(j + 1, "=") {
                    if self.ts
                        && self.is(j + 2, "require")
                        && self.is(j + 3, "(")
                        && self.is(j + 5, ")")
                        && let Some(module) = self.module_literal(j + 4)
                    {
                        let last = self.module_tail(j + 6, end);
                        self.refer(
                            0,
                            scope,
                            EdgeKind::Imports,
                            format!("import = {}", RawReference::module_name(&module, None)),
                            Some(self.text(j).to_owned()),
                            (i, last),
                            (i, last),
                            Precision::Structural,
                        );
                        return last + 1;
                    }
                    return self.statement_end(j, end);
                }
                bindings.push(("default".into(), Some(self.text(j).into()), (j, j)));
                j += 1;
            } else if self.is(j, "*") && self.is(j + 1, "as") {
                bindings.push(("*".into(), Some(self.text(j + 2).into()), (j, j + 2)));
                j += 3;
            } else if self.is(j, "{") {
                let close = self.close(j, end);
                self.specifiers(j + 1, close, &mut bindings);
                j = self.after(j, end);
            } else {
                break;
            }
            j += usize::from(self.is(j, ","));
        }
        if !self.is(j, "from") {
            return j.max(i + 1);
        }
        let module = self.module_literal(j + 1);
        let last = self.module_tail(j + 2, end);
        self.emit_modules(i, last, scope, EdgeKind::Imports, module, bindings);
        last + 1
    }

    pub fn export_clause(&mut self, i: usize, end: usize, scope: &Scope) -> usize {
        let mut j = i + 1 + usize::from(self.is(i + 1, "type"));
        let mut bindings: Vec<Binding> = Vec::new();
        if self.is(j, "*") {
            if self.is(j + 1, "as") {
                bindings.push(("*".into(), Some(self.text(j + 2).into()), (j, j + 2)));
                j += 3;
            } else {
                j += 1;
            }
        } else {
            let close = self.close(j, end);
            self.specifiers(j + 1, close, &mut bindings);
            j = self.after(j, end);
        }
        let mut module = None;
        if self.is(j, "from") && self.kind(j + 1) == Some(Kind::Str) {
            module = self.module_literal(j + 1);
            j += 2;
        }
        let last = self.module_tail(j, end);
        self.emit_modules(i, last, scope, EdgeKind::Reexports, module, bindings);
        last + 1
    }

    fn specifiers(&self, begin: usize, close: usize, bindings: &mut Vec<Binding>) {
        for (start, stop) in self.segments(begin, close) {
            let mut k = start;
            if self.is(k, "type") && k + 1 < stop && !self.is(k + 1, "as") {
                k += 1;
            }
            let name = match self.kind(k) {
                Some(Kind::Str) => self.content(k),
                Some(Kind::Ident) => self.text(k).to_owned(),
                _ => continue,
            };
            let alias = self.is(k + 1, "as").then(|| self.text(k + 2).to_owned());
            bindings.push((name, alias, (start, stop - 1)));
        }
    }

    /// Last token of an import/export statement: attributes and `;` included.
    fn module_tail(&self, mut j: usize, end: usize) -> usize {
        if (self.is(j, "with") || self.is(j, "assert")) && self.is(j + 1, "{") && !self.nl(j) {
            j = self.after(j + 1, end);
        }
        if self.is(j, ";") {
            j
        } else {
            j.saturating_sub(1)
        }
    }

    fn emit_modules(
        &mut self,
        first: usize,
        last: usize,
        scope: &Scope,
        kind: EdgeKind,
        module: Option<String>,
        bindings: Vec<Binding>,
    ) {
        let key = (first, last);
        let all_types = self.is(first + 1, "type") && !self.is(first + 2, "from");
        if bindings.is_empty() {
            if let Some(module) = module {
                let target = RawReference::module_name(
                    &module,
                    (kind == EdgeKind::Reexports).then_some("*"),
                );
                self.refer(
                    0,
                    scope,
                    kind,
                    target,
                    None,
                    key,
                    key,
                    Precision::Structural,
                );
            }
            return;
        }
        for (name, alias, site) in bindings {
            let target = match &module {
                Some(module) => RawReference::module_name(
                    module,
                    (!(name == "*" && alias.is_some())).then_some(name.as_str()),
                ),
                None => name,
            };
            let target = if kind == EdgeKind::Imports
                && (all_types
                    || (site.0 < site.1 && self.is(site.0, "type") && !self.is(site.0 + 1, "as")))
            {
                format!("import type {target}")
            } else {
                target
            };
            self.refer(
                0,
                scope,
                kind,
                target,
                alias,
                site,
                key,
                Precision::Structural,
            );
        }
    }
}

impl crate::JsLexer {
    /// Read a literal boolean compiler option from a validated TypeScript JSON manifest.
    #[must_use]
    pub fn compiler_option(&self, source: &str, option: &str) -> Option<bool> {
        if source.len() > self.limits.max_bytes {
            return None;
        }
        let lexed = crate::lexer::lex(source, crate::Dialect::JavaScript, &self.limits).ok()?;
        let word = |i: usize| {
            lexed
                .tokens
                .get(i)
                .map(|t| &source[t.start as usize..t.end as usize])
        };
        let field = |object: usize, key: &str| {
            if word(object)? != "{" {
                return None;
            }
            let end = *lexed.partner.get(object)? as usize;
            let mut i = object + 1;
            let mut found = None;
            while i < end {
                let spelling = RawReference::string_literal(word(i)?)?;
                if word(i + 1)? != ":" {
                    return None;
                }
                let value = i + 2;
                if spelling == key && found.replace(value).is_some() {
                    return None;
                }
                i = match word(value)? {
                    "[" | "{" => (*lexed.partner.get(value)? as usize).checked_add(1)?,
                    _ => value + 1,
                };
                if i < end {
                    if word(i)? != "," {
                        return None;
                    }
                    i += 1;
                }
            }
            found
        };
        match word(field(field(0, "compilerOptions")?, option)?)? {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        }
    }
}
#[cfg(test)]
mod tests {
    use crate::JsLexer;
    use dctx_map_model::EdgeKind;

    fn refs(path: &str, source: &str) -> Vec<(EdgeKind, String, Option<String>, u32)> {
        let facts = JsLexer::default().extract(path, source.as_bytes()).unwrap();
        facts
            .references
            .iter()
            .map(|r| {
                (
                    r.fact.kind,
                    r.fact.name.clone(),
                    r.fact.alias.clone(),
                    r.fact.lines[0],
                )
            })
            .collect()
    }

    #[test]
    fn import_forms_and_reexports_match_the_oracle_spelling() {
        let source = "import d, * as ns from 'a';\nimport {\n  x,\n  type y as z,\n} from \"b\" with { type: 'json' };\nimport 'c';\nexport * as all from 'e';\nexport { local as default };\nimport q = require('f');\nconst m = import('g');";
        assert_eq!(
            refs("m.ts", source),
            [
                (EdgeKind::Imports, "a::default".into(), Some("d".into()), 1),
                (EdgeKind::Imports, "a".into(), Some("ns".into()), 1),
                (EdgeKind::Imports, "b::x".into(), None, 3),
                (EdgeKind::Imports, "b::y".into(), Some("z".into()), 4),
                (EdgeKind::Imports, "c".into(), None, 6),
                (EdgeKind::Reexports, "e".into(), Some("all".into()), 7),
                (
                    EdgeKind::Reexports,
                    "local".into(),
                    Some("default".into()),
                    8
                ),
                (EdgeKind::Imports, "f".into(), Some("q".into()), 9),
                (EdgeKind::Imports, "g".into(), None, 10),
            ]
        );
    }
}
