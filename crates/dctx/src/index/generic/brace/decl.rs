// SPDX-License-Identifier: Apache-2.0
//! One masked line of a brace-family file: what it declares in its scope. Types and functions come from the keyword tables,
//! with Dart's `Type name(` forms, constructors and accessors and Zig's `test` blocks on top; constants, variables, fields and
//! enum values are answered by [`super::data`]. Pure functions of one line: nesting, spans and signatures belong to the caller.

use super::super::{ident_at, is_denied, skip_modifiers};
use super::{Lang, data};

/// Where a line stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Inside {
    /// File level.
    File,
    /// Directly in the body of a class, trait, object, mixin, extension, contract or library.
    Type,
    /// Directly in the body of a struct or union: its data members are fields.
    Struct,
    /// Directly in the body of an enum: bare names are its values.
    Enum,
    /// In the body of a Scala def or val (only nested defs count) or of a Zig function or test (only nested types count).
    Body,
}

/// The scope a line is read in.
pub(super) struct Scope<'a> {
    /// What kind of body the line is directly in.
    pub(super) inside: Inside,
    /// Qualified name of the enclosing type or def.
    pub(super) owner: Option<&'a str>,
}

/// A name and its wire kind.
pub(super) type Found = (String, &'static str);

/// Words that start a statement or a directive, never a Dart or Groovy type.
const NOT_TYPES: &[&str] = &[
    "import",
    "export",
    "part",
    "library",
    "typedef",
    "extends",
    "implements",
    "with",
    "super",
    "this",
    "package",
    "default",
    "show",
    "hide",
    "in",
    "is",
    "as",
    "on",
    "of",
];

/// Words that are never a member name after a type.
const NOT_NAMES: &[&str] = &[
    "operator",
    "extends",
    "implements",
    "with",
    "is",
    "in",
    "as",
    "on",
    "of",
    "show",
    "hide",
];

/// Everything the masked, trimmed `line` declares (`original` is the same line unmasked, for names inside strings).
/// Almost always zero or one; an enum value list and a Scala `case A, B` declare several.
pub(super) fn declarations(lang: &Lang, line: &str, original: &str, scope: &Scope) -> Vec<Found> {
    if scope.inside == Inside::Enum {
        let values = data::enum_values(lang, line);
        if !values.is_empty() {
            return values;
        }
    }
    let rest = skip_modifiers(line, lang.modifiers);
    let mods = &line[..line.len() - rest.len()];
    let found = core(lang, rest);
    if scope.inside == Inside::Body {
        let wanted = if lang.const_type { "type" } else { "fn" };
        return found
            .filter(|(_, kind)| *kind == wanted)
            .into_iter()
            .collect();
    }
    let found = found
        .or_else(|| zig_test(lang, line, original).map(|name| (name, "fn")))
        .or_else(|| constructor(lang, rest, scope).map(|name| (name, "fn")))
        .or_else(|| powershell_method(lang, rest, scope))
        .or_else(|| data::member(lang, rest, mods, scope));
    found.into_iter().collect()
}

/// The body kind a type declaration opens: enum, struct (or union) or any other type.
pub(super) fn shape(lang: &Lang, signature: &str) -> Inside {
    let rest = skip_modifiers(signature, lang.modifiers);
    let word = ident_at(rest).unwrap_or("");
    let keyword = if lang.const_type && word == "const" {
        const_kind(&rest[word.len()..]).map(|(_, kind)| kind)
    } else {
        Some(word)
    };
    match keyword {
        Some("enum") => Inside::Enum,
        Some("struct" | "union") => Inside::Struct,
        _ => Inside::Type,
    }
}

/// Name and wire kind of a type or function declared by the keyword tables (and Dart's typed functions).
fn core(lang: &Lang, rest: &str) -> Option<Found> {
    if lang.typed
        && ident_at(rest).is_none_or(|word| !lang.types.contains(&word))
        && let Some(name) = dart_function(rest)
    {
        return Some((name, "fn"));
    }
    let word = ident_at(rest)?;
    let after = &rest[word.len()..];
    if lang.types.contains(&word) {
        return name_after(after, false, word).map(|name| (name, "type"));
    }
    if lang.fns.contains(&word) {
        if lang.tag == "powershell" {
            // PowerShell function names may contain hyphens.
            if let Some(name) = read_powershell_name(after.trim_start()) {
                if !is_denied(name) {
                    return Some((name.to_string(), "fn"));
                }
                // Denied name (e.g., `function if {`): not a function declaration.
                return None;
            }
        }
        return name_after(after, lang.paren, word).map(|name| (name, "fn"));
    }
    if lang.const_type && word == "const" {
        return const_type(after).map(|name| (name, "type"));
    }
    None
}

/// The name that follows a declaration keyword. `paren` requires `(` right after it.
fn name_after(after: &str, paren: bool, word: &str) -> Option<String> {
    if !after.starts_with(char::is_whitespace) {
        return None;
    }
    let mut rest = after.trim_start();
    // `mixin class Name` and `extension type Name`: the second keyword is part of the declaration.
    if ident_at(rest) == Some("class") {
        rest = rest["class".len()..].trim_start();
    } else if word == "extension" && ident_at(rest) == Some("type") {
        rest = rest["type".len()..].trim_start();
    }
    // `extension on Box {`: an unnamed extension.
    if word == "extension" && ident_at(rest) == Some("on") {
        return Some("extension".to_string());
    }
    let name = ident_at(rest).filter(|name| !is_denied(name))?;
    (!paren || rest[name.len()..].trim_start().starts_with('(')).then(|| name.to_string())
}

/// Zig `const Name = [packed|extern] struct|enum|union|opaque`: the name and the keyword.
fn const_kind(after: &str) -> Option<(&str, &str)> {
    let rest = after.trim_start();
    let name = ident_at(rest).filter(|name| !is_denied(name))?;
    let mut value = rest[name.len()..]
        .trim_start()
        .strip_prefix('=')?
        .trim_start();
    while let Some(word) = ident_at(value).filter(|word| matches!(*word, "packed" | "extern")) {
        value = value[word.len()..].trim_start();
    }
    let kind = ident_at(value)?;
    matches!(kind, "struct" | "enum" | "union" | "opaque").then_some((name, kind))
}

/// Zig `const Name = struct|enum|union|opaque`: the type's name.
fn const_type(after: &str) -> Option<String> {
    const_kind(after).map(|(name, _)| name.to_string())
}

/// Zig `test "name" {`, `test name {` or `test {`: `test "name"`, `test name` or `test`, read from the unmasked line.
fn zig_test(lang: &Lang, line: &str, original: &str) -> Option<String> {
    if lang.tag != "zig" || !line.contains('{') {
        return None;
    }
    line.strip_prefix("test").filter(|after| {
        after.is_empty() || after.starts_with(char::is_whitespace) || after.starts_with('{')
    })?;
    let text = original.trim_start().strip_prefix("test")?.trim_start();
    if let Some(body) = text.strip_prefix('"') {
        let mut end = body.len();
        let mut escaped = false;
        for (at, c) in body.char_indices() {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                end = at;
                break;
            }
        }
        let name: String = body[..end].chars().take(80).collect();
        return Some(format!("test \"{name}\""));
    }
    Some(ident_at(text).map_or_else(|| "test".to_string(), |name| format!("test {name}")))
}

