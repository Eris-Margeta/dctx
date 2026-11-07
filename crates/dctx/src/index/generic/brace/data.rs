// SPDX-License-Identifier: Apache-2.0
//! Data declarations of the brace family: Scala `val` and `var`, Dart constants and fields, Zig `const` and `var` values and
//! struct fields, Solidity state variables and struct fields, Gleam constants, Groovy fields, and the values of enums. Every
//! function takes one masked, trimmed line and answers with names and wire kinds (`const`, `var`, `field`); the caller owns
//! nesting, spans and signatures. Never panics on odd input.

use super::super::{ident_at, is_denied, skip_modifiers};
use super::Lang;
use super::decl::{Found, Inside, Scope, typed_member};

/// Solidity words that start a line which is not a state variable, plus the attributes that never name one.
const SOLIDITY_WORDS: &[&str] = &[
    "function",
    "event",
    "modifier",
    "error",
    "struct",
    "enum",
    "contract",
    "interface",
    "library",
    "using",
    "pragma",
    "import",
    "constructor",
    "receive",
    "fallback",
    "type",
    "returns",
    "return",
    "emit",
    "revert",
    "require",
    "delete",
    "assembly",
    "unchecked",
    "try",
    "catch",
    "abstract",
    "is",
    "public",
    "private",
    "internal",
    "external",
    "view",
    "pure",
    "payable",
    "virtual",
    "override",
    "constant",
    "immutable",
    "memory",
    "storage",
    "calldata",
];

/// The data member (`const`, `var` or `field`) that the line declares in `lang`, if any. `rest` is the line without its
/// modifiers and `mods` the modifiers that were removed.
pub(super) fn member(lang: &Lang, rest: &str, mods: &str, scope: &Scope) -> Option<Found> {
    match lang.tag {
        "scala" => scala(rest),
        "dart" => dart(rest, mods, scope),
        "zig" => zig(rest, scope),
        "solidity" => solidity(rest, scope),
        "gleam" => gleam(rest),
        "groovy" => groovy(rest, mods, scope),
        _ => None,
    }
}

/// The values an enum body line lists: `A, B`, `A = 1,`, `A(1),` and Scala's `case A, B`. Empty when the line is anything else.
pub(super) fn enum_values(lang: &Lang, line: &str) -> Vec<Found> {
    let list = if lang.tag == "scala" {
        match line.strip_prefix("case") {
            Some(after)
                if after.starts_with(char::is_whitespace)
                    && !matches!(ident_at(after.trim_start()), Some("class" | "object")) =>
            {
                after
            }
            _ => return Vec::new(),
        }
    } else if lang.tag == "powershell" {
        // PowerShell enums have bare names on separate lines: "Good\n  Bad"
        // Split by newline and extract each identifier.
        return line
            .lines()
            .filter_map(|l| {
                let trimmed = l.trim();
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    return None;
                }
                // PowerShell identifiers can contain hyphens
                let mut end = 0usize;
                for (i, &byte) in trimmed.as_bytes().iter().enumerate() {
                    if i == 0 {
                        if byte.is_ascii_alphanumeric() || byte == b'_' || byte >= 0x80 {
                            if byte.is_ascii_digit() {
                                return None;
                            }
                            end = i + 1;
                        } else {
                            return None;
                        }
                    } else if byte.is_ascii_alphanumeric()
                        || byte == b'_'
                        || byte == b'-'
                        || byte >= 0x80
                    {
                        end = i + 1;
                    } else {
                        break;
                    }
                }
                (end > 0).then(|| trimmed[..end].to_string())
            })
            .map(|name| (name, "const"))
            .collect();
    } else {
        line
    };
    value_list(list)
        .into_iter()
        .map(|name| (name, "const"))
        .collect()
}

/// The members of a type whose whole body sits on its declaration line: `enum E { A, B }`, `struct S { uint a; }`.
pub(super) fn inline_members(lang: &Lang, line: &str, inside: Inside) -> Vec<Found> {
    let (Some(open), Some(close)) = (line.find('{'), line.rfind('}')) else {
        return Vec::new();
    };
    if open >= close {
        return Vec::new();
    }
    let inner = &line[open + 1..close];
    match inside {
        Inside::Enum => enum_values(lang, inner),
        Inside::Struct | Inside::Type if matches!(lang.tag, "zig" | "solidity") => {
            let separator = if lang.tag == "zig" { ',' } else { ';' };
            let scope = Scope {
                inside,
                owner: None,
            };
            split_top(inner, separator)
                .into_iter()
                .filter_map(|chunk| {
                    member(
                        lang,
                        skip_modifiers(chunk.trim(), lang.modifiers),
                        "",
                        &scope,
                    )
                })
                .collect()
        }
        _ => Vec::new(),
    }
}

