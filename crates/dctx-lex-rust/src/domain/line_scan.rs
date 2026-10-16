//! Conservative line scan that recovers declarations from files syn refused.
//! Implements PARSERS-v1 "malformed syntax returns partial opaque module + diagnostics";
//! every fact is labelled `Precision::LineScan`. Pure, single pass over masked lines,
//! scopes tracked by delimiters; macro bodies never declare. Called by the extract pipeline.
use super::line_head::{self, Head};
use super::mask::{Masked, mask};
use super::sink::Sink;
use super::text::{first_doc_line, impl_scope, is_ident_char, is_test_path, qualify};
use super::{
    Coverage, Declaration, DeclarationKind, Extraction, Implementation, ImportKind, Limits,
    Precision, RawImport, Refusal, SourceSpan,
};

struct Frame {
    name: String,
    decl: Option<usize>,
    methods: bool,
    public_items: bool,
}

struct Delimiter {
    close: char,
    frame: bool,
    macro_body: bool,
}

struct Pending {
    frame: Frame,
    depth: usize,
    macro_body: bool,
}

struct Scan<'a> {
    sink: Sink,
    frames: Vec<Frame>,
    delimiters: Vec<Delimiter>,
    macro_bodies: usize,
    pending: Option<Pending>,
    docs: Vec<&'a str>,
    test: bool,
    bang: bool,
    after_ident: bool,
}

/// Declarations and file-module imports recoverable line by line, with `refusal` as reason.
pub(crate) fn extract(source: &str, limits: &Limits, refusal: Refusal) -> Extraction {
    let Masked { code, doc_starts } = mask(source);
    let mut scan = Scan {
        sink: Sink::new(limits),
        frames: Vec::new(),
        delimiters: Vec::new(),
        macro_bodies: 0,
        pending: None,
        docs: Vec::new(),
        test: false,
        bang: false,
        after_ident: false,
    };
    let mut offset = 0;
    let lines = code.split_inclusive('\n').zip(source.split_inclusive('\n'));
    for (index, (code, raw)) in lines.enumerate() {
        let number = u32::try_from(index + 1).unwrap_or(u32::MAX);
        scan.line(offset, number, code, raw, &doc_starts);
        offset += raw.len();
    }
    let module_doc = first_doc_line(
        source
            .lines()
            .map(str::trim)
            .take_while(|line| line.is_empty() || line.starts_with("//"))
            .filter_map(|line| line.strip_prefix("//!")),
    );
    scan.sink
        .finish(Coverage::Opaque, Some(refusal), module_doc)
}

impl<'a> Scan<'a> {
    fn line(&mut self, offset: usize, number: u32, code: &str, raw: &'a str, docs: &[usize]) {
        let trimmed = code.trim_start();
        if trimmed.trim_end().is_empty() {
            let text = raw.trim();
            let indent = raw.len() - raw.trim_start().len();
            if docs.binary_search(&(offset + indent)).is_ok() {
                self.docs.push(text.trim_start_matches("///"));
            } else if !text.is_empty() {
                self.docs.clear();
            }
            return;
        }
        let rest = self.attributes(trimmed);
        if !rest.trim_end().is_empty() {
            if self.macro_bodies == 0
                && let Some(head) = line_head::parse(rest)
            {
                let start = offset + (code.len() - rest.len());
                let span = SourceSpan {
                    start,
                    end: offset + raw.trim_end().len(),
                    lines: [number, number],
                };
                self.record(head, span);
            }
            self.docs.clear();
            self.test = false;
        }
        self.delimiters(code);
    }

