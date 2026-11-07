// SPDX-License-Identifier: Apache-2.0
//! Layout-driven declarations of OCaml: `let`, `type`, `exception`, `val` and `module X = struct` at the level of their scope, and
//! `class X = object` with its `method`s and instance variables. Nesting follows `struct ... end`, `sig ... end` and `object ... end`;
//! a declaration runs to the last line indented under it. Bodies are never entered. Haskell, which shares the helpers below, is in `haskell.rs`.

use super::super::{Decl, Syntax, ident_at, is_denied, mask, signature_of};
use super::ocaml_body::{Nested, nested_fn, type_members};

/// OCaml: nested `(* *)` comments and double-quoted strings. A single quote is a character or a type variable, never a string.
const OCAML: Syntax = Syntax {
    line: &[],
    block: Some(("(*", "*)")),
    nested: true,
    quotes: b"\"",
    triple: false,
};

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
    // Inside the declaration being read: its latest `and` sibling, the functions its body defines, and the type that takes
    // constructors and fields from these lines with its record brace depth.
    let mut sibling: Option<usize> = None;
    let mut nested = Nested::default();
    let mut members: Option<(usize, i32)> = None;
    for (index, (shown, original)) in masked.split('\n').zip(text.split('\n')).enumerate() {
        let number = u32::try_from(index + 1).unwrap_or(u32::MAX);
        let line = shown.trim();
        if line.is_empty() {
            continue;
        }
        let indent = indent_of(shown);
        let first = ident_run(line);
        let mut module = pending
            .take()
            .filter(|_| matches!(first, "struct" | "sig" | "object"));
        let mut belongs = false;
        if let Some((at, base)) = current {
            if indent > base || line.starts_with('|') || first == "and" {
                decls[at].lines[1] = number;
                if let Some(other) = sibling {
                    decls[other].lines[1] = number;
                }
                belongs = true;
                body_line(
                    &mut decls,
                    BodyLine {
                        number,
                        indent,
                        base,
                        at,
                        line,
                        original,
                    },
                    &mut sibling,
                    &mut nested,
                    &mut members,
                );
            } else {
                current = None;
            }
        }
        if current.is_none() {
            sibling = None;
            nested.clear();
            members = None;
        }
        let fits = match stack.last() {
            None => indent == 0,
            Some(Some(open)) => open
                .child
                .map_or(indent > open.indent, |child| indent == child),
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
            decls.push(Decl {
                name,
                kind,
                lines: [number, number],
                signature: signature_of(original),
            });
            let at = decls.len() - 1;
            if is_module {
                module = Some((at, indent));
                current = None;
            } else {
                current = Some((at, indent));
                members = start_members(&mut decls, at, line, number, original);
            }
        }
        for word in line
            .split(|c: char| !is_ident_char(c))
            .filter(|word| !word.is_empty())
        {
            match word {
                "struct" | "sig" | "object" => {
                    stack.push(module.take().map(|(decl, indent)| Module {
                        decl,
                        indent,
                        child: None,
                    }))
                }
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
    let tail = if word == "method" {
        tail.strip_prefix('!').unwrap_or(tail)
    } else {
        tail
    };
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
            if local
                || name == "_"
                || matches!(name, "open" | "module" | "exception" | "rec" | "in")
                || is_denied(name)
            {
                return None;
            }
            Some((
                name.to_string(),
                if word == "val" && in_class {
                    "field"
                } else {
                    "fn"
                },
                false,
            ))
        }
        "exception" => {
            let name = ocaml_ident(rest)
                .filter(|name| name.starts_with(char::is_uppercase) && !is_denied(name))?;
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
            let opens = line
                .split(|c: char| !is_ident_char(c))
                .any(|word| word == "object")
                || line.ends_with('=');
            Some((name.to_string(), "type", opens))
        }
        "type" => {
            let mut rest = rest;
            loop {
                if let Some(after) = rest
                    .strip_prefix("nonrec")
                    .filter(|after| after.starts_with(char::is_whitespace))
                {
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
                if let Some(after) = rest
                    .strip_prefix(skip)
                    .filter(|after| after.starts_with(char::is_whitespace))
                {
                    rest = after.trim_start();
                }
            }
            let name = ocaml_ident(rest).filter(|name| name.starts_with(char::is_uppercase))?;
            let opens = line
                .split(|c: char| !is_ident_char(c))
                .any(|word| matches!(word, "struct" | "sig"))
                || line.ends_with('=');
            // `module Fmt = Cmdliner_base.Fmt` and `module Arg : Arg_intf`: an alias or a declaration with a named signature.
            let tail = rest[name.len()..].trim_start();
            let named = tail
                .strip_prefix(['=', ':'])
                .is_some_and(|after| ident_run(after.trim_start()).starts_with(char::is_uppercase));
            (opens || named).then(|| (name.to_string(), "type", opens))
        }
        _ => None,
    }
}

/// `text` without the leading words of `words`, each of which must be followed by whitespace.
fn skip_words<'a>(mut text: &'a str, words: &[&str]) -> &'a str {
    while let Some(after) = words.iter().find_map(|word| {
        text.strip_prefix(word)
            .filter(|after| after.starts_with(char::is_whitespace))
    }) {
        text = after.trim_start();
    }
    text
}