/// True for a plain `=` assignment at the start of `tail` (not `==` or `=>`).
fn assigned(tail: &str) -> bool {
    tail.starts_with('=') && !tail.starts_with("==") && !tail.starts_with("=>")
}

/// True when `tail` (after a data member's name) is a statement end, a list comma, an assignment, or (with `bare`) nothing.
fn ends_data(tail: &str, bare: bool) -> bool {
    (bare && tail.is_empty()) || tail.starts_with([';', ',']) || assigned(tail)
}

/// The name after a keyword (whitespace required) and the trimmed text behind it.
fn declared_name(after: &str) -> Option<(&str, &str)> {
    if !after.starts_with(char::is_whitespace) {
        return None;
    }
    let after = after.trim_start();
    let name = ident_at(after).filter(|name| !is_denied(name))?;
    Some((name, after[name.len()..].trim_start()))
}

/// The words of a modifier prefix.
fn words(mods: &str) -> Vec<&str> {
    mods.split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|word| !word.is_empty())
        .collect()
}

/// Scala `val name`, `lazy val name` (`const`) and `var name` (`var`).
fn scala(rest: &str) -> Option<Found> {
    let word = ident_at(rest)?;
    let kind = match word {
        "val" => "const",
        "var" => "var",
        _ => return None,
    };
    let (name, tail) = declared_name(&rest[word.len()..])?;
    (tail.is_empty() || tail.starts_with(':') || assigned(tail)).then(|| (name.to_string(), kind))
}

/// Dart top-level `const` and `final` values, `var` and typed variables, and class fields.
fn dart(rest: &str, mods: &str, scope: &Scope) -> Option<Found> {
    let first = ident_at(rest)?;
    if matches!(first, "get" | "set") {
        return None;
    }
    let words = words(mods);
    let has = |word: &str| words.contains(&word);
    let declared = first == "var" || has("const") || has("final") || has("late");
    let body = if first == "var" {
        rest[first.len()..].trim_start()
    } else {
        rest
    };
    let untyped = ident_at(body).filter(|name| {
        declared && !is_denied(name) && ends_data(body[name.len()..].trim_start(), false)
    });
    let name = match untyped {
        Some(name) => name.to_string(),
        None => {
            let (name, tail) = typed_member(body)?;
            if !ends_data(tail, false) {
                return None;
            }
            name
        }
    };
    let kind = if has("const") || (has("final") && has("static")) {
        "const"
    } else if matches!(scope.inside, Inside::Type | Inside::Enum) {
        "field"
    } else if has("final") {
        "const"
    } else {
        "var"
    };
    Some((name, kind))
}

/// Zig `const name = value;` (`const`), `var name: T = value;` (`var`) and the fields of a struct or union. Imports are not data.
fn zig(rest: &str, scope: &Scope) -> Option<Found> {
    let word = ident_at(rest)?;
    if matches!(word, "const" | "var") {
        let (name, tail) = declared_name(&rest[word.len()..])?;
        if !(assigned(tail) || tail.starts_with(':')) {
            return None;
        }
        let value = tail
            .split_once('=')
            .map_or("", |(_, value)| value.trim_start());
        if value.starts_with("@import")
            || value.starts_with("@cImport")
            || value.starts_with("@This")
        {
            return None;
        }
        return Some((
            name.to_string(),
            if word == "const" { "const" } else { "var" },
        ));
    }
    if matches!(scope.inside, Inside::Struct | Inside::Enum) {
        let name = ident_at(rest).filter(|name| !is_denied(name))?;
        let tail = rest[name.len()..].trim_start();
        if tail.starts_with(':') && !tail.starts_with("::") {
            return Some((name.to_string(), "field"));
        }
    }
    None
}

