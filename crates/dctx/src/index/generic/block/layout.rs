//! Layout-driven declarations: OCaml (`let`, `type`, `val` and `module X = struct` at the level of their scope) and Haskell
//! (column-0 `name ::` signatures, `data`, `newtype`, `type` and `class`). Nesting follows `struct ... end` in OCaml and the
//! `where` body of a class in Haskell; a declaration runs to the last line indented under it. Bodies are never entered.

use super::super::{ident_at, is_denied, mask, signature_of, Decl, Syntax};

/// OCaml: nested `(* *)` comments and double-quoted strings. A single quote is a character or a type variable, never a string.
const OCAML: Syntax = Syntax { line: &[], block: Some(("(*", "*)")), nested: true, quotes: b"\"", triple: false };

/// Haskell: `--` comments, nested `{- -}` comments and double-quoted strings.
const HASKELL: Syntax = Syntax { line: &["--"], block: Some(("{-", "-}")), nested: true, quotes: b"\"", triple: false };

/// A `struct` or `sig` body being read: the module declaration, its own column and the column of its members.
struct Module {
    /// Index of the module declaration.
    decl: usize,
    /// Column of the `module` keyword.
    indent: usize,
    /// Column of the first member, once one was seen.
    child: Option<usize>,
}

/// Columns before the first non-blank character of a masked line.
fn indent_of(shown: &str) -> usize {
    shown.len() - shown.trim_start().len()
}

/// True for a character that continues an OCaml or Haskell identifier.
fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '\''
}

/// The identifier that starts `text` (primes included), or an empty string.
fn ident_run(text: &str) -> &str {
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
        let mut module = pending.take().filter(|_| matches!(first, "struct" | "sig"));
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
        if !belongs
            && fits
            && module.is_none()
            && let Some((name, kind, is_module)) = ocaml_decl(line)
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
                "struct" | "sig" => stack.push(module.take().map(|(decl, indent)| Module { decl, indent, child: None })),
                "begin" | "object" => stack.push(None),
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

/// Name, wire kind and whether it opens a module when `line` declares something in OCaml.
fn ocaml_decl(line: &str) -> Option<(String, &'static str, bool)> {
    let word = ident_at(line)?;
    let rest = line[word.len()..].strip_prefix(char::is_whitespace)?.trim_start();
    match word {
        "let" | "val" | "external" => {
            let mut rest = rest;
            if word == "let" && ident_run(rest) == "rec" {
                rest = rest["rec".len()..].trim_start();
            }
            let name = ocaml_ident(rest)?;
            let local = word == "let" && line.ends_with(" in");
            if local || name == "_" || matches!(name, "open" | "module" | "exception" | "rec" | "in") || is_denied(name) {
                return None;
            }
            Some((name.to_string(), "fn", false))
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

/// The identifier that starts `text`, when it can name a value, type or module.
fn ocaml_ident(text: &str) -> Option<&str> {
    let name = ident_run(text);
    name.starts_with(|c: char| c.is_alphabetic() || c == '_').then_some(name)
}

/// Declarations of a Haskell module.
pub(super) fn haskell(text: &str) -> Vec<Decl> {
    let masked = mask(text, &HASKELL);
    let mut decls: Vec<Decl> = Vec::new();
    // The column-0 declarations the indented lines below belong to, and a class with the column of its method signatures.
    let mut group: Vec<usize> = Vec::new();
    let mut class: Option<(usize, Option<usize>)> = None;
    for (index, (shown, original)) in masked.split('\n').zip(text.split('\n')).enumerate() {
        let number = u32::try_from(index + 1).unwrap_or(u32::MAX);
        let line = shown.trim();
        if line.is_empty() {
            continue;
        }
        let indent = indent_of(shown);
        if indent == 0 {
            class = None;
            let found = haskell_top(line);
            if found.is_empty() {
                // Further equations of the function named by the signature extend it.
                let word = ident_run(line);
                if !group.iter().any(|&at| decls[at].kind == "fn" && decls[at].name == word) {
                    group.clear();
                }
            } else {
                group.clear();
                for (name, kind) in found {
                    decls.push(Decl { name, kind, lines: [number, number], signature: signature_of(original) });
                    group.push(decls.len() - 1);
                }
                if ident_run(line) == "class" {
                    class = Some((decls.len() - 1, None));
                }
            }
        } else if let Some((owner, child)) = class
            && child.is_none_or(|column| column == indent)
        {
            let methods = signature_names(line);
            if !methods.is_empty() {
                class = Some((owner, Some(indent)));
            }
            for (name, kind) in methods {
                let name = format!("{}::{name}", decls[owner].name);
                decls.push(Decl { name, kind, lines: [number, number], signature: signature_of(original) });
            }
        }
        for &at in &group {
            decls[at].lines[1] = number;
        }
    }
    decls
}

/// Declarations that the column-0 `line` of a Haskell module starts.
fn haskell_top(line: &str) -> Vec<(String, &'static str)> {
    let word = ident_run(line);
    let rest = &line[word.len()..];
    match word {
        "data" | "newtype" | "class" | "type" if rest.starts_with(char::is_whitespace) => {
            let mut rest = rest.trim_start();
            if let Some(after) = rest.strip_prefix("family").filter(|after| after.starts_with(char::is_whitespace)) {
                rest = after.trim_start();
            }
            if rest.starts_with("instance") {
                return Vec::new();
            }
            // A context `Eq a =>` comes before the name. After a `data` constructor's `=` it is the constructor's own.
            let head = rest.split(" where").next().unwrap_or(rest);
            let head = match head.find("=>") {
                Some(arrow) if word == "class" || head.find('=') == Some(arrow) => head[arrow + 2..].trim_start(),
                _ => head,
            };
            let name = ident_run(head);
            if name.starts_with(char::is_uppercase) { vec![(name.to_string(), "type")] } else { Vec::new() }
        }
        _ => signature_names(line),
    }
}

/// Names of a type signature line `a, b :: Type`, or none when the line is anything else.
fn signature_names(line: &str) -> Vec<(String, &'static str)> {
    let Some(split) = line.find("::") else {
        return Vec::new();
    };
    if line[split..].starts_with(":::") {
        return Vec::new();
    }
    let mut names = Vec::new();
    for name in line[..split].split(',').map(str::trim) {
        let valid = ident_run(name) == name && name.starts_with(|c: char| c.is_lowercase() || c == '_') && !is_denied(name);
        if !valid {
            return Vec::new();
        }
        names.push((name.to_string(), "fn"));
    }
    names
}
