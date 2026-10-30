//! Elixir: modules, protocols and implementations (types), structs, `@type` names, functions, macros and `@spec` names.
//! Blocks open at `do` and `fn`; a `defimpl` is named `Protocol.Target` so its functions stay apart from the protocol's own.

use std::collections::HashSet;

use super::super::{ident_at, is_denied, mask, Decl, Syntax};
use super::{Block, Tok};

/// How Elixir reads: `#` comments, `do` blocks, no bracket flattening.
pub(super) const SCAN: Block = Block { mask: mask_of, decl: decl_of, word: word_of, flat: false };

/// Elixir: `#` comments, quoted strings and heredocs.
fn mask_of(text: &str) -> String {
    mask(text, &Syntax { line: &["#"], block: None, nested: false, quotes: b"\"'", triple: true })
}

/// Elixir blocks open at `do` and `fn`; `do:` and `fn:` are keyword arguments.
fn word_of(word: &str, next: Option<u8>) -> Tok {
    match word {
        "do" | "fn" if next != Some(b':') => Tok::Open,
        "end" if next != Some(b':') => Tok::Close,
        _ => Tok::Neutral,
    }
}

/// `defmodule`, `defprotocol` and `defimpl` (type), `defstruct` (type named like its module), `@type` (type), `@spec` (fn until [`finish`] decides),
/// and `def`, `defp`, `defmacro` and friends (fn).
fn decl_of(line: &str, owner: Option<&str>) -> Option<(String, &'static str)> {
    if let Some(attribute) = line.strip_prefix('@') {
        return attribute_decl(attribute);
    }
    let word = ident_at(line)?;
    if matches!(word, "defstruct" | "defexception") {
        let own = owner?.rsplit([':', '.']).next().filter(|last| !last.is_empty())?;
        return Some((own.to_string(), "type"));
    }
    let kind = match word {
        "defmodule" | "defprotocol" | "defimpl" => "type",
        "def" | "defp" | "defmacro" | "defmacrop" | "defguard" | "defguardp" | "defdelegate" => "fn",
        _ => return None,
    };
    let rest = line[word.len()..].strip_prefix(char::is_whitespace)?.trim_start();
    let end = if kind == "type" {
        rest.find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '.')).unwrap_or(rest.len())
    } else {
        let name = ident_at(rest)?;
        name.len() + usize::from(rest[name.len()..].starts_with(['?', '!']))
    };
    let mut name = rest[..end].trim_end_matches('.').to_string();
    if name.is_empty() || is_denied(&name) || name == "unquote" {
        return None;
    }
    if word == "defimpl"
        && let Some(target) = impl_target(&rest[end..])
    {
        name = format!("{name}.{target}");
    }
    Some((name, kind))
}

/// The module after `for:` in the rest of a `defimpl` line, when it is a module name (`for: [A, B]` and `for: __MODULE__` have none).
fn impl_target(rest: &str) -> Option<&str> {
    let after = rest[rest.find("for:")? + 4..].trim_start();
    let end = after.find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '.')).unwrap_or(after.len());
    let target = after[..end].trim_end_matches('.');
    target.starts_with(char::is_uppercase).then_some(target)
}

/// `@type name`, `@typep name`, `@opaque name` (type) and `@spec name`, `@callback name` (kind `spec` until [`finish`]).
fn attribute_decl(attribute: &str) -> Option<(String, &'static str)> {
    let word = ident_at(attribute)?;
    let kind = match word {
        "type" | "typep" | "opaque" => "type",
        "spec" | "callback" | "macrocallback" => "spec",
        _ => return None,
    };
    let rest = attribute[word.len()..].strip_prefix(char::is_whitespace)?.trim_start();
    let name = ident_at(rest)?;
    let end = name.len() + usize::from(kind == "spec" && rest[name.len()..].starts_with(['?', '!']));
    let name = &rest[..end];
    if is_denied(name) || name == "unquote" {
        return None;
    }
    Some((name.to_string(), kind))
}

/// Settles the `spec` declarations: one that names a function defined in the file, or that repeats an earlier spec, is dropped; the rest are functions.
pub(super) fn finish(decls: Vec<Decl>) -> Vec<Decl> {
    let defined: HashSet<String> = decls.iter().filter(|decl| decl.kind != "spec").map(|decl| decl.name.clone()).collect();
    let mut seen: HashSet<String> = HashSet::new();
    decls
        .into_iter()
        .filter_map(|mut decl| {
            if decl.kind == "spec" {
                if defined.contains(&decl.name) || !seen.insert(decl.name.clone()) {
                    return None;
                }
                decl.kind = "fn";
            }
            Some(decl)
        })
        .collect()
}