    /// Strips leading `#[...]` / `#![...]` groups, noting test attributes.
    fn attributes<'l>(&mut self, mut rest: &'l str) -> &'l str {
        while let Some(inner) = rest.strip_prefix("#[").or_else(|| rest.strip_prefix("#![")) {
            let mut depth = 1_usize;
            let Some(end) = inner.char_indices().find_map(|(i, c)| {
                match c {
                    '[' => depth += 1,
                    ']' => depth -= 1,
                    _ => {}
                }
                (depth == 0).then_some(i)
            }) else {
                return rest;
            };
            let path: String = inner[..end]
                .split(['(', '=', ']'])
                .next()
                .unwrap_or_default()
                .split_whitespace()
                .collect();
            if rest.starts_with("#[") && is_test_path(&path) {
                self.test = true;
            }
            rest = inner[end + 1..].trim_start();
        }
        rest
    }

    fn record(&mut self, head: Head, span: SourceSpan) {
        let scope = self.frames.last();
        let scope_name = scope.map_or(String::new(), |f| f.name.clone());
        let methods = scope.is_some_and(|f| f.methods);
        let public_items = scope.is_some_and(|f| f.public_items);
        let parent = scope.and_then(|f| f.decl);
        let import = |kind, path, alias, public| RawImport {
            kind,
            path,
            alias,
            public,
            scope: scope_name.clone(),
            span,
            precision: Precision::LineScan,
        };
        let (kind, name, public, implementation) = match head {
            Head::ModFile { name, public } => {
                let import = import(ImportKind::ModFile, name, None, public);
                return self.sink.import(import);
            }
            Head::ExternCrate {
                name,
                alias,
                public,
            } => {
                let import = import(ImportKind::ExternCrate, name, alias, public);
                return self.sink.import(import);
            }
            Head::Item {
                kind,
                name,
                public,
                implementation,
            } => (kind, name, public, implementation),
        };
        let kind = match kind {
            DeclarationKind::Function if methods => DeclarationKind::Method,
            other => other,
        };
        let public = public || (methods && public_items);
        let name = qualify(&scope_name, &name);
        let child = match &implementation {
            Some((self_type, _)) => qualify(&scope_name, impl_scope(self_type)),
            None => name.clone(),
        };
        let function = matches!(kind, DeclarationKind::Function | DeclarationKind::Method);
        let decl = self.sink.declare(Declaration {
            kind,
            name,
            parent,
            span,
            public,
            test: self.test && function,
            doc: first_doc_line(self.docs.iter().copied()),
            implementation: implementation.map(|(self_type, trait_path)| Implementation {
                self_type,
                trait_path,
            }),
            precision: Precision::LineScan,
        });
        self.pending = Some(Pending {
            frame: Frame {
                name: child,
                decl,
                methods: matches!(kind, DeclarationKind::Impl | DeclarationKind::Trait),
                public_items: kind == DeclarationKind::Trait && public,
            },
            depth: self.delimiters.len(),
            macro_body: kind == DeclarationKind::Macro,
        });
    }

    fn delimiters(&mut self, code: &str) {
        for c in code.chars() {
            match c {
                '{' | '(' | '[' => self.open(c),
                '}' | ')' | ']' => self.close(c),
                ';' if self
                    .pending
                    .as_ref()
                    .is_some_and(|p| p.depth == self.delimiters.len()) =>
                {
                    self.pending = None;
                }
                _ => {}
            }
            if c == '!' {
                self.bang = self.after_ident;
            } else if !c.is_whitespace() {
                self.bang = false;
            }
            self.after_ident = is_ident_char(c);
        }
    }

    fn open(&mut self, c: char) {
        let depth = self.delimiters.len();
        let mut macro_body = self.bang;
        let mut frame = false;
        if c == '{'
            && self.pending.as_ref().is_some_and(|p| p.depth == depth)
            && let Some(pending) = self.pending.take()
        {
            macro_body |= pending.macro_body;
            self.frames.push(pending.frame);
            frame = true;
        }
        if macro_body {
            self.macro_bodies += 1;
        }
        let close = match c {
            '{' => '}',
            '(' => ')',
            _ => ']',
        };
        self.delimiters.push(Delimiter {
            close,
            frame,
            macro_body,
        });
    }

    fn close(&mut self, c: char) {
        let Some(at) = self.delimiters.iter().rposition(|d| d.close == c) else {
            return;
        };
        for closed in self.delimiters.split_off(at) {
            if closed.frame {
                self.frames.pop();
            }
            if closed.macro_body {
                self.macro_bodies -= 1;
            }
        }
        if self.pending.as_ref().is_some_and(|p| p.depth > self.delimiters.len()) {
            self.pending = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(source: &str) -> Vec<(DeclarationKind, String)> {
        extract(source, &Limits::default(), Refusal::Syntax { line: 1 })
            .declarations
            .into_iter()
            .map(|d| (d.kind, d.name))
            .collect()
    }

    #[test]
    fn scopes_follow_braces_and_methods_know_their_owner() {
        use DeclarationKind::*;
        let source = "pub trait Work {\n    fn run(&self);\n}\nimpl Work for W {\n    fn run(&self) {\n        struct Inner;\n    }\n}\nfn broken( {\n";
        assert_eq!(
            names(source),
            [
                (Trait, "Work".into()),
                (Method, "Work::run".into()),
                (Impl, "W::impl<Work>".into()),
                (Method, "W::run".into()),
                (Struct, "W::run::Inner".into()),
                (Function, "broken".into()),
            ]
        );
    }

    #[test]
    fn macro_bodies_strings_and_comments_never_declare() {
        let source = "macro_rules! make {\n    () => { fn phantom() {} };\n}\nthread_local! {\n    static KEY: u8 = 0;\n}\nconst S: &str = \"\nfn fake() {}\n\";\n/*\nfn hidden() {}\n*/\nfn real() {\n";
        let found = names(source);
        let found: Vec<&str> = found.iter().map(|(_, n)| n.as_str()).collect();
        assert_eq!(found, ["make", "S", "real"]);
    }

    #[test]
    fn docs_tests_and_labels_are_kept() {
        let source = "//! Module doc.\n/// Does work.\n#[tokio::test]\nasync fn checks() {\nmod support;\n";
        let facts = extract(source, &Limits::default(), Refusal::Syntax { line: 4 });
        assert_eq!(facts.module_doc.as_deref(), Some("Module doc."));
        let decl = &facts.declarations[0];
        assert!(decl.test);
        assert_eq!(decl.doc.as_deref(), Some("Does work."));
        assert_eq!(decl.precision, Precision::LineScan);
        assert_eq!(decl.span.lines, [4, 4]);
        assert_eq!(&source[decl.span.start..decl.span.end], "async fn checks() {");
        assert_eq!(facts.imports[0].path, "support");
        assert_eq!(facts.imports[0].scope, "checks");
        assert_eq!(facts.coverage, Coverage::Opaque);
    }
}
