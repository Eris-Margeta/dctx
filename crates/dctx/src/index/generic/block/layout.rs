//! Layout-driven declarations of OCaml: `let`, `type`, `exception`, `val` and `module X = struct` at the level of their scope, and
//! `class X = object` with its `method`s and instance variables. Nesting follows `struct ... end`, `sig ... end` and `object ... end`;
//! a declaration runs to the last line indented under it. Bodies are never entered. Haskell, which shares the helpers below, is in `haskell.rs`.

use super::super::{ident_at, is_denied, mask, signature_of, Decl, Syntax};

/// OCaml: nested `(* *)` comments and double-quoted strings. A single quote is a character or a type variable, never a string.
const OCAML: Syntax = Syntax { line: &[], block: Some(("(*", "*)")), nested: true, quotes: b"\"", triple: false };

/// A `struct`, `sig` or `object` body being read: the module or class declaration, its own column and the column of its members.
struct Module {
    /// Index of the module declaration.
    decl: usize,
    /// Column of the `module` keyword.
    indent: usize,
    /// Column of the first member, once one was seen.
    child: Option<usize>,
}

/// Columns before the first non-blank character of a masked line.
pub(super) fn indent_of(shown: &str) -> usize {
    shown.len() - shown.trim_start().len()
}

/// True for a character that continues an OCaml or Haskell identifier.
pub(super) fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '\''
}

/// The identifier that starts `text` (primes included), or an empty string.
pub(super) fn ident_run(text: &str) -> &str {
    &text[..text.find(|c: char| !is_ident_char(c)).unwrap_or(text.len())]
}

/// Declarations of an OCaml file or interface.
pub(super) fn ocaml(text: &str) -> Vec<Decl> {
    let masked = mask(text, &OCAML);
    let mut decls: Vec<Decl> = Vec::new();
    let mut stack: Vec<Option<Module>> = Vec::new();
    // A module whose `struct` is on a later line, and the declaration whose indented body is being read.
    let mut pending: Option<(usize, usize)> = None;
    let mut current: Option<(usize, usize)> = None;
    for (index, (shown, original)) in masked.split('\n').zip(text.split('\n')).enumerate() {
        let number = u32::try_from(index + 1).unwrap_or(u32::MAX);
        let line = shown.trim();
        if line.is_empty() {
            continue;
        }
        let indent = indent_of(shown);
        let first = ident_run(line);
        let mut module = pending.take().filter(|_| matches!(first, "struct" | "sig" | "object"));
        let mut belongs = false;
        if let Some((at, base)) = current {
            if indent > base || line.starts_with('|') || first == "and" {
                decls[at].lines[1] = number;
                belongs = true;
            } else {
                current = None;
            }
        }
        let fits = match stack.last() {
            None => indent == 0,
            Some(Some(open)) => open.child.map_or(indent > open.indent, |child| indent == child),
            Some(None) => false,
        };
        let in_class = matches!(stack.last(), Some(Some(open)) if decls[open.decl].signature.starts_with("class"));
        if !belongs
            && fits
            && module.is_none()
            && let Some((name, kind, is_module)) = ocaml_decl(line, in_class)
        {
            let owner = match stack.last() {
                Some(Some(open)) => Some(decls[open.decl].name.clone()),
                _ => None,
            };
            if let Some(Some(open)) = stack.last_mut()
                && open.child.is_none()
            {
                open.child = Some(indent);
            }
            let name = match owner {
                Some(owner) => format!("{owner}::{name}"),
                None => name,
            };
            decls.push(Decl { name, kind, lines: [number, number], signature: signature_of(original) });
            let at = decls.len() - 1;
            if is_module {
                module = Some((at, indent));
                current = None;
            } else {
                current = Some((at, indent));
            }
        }
        for word in line.split(|c: char| !is_ident_char(c)).filter(|word| !word.is_empty()) {
            match word {
                "struct" | "sig" | "object" => stack.push(module.take().map(|(decl, indent)| Module { decl, indent, child: None })),
                "begin" => stack.push(None),
                "end" => {
                    if let Some(Some(open)) = stack.pop() {
                        decls[open.decl].lines[1] = number;
                    }
                }
                _ => {}
            }
        }
        pending = module.filter(|_| line.ends_with('='));
    }
    decls
}

