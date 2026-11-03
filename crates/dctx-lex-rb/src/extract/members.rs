// SPDX-License-Identifier: Apache-2.0
//! Visibility, attributes, aliases and final fact assembly.
//! Implements Ruby's attribute and alias conventions as method symbols.
//! A dynamic name is skipped; the extractor does not call `define_method`.
use super::{assign_ordinals, labeled, qualify, references::symbol_name, Scope, Sym, Walker};
use crate::domain::{
    Completion, Diagnostic, Dialect, Labeled, LexError, LexedFile, Precision,
    RawReference,
};
use crate::lexer::{Kind, Lexed};
use dctx_map_model::{Confidence, Edge, EdgeKind, StableId, SymbolKind};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Attr {
    Reader,
    Writer,
    Accessor,
    Plain,
}

impl Walker<'_> {
    pub(super) fn modifier(&mut self, scope: &Scope) -> bool {
        if !matches!(
            self.cur(),
            "private" | "public" | "protected" | "module_function"
        ) {
            return false;
        }
        let word = self.cur().to_owned();
        let save = self.i;
        self.bump();
        if self.is_sep() || self.eof() || self.at_word("end") {
            self.set_vis(&word);
            if self.is_sep() {
                self.bump();
            }
            return true;
        }
        if self.at_word("def") {
            let vis = match word.as_str() {
                "private" => Some(super::Vis::Private),
                "protected" => Some(super::Vis::Protected),
                "public" => Some(super::Vis::Public),
                _ => None,
            };
            self.method_decl(scope, vis, word == "module_function");
            return true;
        }
        self.i = save;
        false
    }

    pub(super) fn attr_stmt(&mut self, scope: &Scope) -> bool {
        let kind = match self.cur() {
            "attr_accessor" => Attr::Accessor,
            "attr_reader" => Attr::Reader,
            "attr_writer" => Attr::Writer,
            "attr" => Attr::Plain,
            _ => return false,
        };
        let keyword = self.cur().to_owned();
        let start = self.tok_start();
        self.bump();
        if self.at_punct("(") {
            self.bump();
        }
        let mut last: Option<String> = None;
        while !self.eof() && !self.is_sep() && !self.at_word("end") {
            if self.at_punct(",") {
                self.bump();
                continue;
            }
            if self.at_punct(")") {
                self.bump();
                break;
            }
            if self.cur() == "true" && kind == Attr::Plain {
                if let Some(name) = last.clone() {
                    self.emit_attr(scope, &keyword, &name, Attr::Writer, start);
                }
                self.bump();
                continue;
            }
            if self.cur() == "false" && kind == Attr::Plain {
                self.bump();
                continue;
            }
            let Some(name) = self.attr_name() else {
                break;
            };
            self.emit_attr(scope, &keyword, &name, kind, start);
            last = Some(name);
        }
        self.scan_statement(scope);
        true
    }

    pub(super) fn alias_stmt(&mut self, scope: &Scope) -> bool {
        if self.at_word("alias") {
            let start = self.tok_start();
            self.bump();
            let new_name = self.alias_name();
            let _old = self.alias_name();
            self.emit_alias(scope, start, new_name, "alias");
            true
        } else if self.at_word("alias_method") {
            let start = self.tok_start();
            self.bump();
            if self.at_punct("(") {
                self.bump();
            }
            let new_name = self.alias_name();
            if self.at_punct(",") {
                self.bump();
            }
            let _old = self.alias_name();
            self.emit_alias(scope, start, new_name, "alias_method");
            true
        } else {
            false
        }
    }

    fn attr_name(&mut self) -> Option<String> {
        if self.at_kind(Kind::Symbol) {
            let name = symbol_name(self.cur())?;
            self.bump();
            return Some(name);
        }
        if self.at_kind(Kind::Ident) && !super::references::is_keyword(self.cur()) {
            let name = self.cur().to_owned();
            self.bump();
            return Some(name);
        }
        None
    }

    fn alias_name(&mut self) -> Option<String> {
        if self.at_kind(Kind::Symbol) {
            let name = symbol_name(self.cur());
            self.bump();
            return name;
        }
        if self.at_kind(Kind::Ident) && !super::references::is_keyword(self.cur()) {
            let name = self.cur().to_owned();
            self.bump();
            return Some(name);
        }
        None
    }

    fn emit_attr(&mut self, scope: &Scope, keyword: &str, name: &str, kind: Attr, start: u32) {
        let end = self.prev_end().max(start);
        let signature = format!("{keyword} :{name}");
        let write = name.strip_suffix('=').unwrap_or(name);
        let reader = !name.ends_with('=');
        if matches!(kind, Attr::Reader | Attr::Accessor | Attr::Plain) && reader {
            self.one_attr(scope, qualify(scope, name), signature.clone(), start, end);
        }
        if matches!(kind, Attr::Writer | Attr::Accessor) {
            let writer = if name.ends_with('=') {
                name.to_owned()
            } else {
                format!("{write}=")
            };
            self.one_attr(scope, qualify(scope, &writer), signature, start, end);
        }
    }

    fn one_attr(&mut self, scope: &Scope, name: String, signature: String, start: u32, end: u32) {
        let _ = self.emit_symbol(
            scope,
            Sym {
                name,
                symbol: SymbolKind::Method,
                signature,
                start,
                end,
                public: self.method_public(None, false),
                precision: Precision::Convention,
                doc: self.docs(start),
            },
        );
    }

    fn emit_alias(&mut self, scope: &Scope, start: u32, new_name: Option<String>, keyword: &str) {
        self.scan_statement(scope);
        let end = self.prev_end().max(start);
        if let Some(name) = new_name {
            let _ = self.emit_symbol(
                scope,
                Sym {
                    name: qualify(scope, &name),
                    symbol: SymbolKind::Method,
                    signature: collapse_alias(keyword, self.slice(start, end)),
                    start,
                    end,
                    public: self.method_public(None, false),
                    precision: Precision::Convention,
                    doc: self.docs(start),
                },
            );
        }
    }
}

