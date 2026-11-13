// SPDX-License-Identifier: Apache-2.0
//! Recognises one declaration head at the start of a masked code line.
//! Implements the conservative recovery of PARSERS-v1 "malformed syntax returns partial".
//! Pure; the line scan supplies masked lines and tracks scopes around the result.
use super::DeclarationKind;
use super::text::{compact, is_ident_char, unraw};

/// What a line declares, before scope qualification.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Head {
    Item {
        kind: DeclarationKind,
        name: String,
        public: bool,
        /// Self type and trait text for `impl` heads.
        implementation: Option<(String, Option<String>)>,
    },
    ModFile {
        name: String,
        public: bool,
    },
    ExternCrate {
        name: String,
        alias: Option<String>,
        public: bool,
    },
}

const RESERVED: &[&str] = &[
    "as", "break", "const", "continue", "crate", "else", "enum", "extern", "false", "fn", "for",
    "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return",
    "self", "Self", "static", "struct", "super", "trait", "true", "type", "unsafe", "use", "where",
    "while", "async", "await", "dyn",
];

#[derive(Clone, Copy)]
struct Words<'a> {
    rest: &'a str,
}

impl<'a> Words<'a> {
    fn peek(&self) -> Option<&'a str> {
        let rest = self.rest.trim_start();
        let first = rest.chars().next()?;
        let raw = rest.starts_with("r#") && rest[2..].starts_with(is_ident_char);
        let len = if raw || is_ident_char(first) {
            let start = if raw { 2 } else { 0 };
            start
                + rest[start..]
                    .find(|c: char| !is_ident_char(c))
                    .unwrap_or(rest.len() - start)
        } else {
            first.len_utf8()
        };
        Some(&rest[..len])
    }

    fn next(&mut self) -> Option<&'a str> {
        let word = self.peek()?;
        let rest = self.rest.trim_start();
        self.rest = &rest[word.len()..];
        Some(word)
    }

    fn eat(&mut self, word: &str) -> bool {
        let found = self.peek() == Some(word);
        if found {
            self.next();
        }
        found
    }

    fn skip_group(&mut self, open: &str, close: &str) {
        if !self.eat(open) {
            return;
        }
        let mut depth = 1_usize;
        while depth > 0 {
            match self.next() {
                Some(w) if w == open => depth += 1,
                Some(w) if w == close => depth -= 1,
                Some(_) => {}
                None => return,
            }
        }
    }

    fn name(&mut self) -> Option<String> {
        let word = self.next()?;
        let valid = word.starts_with(is_ident_char)
            && !word.starts_with(|c: char| c.is_ascii_digit())
            && !RESERVED.contains(&word);
        valid.then(|| unraw(word).to_owned())
    }
}

/// `const` followed by `fn`, `unsafe`, `async` or `extern` is a function modifier.
fn qualifies_fn(mut words: Words<'_>) -> bool {
    words.next();
    matches!(words.peek(), Some("fn" | "unsafe" | "async" | "extern"))
}

pub(crate) fn parse(line: &str) -> Option<Head> {
    let mut words = Words { rest: line };
    let public = words.eat("pub");
    if public {
        words.skip_group("(", ")");
    }
    loop {
        match words.peek()? {
            "default" | "async" | "unsafe" | "safe" => {
                words.next();
            }
            "const" if qualifies_fn(words) => {
                words.next();
            }
            "extern" => {
                words.next();
                if words.eat("crate") {
                    let name = if words.eat("self") {
                        "self".into()
                    } else {
                        words.name()?
                    };
                    let alias = if words.eat("as") { words.name() } else { None };
                    return Some(Head::ExternCrate {
                        name,
                        alias,
                        public,
                    });
                }
                if words.eat("\"") {
                    while words.next().is_some_and(|w| w != "\"") {}
                }
            }
            _ => break,
        }
    }
    let keyword = words.next()?;
    let item = |kind, name| Head::Item {
        kind,
        name,
        public,
        implementation: None,
    };
    let kind = match keyword {
        "fn" => DeclarationKind::Function,
        "struct" => DeclarationKind::Struct,
        "enum" => DeclarationKind::Enum,
        "trait" => DeclarationKind::Trait,
        "type" => DeclarationKind::TypeAlias,
        "static" => {
            words.eat("mut");
            DeclarationKind::Static
        }
        "const" => {
            if words.eat("_") {
                return Some(item(DeclarationKind::Const, "_".into()));
            }
            DeclarationKind::Const
        }
        "union" => {
            let name = words.name()?;
            return matches!(words.peek(), Some("{" | "<") | None)
                .then(|| item(DeclarationKind::Union, name));
        }
        "mod" => {
            let name = words.name()?;
            return Some(if words.peek() == Some(";") {
                Head::ModFile { name, public }
            } else {
                item(DeclarationKind::Module, name)
            });
        }
        "macro_rules" => {
            if !words.eat("!") {
                return None;
            }
            return Some(item(DeclarationKind::Macro, words.name()?));
        }
        "impl" => return implementation(words.rest, public),
        _ => return None,
    };
    let name = words.name()?;
    let kind = if kind == DeclarationKind::Trait {
        words.skip_group("<", ">");
        if words.eat("=") {
            DeclarationKind::TraitAlias
        } else {
            kind
        }
    } else {
        kind
    };
    Some(item(kind, name))
}