/// Solidity state variables (`var`, or `const` for `constant` and `immutable`) and struct fields (`field`).
fn solidity(rest: &str, scope: &Scope) -> Option<Found> {
    let statement = rest.trim().trim_end_matches(';').trim_end();
    let flat = flatten(cut_initializer(statement));
    let tokens: Vec<&str> = flat.split_whitespace().collect();
    let [first, .., name] = tokens.as_slice() else {
        return None;
    };
    let type_like = first.starts_with(|c: char| c.is_alphabetic() || c == '_')
        && first
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | '[' | ']' | '(' | ')' | '$'));
    let name_like =
        ident_at(name) == Some(*name) && !SOLIDITY_WORDS.contains(name) && !is_denied(name);
    if !type_like || !name_like || SOLIDITY_WORDS.contains(first) || is_denied(first) {
        return None;
    }
    let constant = tokens
        .iter()
        .any(|token| matches!(*token, "constant" | "immutable"));
    let kind = if scope.inside == Inside::Struct {
        "field"
    } else if constant {
        "const"
    } else {
        "var"
    };
    Some((name.to_string(), kind))
}

/// `statement` up to the first plain `=` outside parentheses and brackets.
fn cut_initializer(statement: &str) -> &str {
    let bytes = statement.as_bytes();
    let mut depth = 0usize;
    for (at, &byte) in bytes.iter().enumerate() {
        match byte {
            b'(' | b'[' => depth += 1,
            b')' | b']' => depth = depth.saturating_sub(1),
            b'=' if depth == 0 => {
                let before = at.checked_sub(1).map(|prior| bytes[prior]);
                if !matches!(before, Some(b'=' | b'!' | b'<' | b'>'))
                    && !matches!(bytes.get(at + 1), Some(b'=' | b'>'))
                {
                    return &statement[..at];
                }
            }
            _ => {}
        }
    }
    statement
}

/// `text` with every parenthesized group replaced by `()`, so `mapping(address => uint) m` reads as `mapping() m`.
fn flatten(text: &str) -> String {
    let mut out = String::new();
    let mut depth = 0usize;
    for c in text.chars() {
        match c {
            '(' => {
                if depth == 0 {
                    out.push_str("()");
                }
                depth += 1;
            }
            ')' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

/// Gleam `const name = value` and `const name: Type = value`.
fn gleam(rest: &str) -> Option<Found> {
    let word = ident_at(rest).filter(|word| *word == "const")?;
    let (name, tail) = declared_name(&rest[word.len()..])?;
    (tail.starts_with(':') || assigned(tail)).then(|| (name.to_string(), "const"))
}

/// Groovy fields inside a class: `def name = value`, `Type name = value`, `Type name`; `static final` ones are `const`.
fn groovy(rest: &str, mods: &str, scope: &Scope) -> Option<Found> {
    if !matches!(scope.inside, Inside::Type | Inside::Struct | Inside::Enum) {
        return None;
    }
    let first = ident_at(rest)?;
    let (name, tail) = if first == "def" {
        declared_name(&rest[first.len()..]).map(|(name, tail)| (name.to_string(), tail))?
    } else {
        typed_member(rest)?
    };
    if !ends_data(tail, true) {
        return None;
    }
    let words = words(mods);
    let kind = if words.contains(&"static") && words.contains(&"final") {
        "const"
    } else {
        "field"
    };
    Some((name, kind))
}

/// Names of a comma-separated value list (`A, B = 1, C(2)`, trailing `,` or `;` allowed). Empty when anything else is in it.
fn value_list(text: &str) -> Vec<String> {
    let text = text.trim().trim_end_matches([',', ';']).trim_end();
    let mut names = Vec::new();
    for chunk in split_top(text, ',') {
        let chunk = chunk.trim();
        let Some(name) = ident_at(chunk).filter(|name| !is_denied(name)) else {
            return Vec::new();
        };
        let tail = chunk[name.len()..].trim_start();
        let after_args = if tail.starts_with('(') {
            after_group(tail)
        } else {
            tail
        };
        if !(after_args.is_empty() || assigned(after_args) || after_args.starts_with("extends")) {
            return Vec::new();
        }
        names.push(name.to_string());
    }
    names
}

/// The text after the balanced `( ... )` group at the start of `text`, trimmed; the whole text when it never closes.
fn after_group(text: &str) -> &str {
    let mut depth = 0usize;
    for (at, c) in text.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return text[at + 1..].trim_start();
                }
            }
            _ => {}
        }
    }
    text
}

/// `text` cut at every `separator` that stands outside parentheses, brackets and braces.
fn split_top(text: &str, separator: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (at, c) in text.char_indices() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            c if c == separator && depth == 0 => {
                parts.push(&text[start..at]);
                start = at + c.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(&text[start..]);
    parts
}