impl Walker<'_> {
    pub(super) fn finish(mut self, lexed: &Lexed) -> Result<LexedFile, LexError> {
        if let Some(error) = self.refused {
            return Err(error);
        }
        assign_ordinals(&mut self.nodes, &self.path)?;
        let mut diagnostics = lexed.diagnostics.clone();
        diagnostics.extend(self.problems.iter().map(|&(byte, reason)| Diagnostic {
            line: lexed.line(byte),
            byte,
            reason,
        }));
        diagnostics.sort_by_key(|diagnostic| diagnostic.byte);
        let partial = !diagnostics.is_empty();
        diagnostics.truncate(LexedFile::MAX_DIAGNOSTICS);
        let first = diagnostics.first().map_or(u32::MAX, |diagnostic| diagnostic.byte);
        let ids: Vec<StableId> = self.nodes.iter().map(|draft| draft.node.id.clone()).collect();
        let parents: Vec<usize> = self.nodes.iter().map(|draft| draft.parent).collect();
        let path = self.path.clone();
        let edges = self
            .nodes
            .iter()
            .enumerate()
            .skip(1)
            .map(|(index, child)| {
                let parent = parents.get(index).copied().unwrap_or(0);
                let src = ids.get(parent).cloned().unwrap_or_else(|| ids[0].clone());
                Labeled {
                    precision: labeled(child.end, child.precision, first),
                    fact: Edge {
                        src,
                        dst: child.node.id.clone(),
                        kind: EdgeKind::Contains,
                        conf: Confidence::Exact,
                        owner_path: path.clone(),
                    },
                }
            })
            .collect();
        self.refs.sort_by_key(|reference| (reference.start, reference.end));
        let references = self
            .refs
            .iter()
            .map(|reference| {
                let src = ids
                    .get(reference.src)
                    .cloned()
                    .unwrap_or_else(|| ids[0].clone());
                Labeled {
                    precision: labeled(reference.end, reference.precision, first),
                    fact: RawReference {
                        src,
                        kind: reference.kind,
                        name: reference.name.clone(),
                        scope: reference.scope.clone(),
                        alias: None,
                        lines: self.span_lines(reference.start, reference.end),
                        owner_path: path.clone(),
                    },
                }
            })
            .collect();
        let nodes = self
            .nodes
            .into_iter()
            .enumerate()
            .map(|(index, draft)| Labeled {
                precision: if index == 0 {
                    Precision::Structural
                } else {
                    labeled(draft.end, draft.precision, first)
                },
                fact: draft.node,
            })
            .collect();
        Ok(LexedFile {
            path: self.path,
            dialect: Dialect::Ruby,
            completion: if partial {
                Completion::Partial
            } else {
                Completion::Complete
            },
            diagnostics,
            nodes,
            edges,
            references,
        })
    }
}