/// Dart `Owner(` or `Owner.named(` inside the class `Owner`: the constructor's name (`Owner`, or `named`).
fn constructor(lang: &Lang, rest: &str, scope: &Scope) -> Option<String> {
    if lang.tag != "dart" || scope.inside != Inside::Type {
        return None;
    }
    let owner = scope.owner?.rsplit("::").next()?;
    let first = ident_at(rest).filter(|word| *word == owner)?;
    let after = &rest[first.len()..];
    if after.starts_with('(') {
        return Some(first.to_string());
    }
    let named = after.strip_prefix('.').and_then(ident_at)?;
    after[1 + named.len()..]
        .starts_with('(')
        .then(|| named.to_string())
}

/// PowerShell class methods: `[type] Name(` inside a class body. Names may contain `-`.
fn powershell_method(lang: &Lang, rest: &str, scope: &Scope) -> Option<Found> {
    if lang.tag != "powershell" || !matches!(scope.inside, Inside::Type | Inside::Enum) {
        return None;
    }
    // Strip `[type]` prefix: skip past `]` if present.
    let after_bracket = if rest.starts_with('[') {
        rest.find(']').map(|i| rest[i + 1..].trim_start())
    } else {
        None
    }?;
    // Read the method name (may include `-`).
    let name = read_powershell_name(after_bracket)?;
    let tail = after_bracket[name.len()..].trim_start();
    tail.starts_with('(').then(|| (name.to_string(), "fn"))
}

/// Read a PowerShell identifier from `text`: letters, digits, `_`, and `-`, not starting with a digit.
fn read_powershell_name(text: &str) -> Option<&str> {
    let mut end = 0usize;
    for (i, &byte) in text.as_bytes().iter().enumerate() {
        if i == 0 {
            if byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$' || byte >= 0x80 {
                if byte.is_ascii_digit() {
                    return None;
                }
                end = i + 1;
            } else {
                return None;
            }
        } else if byte.is_ascii_alphanumeric()
            || byte == b'_'
            || byte == b'$'
            || byte == b'-'
            || byte >= 0x80
        {
            end = i + 1;
        } else {
            break;
        }
    }
    (end > 0).then(|| &text[..end])
}

/// Dart `Type name(` or `Type<T> name<U>(`, and the accessors `get name =>`, `set name(..)`, `Type get name =>`.
fn dart_function(rest: &str) -> Option<String> {
    let first = ident_at(rest)?;
    let after_first = &rest[first.len()..];
    if matches!(first, "get" | "set")
        && after_first.starts_with(char::is_whitespace)
        && let Some(name) = accessor_name(after_first.trim_start())
    {
        return Some(name);
    }
    let (name, tail) = typed_member(rest)?;
    if matches!(name.as_str(), "get" | "set")
        && let Some(accessor) = accessor_name(tail)
    {
        return Some(accessor);
    }
    tail.starts_with(['(', '<']).then_some(name)
}

