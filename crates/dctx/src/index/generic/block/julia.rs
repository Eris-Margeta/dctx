//! Julia: functions (`function f(`, short-form `f(x) = ...`), macros, structs and their fields, abstract and primitive types, modules and constants.
//! A function or macro is `fn`, a struct, module or abstract type `type`, `name::T` inside a type a `field` and `const NAME = ...` a `const`.

use super::super::{ident_at, is_denied, mask, skip_modifiers, Syntax};
use super::{Block, Tok};

/// How Julia reads: `#` and `#= =#` comments, block keywords closed by `end`, words inside brackets ignored.
pub(super) const SCAN: Block = Block { mask: mask_of, decl: decl_of, word: word_of, flat: true, nested: false };

/// Words that can be followed by a parenthesis without being a function name.
const KEYWORDS: &[&str] = &[
    "elseif", "let", "begin", "end", "using", "import", "export", "local", "global", "const", "quote", "do", "in", "isa", "where", "function",
    "macro", "struct", "module", "mutable", "abstract", "primitive", "baremodule",
];

/// Julia: `#= =#` comments (nesting) first, then `#` comments and strings.
fn mask_of(text: &str) -> String {
    let blocks = mask(text, &Syntax { line: &[], block: Some(("#=", "=#")), nested: true, quotes: b"", triple: false });
    mask(&blocks, &Syntax { line: &["#"], block: None, nested: false, quotes: b"\"", triple: true })
}

/// Julia blocks open at the block keywords and close at `end`.
fn word_of(word: &str, _next: Option<u8>) -> Tok {
    match word {
        "function" | "macro" | "struct" | "module" | "baremodule" | "if" | "for" | "while" | "begin" | "let" | "try" | "quote" | "do"
        | "abstract" | "primitive" => Tok::Open,
        "end" => Tok::Close,
        _ => Tok::Neutral,
    }
}

/// `function f(`, `macro m(` and `f(x) = ...` (fn); `struct X`, `mutable struct X`, `abstract type X` and `module X` (type);
/// `const X = ...` (const); `x::T` inside a type (field).
fn decl_of(line: &str, _source: &str, owner: Option<&str>) -> Option<(String, &'static str)> {
    let rest = skip_modifiers(line, &["mutable"]);
    let word = ident_at(rest)?;
    let after = &rest[word.len()..];
    let (kind, source) = match word {
        "function" | "macro" => ("fn", after),
        "struct" | "module" | "baremodule" => ("type", after),
        "const" => ("const", after),
        "abstract" | "primitive" => {
            let typed = after.trim_start();
            (ident_at(typed)? == "type").then_some(("type", &typed["type".len()..]))?
        }
        _ => return short_form(rest).or_else(|| owner.and_then(|_| field(rest))),
    };
    let source = source.strip_prefix(char::is_whitespace)?.trim_start();
    let end = source.find(|c: char| !(c.is_alphanumeric() || matches!(c, '_' | '!' | '.'))).unwrap_or(source.len());
    let name = source[..end].trim_matches('.');
    if name.is_empty() || name.starts_with(|c: char| c.is_ascii_digit()) || is_denied(name) {
        return None;
    }
    // A constant is one name bound by `=` or typed with `::`; `const a, b = 1, 2` declares nothing here.
    if kind == "const" && !source[end..].trim_start().starts_with(['=', ':']) {
        return None;
    }
    Some((name.to_string(), kind))
}

/// `name(args) = body`, `name(args)::T = body` and `name(args) where T = body` (fn). A comparison, a pair or an indexed assignment is not one.
fn short_form(line: &str) -> Option<(String, &'static str)> {
    let end = line.find(|c: char| !(c.is_alphanumeric() || matches!(c, '_' | '!' | '.'))).unwrap_or(line.len());
    let name = line[..end].trim_matches('.');
    if name.is_empty() || name.starts_with(|c: char| c.is_ascii_digit()) || is_denied(name) || KEYWORDS.contains(&name) {
        return None;
    }
    let args = line[end..].strip_prefix('(')?;
    let mut depth = 1usize;
    let mut close = None;
    for (offset, c) in args.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(offset);
                    break;
                }
            }
            _ => {}
        }
    }
    let mut tail = args[close? + 1..].trim_start();
    if tail.starts_with("::") {
        let cut = tail.find(|c: char| c == '=' || c.is_whitespace()).unwrap_or(tail.len());
        tail = tail[cut..].trim_start();
    }
    if let Some(constraint) = tail.strip_prefix("where").filter(|after| after.starts_with(char::is_whitespace)) {
        tail = &constraint[constraint.find('=')?..];
    }
    let value = tail.strip_prefix('=')?;
    (!value.starts_with(['=', '>'])).then(|| (name.to_string(), "fn"))
}

/// `name::Type` (field), the way a struct body declares one.
fn field(line: &str) -> Option<(String, &'static str)> {
    let name = ident_at(line)?;
    let typed = line[name.len()..].starts_with("::");
    (typed && !is_denied(name) && !KEYWORDS.contains(&name)).then(|| (name.to_string(), "field"))
}