impl Walker<'_> {
    pub(super) fn bump(&mut self) {
        if self.i < self.toks.len() {
            self.i += 1;
        }
    }

    pub(super) fn eof(&self) -> bool {
        self.i >= self.toks.len()
    }

    pub(super) fn cur(&self) -> &str {
        self.text_at(self.i)
    }

    pub(super) fn text_at(&self, index: usize) -> &str {
        self.toks.get(index).map_or("", |token| {
            self.src
                .get(token.start as usize..token.end as usize)
                .unwrap_or("")
        })
    }

    pub(super) fn kind_at(&self, index: usize) -> Option<Kind> {
        self.toks.get(index).map(|token| token.kind)
    }

    pub(super) fn at_word(&self, word: &str) -> bool {
        self.kind_at(self.i) == Some(Kind::Ident) && self.cur() == word
    }

    pub(super) fn at_punct(&self, punct: &str) -> bool {
        self.kind_at(self.i) == Some(Kind::Punct) && self.cur() == punct
    }

    pub(super) fn at_kind(&self, kind: Kind) -> bool {
        self.kind_at(self.i) == Some(kind)
    }

    pub(super) fn is_sep(&self) -> bool {
        self.at_kind(Kind::Newline) || self.at_punct(";")
    }

    pub(super) fn skip_sep(&mut self) {
        while self.is_sep() {
            self.bump();
        }
    }

    pub(super) fn peek_punct(&self, punct: &str) -> bool {
        self.kind_at(self.i + 1) == Some(Kind::Punct) && self.text_at(self.i + 1) == punct
    }

    pub(super) fn tok_start(&self) -> u32 {
        self.toks
            .get(self.i)
            .map_or(u32::try_from(self.src.len()).unwrap_or(u32::MAX), |token| {
                token.start
            })
    }

    pub(super) fn tok_end(&self) -> u32 {
        self.toks
            .get(self.i)
            .map_or(u32::try_from(self.src.len()).unwrap_or(u32::MAX), |token| token.end)
    }

    pub(super) fn end_at(&self, index: usize) -> u32 {
        self.toks
            .get(index)
            .map_or(u32::try_from(self.src.len()).unwrap_or(u32::MAX), |token| token.end)
    }

    pub(super) fn prev_end(&self) -> u32 {
        if self.i == 0 {
            0
        } else {
            self.end_at(self.i - 1)
        }
    }

    pub(super) fn cursor_end(&self) -> u32 {
        if self.eof() {
            u32::try_from(self.src.len()).unwrap_or(u32::MAX)
        } else {
            self.prev_end()
        }
    }

    pub(super) fn slice(&self, start: u32, end: u32) -> &str {
        let last = u32::try_from(self.src.len()).unwrap_or(u32::MAX);
        let start = start.min(last) as usize;
        let end = end.min(last) as usize;
        if start > end || !self.src.is_char_boundary(start) || !self.src.is_char_boundary(end) {
            ""
        } else {
            self.src.get(start..end).unwrap_or("")
        }
    }

    pub(super) fn span_lines(&self, start: u32, end: u32) -> [u32; 2] {
        let last = self.file_lines.max(1);
        let left = self.lex.line(start).clamp(1, last);
        let at = end.saturating_sub(1).max(start);
        let right = self.lex.line(at).clamp(left, last);
        [left, right]
    }

    pub(super) fn here_line(&self) -> u32 {
        self.lex.line(self.tok_start()).max(1)
    }

    pub(super) fn is_const(&self) -> bool {
        self.kind_at(self.i) == Some(Kind::Ident)
            && self.cur().chars().next().is_some_and(|ch| ch.is_ascii_uppercase())
    }

    pub(super) fn block_word(&self) -> bool {
        !self.prev_member()
            && matches!(
                self.cur(),
                "class"
                    | "module"
                    | "def"
                    | "if"
                    | "unless"
                    | "while"
                    | "until"
                    | "for"
                    | "begin"
                    | "case"
                    | "do"
            )
            && self.kind_at(self.i) == Some(Kind::Ident)
    }

    pub(super) fn mid_keyword(&self) -> bool {
        !self.prev_member()
            && matches!(
                self.cur(),
                "else" | "elsif" | "when" | "rescue" | "ensure" | "then" | "in"
            )
            && self.kind_at(self.i) == Some(Kind::Ident)
    }

    pub(super) fn plain_string(&self) -> bool {
        self.toks
            .get(self.i)
            .is_some_and(|token| token.kind == Kind::Str && token.plain)
    }
}

fn collapse_alias(keyword: &str, body: &str) -> String {
    let text = crate::domain::collapse(body);
    if text.is_empty() {
        keyword.to_owned()
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use crate::RubyLexer;
    use dctx_map_model::SymbolKind;

    #[test]
    fn attr_accessor_emits_reader_and_writer() {
        let facts = RubyLexer::default()
            .extract("p.rb", b"class A\nattr_accessor :width\nend\n")
            .expect("extract");
        let names: Vec<_> = facts.nodes.iter().map(|node| node.fact.name.as_str()).collect();
        assert!(names.contains(&"A::width"), "{names:?}");
        assert!(names.contains(&"A::width="), "{names:?}");
        assert!(facts.nodes.iter().any(|node| {
            node.fact.name == "A::width" && node.fact.symbol_kind == Some(SymbolKind::Method)
        }));
    }
}