/// The identifier that starts `text`, when it can name a value, type or module.
fn ocaml_ident(text: &str) -> Option<&str> {
    let name = ident_run(text);
    name.starts_with(|c: char| c.is_alphabetic() || c == '_')
        .then_some(name)
}

/// One body line of the declaration `at` (declared at column `base`), as `ocaml` hands it to [`body_line`].
struct BodyLine<'a> {
    /// 1-based line number.
    number: u32,
    /// Column of the line.
    indent: usize,
    /// Column of the declaration.
    base: usize,
    /// Index of the declaration.
    at: usize,
    /// The masked, trimmed line.
    line: &'a str,
    /// The line as written, for signatures.
    original: &'a str,
}

/// What a body line adds: an `and` sibling at the declaration's own column, the constructors and fields of a type, or a function
/// defined inside a `let` body (`outer::inner`).
fn body_line(
    decls: &mut Vec<Decl>,
    body: BodyLine,
    sibling: &mut Option<usize>,
    nested: &mut Nested,
    members: &mut Option<(usize, i32)>,
) {
    let BodyLine {
        number,
        indent,
        base,
        at,
        line,
        original,
    } = body;
    if ident_run(line) == "and" && indent == base {
        let keyword = ident_at(&decls[at].signature).unwrap_or("let").to_string();
        let Some((name, kind, _)) =
            ocaml_decl(&format!("{keyword}{}", &line["and".len()..]), false)
        else {
            return;
        };
        let owner = decls[at]
            .name
            .rsplit_once("::")
            .map(|(owner, _)| owner.to_string());
        let name = match owner {
            Some(owner) => format!("{owner}::{name}"),
            None => name,
        };
        decls.push(Decl {
            name,
            kind,
            lines: [number, number],
            signature: signature_of(original),
        });
        let new = decls.len() - 1;
        *sibling = Some(new);
        nested.clear();
        *members = start_members(decls, new, line, number, original);
        return;
    }
    if let Some((owner, depth)) = members.as_mut() {
        let owner_name = decls[*owner].name.clone();
        for (member, kind) in type_members(line, false, depth) {
            decls.push(Decl {
                name: format!("{owner_name}::{member}"),
                kind,
                lines: [number, number],
                signature: signature_of(original),
            });
        }
        return;
    }
    let host = sibling.unwrap_or(at);
    if decls[host].kind != "fn" {
        return;
    }
    let closed_here = nested.close(indent, line.starts_with('|'));
    for open in nested.indices() {
        decls[open].lines[1] = number;
    }
    if ident_run(line) == "and" && !closed_here {
        return;
    }
    if let Some((name, one_line)) = nested_fn(line) {
        let owner = decls[nested.innermost().unwrap_or(host)].name.clone();
        decls.push(Decl {
            name: format!("{owner}::{name}"),
            kind: "fn",
            lines: [number, number],
            signature: signature_of(original),
        });
        if !one_line {
            nested.push(decls.len() - 1, indent);
        }
    }
}

/// For a `type` declaration `at` on `line`: records the constructors and fields its own line lists and returns the state that
/// collects the rest from its body lines. Any other declaration collects none.
fn start_members(
    decls: &mut Vec<Decl>,
    at: usize,
    line: &str,
    number: u32,
    original: &str,
) -> Option<(usize, i32)> {
    let is_type = decls[at].kind == "type" && matches!(ident_run(line), "type" | "and");
    if !is_type {
        return None;
    }
    let mut depth = 0i32;
    let owner = decls[at].name.clone();
    for (member, kind) in type_members(line, true, &mut depth) {
        decls.push(Decl {
            name: format!("{owner}::{member}"),
            kind,
            lines: [number, number],
            signature: signature_of(original),
        });
    }
    Some((at, depth))
}