/// The accessor name at the start of `text` (what follows `get` or `set`) when a body or a parameter list comes after it.
fn accessor_name(text: &str) -> Option<String> {
    let name = ident_at(text).filter(|name| !is_denied(name))?;
    let tail = text[name.len()..].trim_start();
    (tail.is_empty()
        || tail.starts_with("=>")
        || tail.starts_with(['(', '{', ';'])
        || tail.starts_with("async"))
    .then(|| name.to_string())
}

/// `Type name` or `Type<T>? name` (a dotted or array type too): the name and the text after it, trimmed.
pub(super) fn typed_member(rest: &str) -> Option<(String, &str)> {
    let first = ident_at(rest)?;
    if is_denied(first) || NOT_TYPES.contains(&first) {
        return None;
    }
    let mut after = &rest[first.len()..];
    after = after
        .strip_prefix('.')
        .and_then(ident_at)
        .map_or(after, |segment| &after[1 + segment.len()..]);
    if after.starts_with('<') {
        let close = balanced_angle(after)?;
        after = &after[close..];
    }
    while let Some(tail) = after.strip_prefix("[]") {
        after = tail;
    }
    after = after.strip_prefix('?').unwrap_or(after);
    if !after.starts_with(char::is_whitespace) {
        return None;
    }
    let rest = after.trim_start();
    let name = ident_at(rest).filter(|name| !is_denied(name) && !NOT_NAMES.contains(name))?;
    Some((name.to_string(), rest[name.len()..].trim_start()))
}

/// Offset just past the `>` that closes the `<` at the start of `text`, on one line.
fn balanced_angle(text: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (offset, c) in text.char_indices() {
        match c {
            '<' => depth += 1,
            '>' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(offset + 1);
                }
            }
            c if c.is_alphanumeric() || matches!(c, '_' | '$' | '?' | ',' | ' ' | '.') => {}
            _ => return None,
        }
    }
    None
}

/// The body kind of a Zig anonymous type that starts `text` (`struct`, `packed struct`, `union(enum)`, `enum(u8)`, `opaque`).
fn anonymous_shape(text: &str) -> Option<Inside> {
    let mut rest = text.trim_start();
    while let Some(word) = ident_at(rest).filter(|word| matches!(*word, "packed" | "extern")) {
        rest = rest[word.len()..].trim_start();
    }
    match ident_at(rest)? {
        "struct" | "union" => Some(Inside::Struct),
        "enum" => Some(Inside::Enum),
        "opaque" => Some(Inside::Type),
        _ => None,
    }
}

/// Zig `name: struct {` or `mode: enum { a, b } = .a,`: the body kind of the anonymous type a field is declared with.
pub(super) fn field_shape(line: &str) -> Option<Inside> {
    let line = line.trim_start();
    let name = ident_at(line)?;
    let after = line[name.len()..]
        .trim_start()
        .strip_prefix(':')
        .filter(|after| !after.starts_with(':'))?;
    anonymous_shape(after)
}

/// Zig `return struct {` inside a function that returns a type: the body kind of the returned type.
pub(super) fn returned_shape(line: &str) -> Option<Inside> {
    let after = line
        .trim_start()
        .strip_prefix("return")
        .filter(|after| after.starts_with(char::is_whitespace))?;
    anonymous_shape(after)
}

/// Scala `class Str(private val chars: Array[Char], var n: Int)`: the `val` (`const`) and `var` (`var`) parameters of a class
/// header line, read from parenthesis `depth` on (the depth the header was at when the line began) up to the body's `{`.
pub(super) fn class_params(line: &str, mut depth: usize) -> Vec<Found> {
    let mut found = Vec::new();
    let mut at = 0usize;
    let bytes = line.as_bytes();
    while at < bytes.len() {
        match bytes[at] {
            b'(' => depth += 1,
            b')' => depth = depth.saturating_sub(1),
            b'{' if depth == 0 => break,
            byte if byte.is_ascii_alphabetic()
                && (at == 0
                    || !(bytes[at - 1].is_ascii_alphanumeric() || bytes[at - 1] == b'_')) =>
            {
                let word = ident_at(&line[at..]).unwrap_or("");
                let after = &line[at + word.len()..];
                let kind = match word {
                    "val" => Some("const"),
                    "var" => Some("var"),
                    _ => None,
                };
                if depth > 0
                    && let Some(kind) = kind
                    && after.starts_with(char::is_whitespace)
                    && let Some(name) = ident_at(after.trim_start()).filter(|name| !is_denied(name))
                {
                    found.push((name.to_string(), kind));
                }
                at += word.len().max(1);
                continue;
            }
            _ => {}
        }
        at += 1;
    }
    found
}