/// Whether a declaration head starts an impl, including its optional modifiers.
pub(crate) fn starts_impl(line: &str) -> bool {
    let mut words = Words { rest: line };
    while matches!(words.peek(), Some("default" | "unsafe" | "const")) {
        words.next();
    }
    words.eat("impl")
}

/// `impl<G> Trait for Type where ... {`, cut at depth-0 `{`, `;` or `where`.
fn implementation(rest: &str, public: bool) -> Option<Head> {
    let mut rest = rest.trim_start();
    if rest.starts_with('<') {
        let mut depth = 0_usize;
        let end = rest.char_indices().find_map(|(i, c)| {
            match c {
                '<' => depth += 1,
                '>' if !rest[..i].ends_with('-') => depth = depth.saturating_sub(1),
                _ => {}
            }
            (depth == 0).then_some(i + 1)
        })?;
        rest = &rest[end..];
    }
    let mut depth = 0_usize;
    let mut split = None;
    let mut end = rest.len();
    for (i, c) in rest.char_indices() {
        match c {
            '<' | '(' | '[' => depth += 1,
            '>' | ')' | ']' => depth = depth.saturating_sub(1),
            '{' | ';' if depth == 0 => {
                end = i;
                break;
            }
            _ if depth == 0 && word_at(rest, i, "where") => {
                end = i;
                break;
            }
            _ if depth == 0 && split.is_none() && word_at(rest, i, "for") => split = Some(i),
            _ => {}
        }
    }
    let (trait_path, self_type) = match split {
        Some(at) => (
            Some(compact(&rest[..at])),
            compact(rest.get(at + 3..end).unwrap_or_default()),
        ),
        None => (None, compact(&rest[..end])),
    };
    let trait_path = trait_path.map(|t| t.trim_start_matches('!').to_owned());
    if self_type.is_empty() || trait_path.as_deref() == Some("") {
        return None;
    }
    let name = match &trait_path {
        Some(t) => format!("{self_type}::impl<{t}>"),
        None => format!("{self_type}::impl"),
    };
    Some(Head::Item {
        kind: DeclarationKind::Impl,
        name,
        public,
        implementation: Some((self_type, trait_path)),
    })
}

fn word_at(text: &str, i: usize, word: &str) -> bool {
    let before = text[..i].chars().next_back();
    let after = text[i..].strip_prefix(word).map(|r| r.chars().next());
    matches!(after, Some(next) if !next.is_some_and(is_ident_char))
        && !before.is_some_and(is_ident_char)
}

#[cfg(test)]
mod tests {
    use super::*;
    use DeclarationKind::*;

    fn item(line: &str) -> Option<(DeclarationKind, String, bool)> {
        match parse(line)? {
            Head::Item {
                kind, name, public, ..
            } => Some((kind, name, public)),
            _ => None,
        }
    }

    #[test]
    fn heads_with_visibility_and_modifiers_are_recognised() {
        assert_eq!(
            item("pub(crate) async unsafe fn run() {"),
            Some((Function, "run".into(), true))
        );
        assert_eq!(
            item("const fn k() -> u8 { 3 }"),
            Some((Function, "k".into(), false))
        );
        assert_eq!(
            item("pub const LIMIT: u32 = 3;"),
            Some((Const, "LIMIT".into(), true))
        );
        assert_eq!(
            item("static mut X: u8 = 0;"),
            Some((Static, "X".into(), false))
        );
        assert_eq!(
            item("extern \"C\" fn cb() {}"),
            Some((Function, "cb".into(), false))
        );
        assert_eq!(
            item("macro_rules! make {"),
            Some((Macro, "make".into(), false))
        );
        assert_eq!(item("struct r#type;"), Some((Struct, "type".into(), false)));
        assert_eq!(item("union U { a: u8 }"), Some((Union, "U".into(), false)));
    }

    #[test]
    fn expressions_and_keywords_are_not_declarations() {
        assert_eq!(parse("let fn_name = 3;"), None);
        assert_eq!(parse("union.method();"), None);
        assert_eq!(parse("static || {}"), None);
        assert_eq!(parse("const { 3 }"), None);
        assert_eq!(parse("return x;"), None);
        assert_eq!(parse("fn"), None);
    }

    #[test]
    fn impls_modules_and_crates_take_their_shapes() {
        assert_eq!(
            item("impl<T: Into<u8>> Work for Vec<T> where T: Copy {"),
            Some((Impl, "Vec<T>::impl<Work>".into(), false))
        );
        assert_eq!(
            item("unsafe impl !Send for Raw {}"),
            Some((Impl, "Raw::impl<Send>".into(), false))
        );
        assert_eq!(
            item("impl Widget {"),
            Some((Impl, "Widget::impl".into(), false))
        );
        assert_eq!(
            parse("pub mod support;"),
            Some(Head::ModFile {
                name: "support".into(),
                public: true
            })
        );
        assert_eq!(item("mod tests {"), Some((Module, "tests".into(), false)));
        assert_eq!(
            parse("extern crate alloc as heap;"),
            Some(Head::ExternCrate {
                name: "alloc".into(),
                alias: Some("heap".into()),
                public: false
            })
        );
    }
}