/// Name, wire kind and whether it opens a module or class when `line` declares something in OCaml. `in_class` makes a `val` an instance variable.
fn ocaml_decl(line: &str, in_class: bool) -> Option<(String, &'static str, bool)> {
    let word = ident_at(line)?;
    let tail = &line[word.len()..];
    let tail = if word == "method" { tail.strip_prefix('!').unwrap_or(tail) } else { tail };
    let rest = tail.strip_prefix(char::is_whitespace)?.trim_start();
    match word {
        "let" | "val" | "external" | "method" => {
            let mut rest = rest;
            if word == "let" && ident_run(rest) == "rec" {
                rest = rest["rec".len()..].trim_start();
            }
            if matches!(word, "val" | "method") {
                rest = skip_words(rest, &["mutable", "private", "virtual"]);
            }
            let name = ocaml_ident(rest)?;
            let local = word == "let" && line.ends_with(" in");
            if local || name == "_" || matches!(name, "open" | "module" | "exception" | "rec" | "in") || is_denied(name) {
                return None;
            }
            Some((name.to_string(), if word == "val" && in_class { "field" } else { "fn" }, false))
        }
        "exception" => {
            let name = ocaml_ident(rest).filter(|name| name.starts_with(char::is_uppercase) && !is_denied(name))?;
            Some((name.to_string(), "type", false))
        }
        "class" => {
            let mut rest = rest;
            loop {
                let skipped = skip_words(rest, &["virtual", "type"]);
                if skipped.len() != rest.len() {
                    rest = skipped;
                } else if rest.starts_with('[') {
                    rest = rest[rest.find(']')? + 1..].trim_start();
                } else {
                    break;
                }
            }
            let name = ocaml_ident(rest).filter(|name| !is_denied(name))?;
            let opens = line.split(|c: char| !is_ident_char(c)).any(|word| word == "object") || line.ends_with('=');
            Some((name.to_string(), "type", opens))
        }
        "type" => {
            let mut rest = rest;
            loop {
                if let Some(after) = rest.strip_prefix("nonrec").filter(|after| after.starts_with(char::is_whitespace)) {
                    rest = after.trim_start();
                } else if let Some(var) = rest.strip_prefix('\'') {
                    rest = var[ident_run(var).len()..].trim_start();
                } else if rest.starts_with('(') {
                    rest = rest[rest.find(')')? + 1..].trim_start();
                } else {
                    break;
                }
            }
            let name = ocaml_ident(rest).filter(|name| !is_denied(name))?;
            Some((name.to_string(), "type", false))
        }
        "module" => {
            let mut rest = rest;
            for skip in ["type", "rec"] {
                if let Some(after) = rest.strip_prefix(skip).filter(|after| after.starts_with(char::is_whitespace)) {
                    rest = after.trim_start();
                }
            }
            let name = ocaml_ident(rest).filter(|name| name.starts_with(char::is_uppercase))?;
            let opens = line.split(|c: char| !is_ident_char(c)).any(|word| matches!(word, "struct" | "sig")) || line.ends_with('=');
            opens.then(|| (name.to_string(), "type", true))
        }
        _ => None,
    }
}

/// `text` without the leading words of `words`, each of which must be followed by whitespace.
fn skip_words<'a>(mut text: &'a str, words: &[&str]) -> &'a str {
    while let Some(after) = words.iter().find_map(|word| text.strip_prefix(word).filter(|after| after.starts_with(char::is_whitespace))) {
        text = after.trim_start();
    }
    text
}

/// The identifier that starts `text`, when it can name a value, type or module.
fn ocaml_ident(text: &str) -> Option<&str> {
    let name = ident_run(text);
    name.starts_with(|c: char| c.is_alphabetic() || c == '_').then_some(name)
}
