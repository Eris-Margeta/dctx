//! Block-structured declarations: Elixir, Lua and Julia (blocks closed by `end`), R (braces), and through [`layout`] OCaml and Haskell.
//! The `end` languages share one scanner: a table says which words open or close a block and how a declaration line reads.
//! A declaration's frame is the first block opened on its line, so its end line is where that block closes; bodies are never entered.
//! Heuristic by design: one declaration per line, nesting as `Parent::child` inside a module or struct.

mod layout;
#[cfg(test)]
mod tests;

use super::{ident_at, is_denied, mask, signature_of, skip_modifiers, to_symbols, Decl, Syntax};
use crate::index::extract::Symbol;

/// What one word does to the stack of open blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tok {
    /// Opens a block that a later `end` closes.
    Open,
    /// Closes the innermost block.
    Close,
    /// Neither.
    Neutral,
}

/// How one `end` language is read.
struct Block {
    /// Blanks comments and strings, keeping every offset.
    mask: fn(&str) -> String,
    /// Name and wire kind when a trimmed, masked line declares something.
    decl: fn(&str) -> Option<(String, &'static str)>,
    /// What a word does to the block stack, given the byte that follows it.
    word: fn(&str, Option<u8>) -> Tok,
    /// True when words inside brackets are ignored (Julia's `a[end]` and comprehensions).
    flat: bool,
}

const ELIXIR: Block = Block { mask: elixir_mask, decl: elixir_decl, word: elixir_word, flat: false };
const LUA: Block = Block { mask: lua_mask, decl: lua_decl, word: lua_word, flat: false };
const JULIA: Block = Block { mask: julia_mask, decl: julia_decl, word: julia_word, flat: true };

/// Comment and string syntax of R.
const R_SYNTAX: Syntax = Syntax { line: &["#"], block: None, nested: false, quotes: b"\"'", triple: false };

/// True for the six tags of this family.
pub(super) fn handles(language: &str) -> bool {
    matches!(language, "elixir" | "lua" | "ocaml" | "r" | "julia" | "haskell")
}

/// Symbols of `text`, or none for an unknown tag.
pub(super) fn symbols(tag: &str, text: &str) -> Vec<Symbol> {
    let decls = match tag {
        "elixir" => scan(&ELIXIR, text),
        "lua" => scan(&LUA, text),
        "julia" => scan(&JULIA, text),
        "r" => r_decls(text),
        "ocaml" => layout::ocaml(text),
        "haskell" => layout::haskell(text),
        _ => return Vec::new(),
    };
    to_symbols(text, decls)
}

/// One open block: the module or struct it belongs to (so members are visible) and the declaration it closes.
struct Frame {
    /// Qualified name of the type whose body this is.
    container: Option<String>,
    /// Index of the declaration whose end line this block sets.
    decl: Option<usize>,
}

/// One word of a masked line with the bytes around it.
struct Word<'a> {
    /// The word.
    text: &'a str,
    /// The byte before it (`.` or `:` mark a field or an atom, not a keyword).
    prev: Option<u8>,
    /// The byte after it (`do:` is a keyword argument, not a block).
    next: Option<u8>,
}

/// True for a byte that continues a word. Non-ASCII bytes count, so slicing stays on character boundaries.
fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte >= 0x80
}

/// Words of one masked line. `brackets` is the open bracket depth, carried from line to line; with `flat` the words inside brackets are left out.
fn words<'a>(line: &'a str, brackets: &mut usize, flat: bool) -> Vec<Word<'a>> {
    let bytes = line.as_bytes();
    let mut found = Vec::new();
    let mut at = 0usize;
    while at < bytes.len() {
        let byte = bytes[at];
        if is_word_byte(byte) {
            let start = at;
            while at < bytes.len() && is_word_byte(bytes[at]) {
                at += 1;
            }
            if !(flat && *brackets > 0) {
                found.push(Word { text: &line[start..at], prev: start.checked_sub(1).map(|before| bytes[before]), next: bytes.get(at).copied() });
            }
            continue;
        }
        match byte {
            b'(' | b'[' | b'{' => *brackets += 1,
            b')' | b']' | b'}' => *brackets = brackets.saturating_sub(1),
            _ => {}
        }
        at += 1;
    }
    found
}

/// Declarations of `text` in an `end` language.
fn scan(block: &Block, text: &str) -> Vec<Decl> {
    let masked = (block.mask)(text);
    let mut decls: Vec<Decl> = Vec::new();
    let mut stack: Vec<Frame> = Vec::new();
    // A declaration whose block has not opened yet: its parameter list runs over several lines.
    let mut pending: Option<usize> = None;
    let mut brackets = 0usize;
    for (index, (shown, original)) in masked.split('\n').zip(text.split('\n')).enumerate() {
        let number = u32::try_from(index + 1).unwrap_or(u32::MAX);
        let line = shown.trim();
        if line.is_empty() {
            continue;
        }
        let at_top = brackets == 0;
        let mut open = pending.take();
        let in_scope = stack.last().is_none_or(|frame| frame.container.is_some());
        if open.is_none()
            && in_scope
            && at_top
            && let Some((name, kind)) = (block.decl)(line)
        {
            let owner = stack.last().and_then(|frame| frame.container.as_deref());
            let name = match owner {
                Some(owner) => format!("{owner}::{name}"),
                None => name,
            };
            // Consecutive clauses of one function are one symbol.
            let again = kind == "fn" && decls.last().is_some_and(|last| last.kind == "fn" && last.name == name);
            if again {
                let last = decls.len() - 1;
                decls[last].lines[1] = number;
            } else {
                decls.push(Decl { name, kind, lines: [number, number], signature: signature_of(original) });
            }
            open = Some(decls.len() - 1);
        }
        for word in words(shown, &mut brackets, block.flat) {
            if matches!(word.prev, Some(b'.' | b':' | b'@')) {
                continue;
            }
            match (block.word)(word.text, word.next) {
                Tok::Open => {
                    let decl = open.take();
                    let container = decl.filter(|&at| decls[at].kind == "type").map(|at| decls[at].name.clone());
                    stack.push(Frame { container, decl });
                }
                Tok::Close => {
                    if let Some(frame) = stack.pop()
                        && let Some(at) = frame.decl
                    {
                        decls[at].lines[1] = number;
                    }
                }
                Tok::Neutral => {}
            }
        }
        // No block opened on the declaration line: a one-liner, unless its parameters are still open.
        if let Some(at) = open {
            if brackets > 0 {
                pending = Some(at);
            } else {
                decls[at].lines[1] = number;
            }
        }
    }
    decls
}

/// Elixir: `#` comments, quoted strings and heredocs.
fn elixir_mask(text: &str) -> String {
    mask(text, &Syntax { line: &["#"], block: None, nested: false, quotes: b"\"'", triple: true })
}

/// Elixir blocks open at `do` and `fn`; `do:` and `fn:` are keyword arguments.
fn elixir_word(word: &str, next: Option<u8>) -> Tok {
    match word {
        "do" | "fn" if next != Some(b':') => Tok::Open,
        "end" if next != Some(b':') => Tok::Close,
        _ => Tok::Neutral,
    }
}

/// `defmodule Name`, `defprotocol Name` (type) and `def`, `defp`, `defmacro` and friends (fn).
fn elixir_decl(line: &str) -> Option<(String, &'static str)> {
    let word = ident_at(line)?;
    let kind = match word {
        "defmodule" | "defprotocol" => "type",
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
    let name = rest[..end].trim_end_matches('.');
    if name.is_empty() || is_denied(name) || name == "unquote" {
        return None;
    }
    Some((name.to_string(), kind))
}

/// Lua: `--[[ ]]` comments first, then `--` comments and strings, then `[[ ]]` long strings.
fn lua_mask(text: &str) -> String {
    let long_comments = mask(text, &Syntax { line: &[], block: Some(("--[[", "]]")), nested: false, quotes: b"", triple: false });
    let strings = mask(&long_comments, &Syntax { line: &["--"], block: None, nested: false, quotes: b"\"'", triple: false });
    mask(&strings, &Syntax { line: &[], block: Some(("[[", "]]")), nested: false, quotes: b"", triple: false })
}

/// Lua blocks open at `function`, `if`, `do` (loops included) and `repeat`; `end` and `until` close them.
fn lua_word(word: &str, _next: Option<u8>) -> Tok {
    match word {
        "function" | "if" | "do" | "repeat" => Tok::Open,
        "end" | "until" => Tok::Close,
        _ => Tok::Neutral,
    }
}

/// The dotted or colon path that starts `text`, and what follows it.
fn lua_path(text: &str) -> (&str, &str) {
    let end = text.find(|c: char| !(c.is_alphanumeric() || matches!(c, '_' | '.' | ':'))).unwrap_or(text.len());
    text.split_at(end)
}

/// `Table::name` for `Table.name` and `Table:name`, plain `name` without a table.
fn lua_name(path: &str) -> Option<String> {
    let (parent, last) = match path.rfind(['.', ':']) {
        Some(cut) => (&path[..cut], &path[cut + 1..]),
        None => ("", path),
    };
    if last.is_empty() || last.starts_with(|c: char| c.is_ascii_digit()) || is_denied(last) {
        return None;
    }
    Some(if parent.is_empty() { last.to_string() } else { format!("{parent}::{last}") })
}

/// `function a.b(`, `function a:b(`, `local function x(`, and `name = function(` (fn).
fn lua_decl(line: &str) -> Option<(String, &'static str)> {
    let mut rest = line;
    if ident_at(rest) == Some("local") {
        rest = rest["local".len()..].trim_start();
    }
    if ident_at(rest) == Some("function") {
        let after = rest["function".len()..].strip_prefix(char::is_whitespace)?.trim_start();
        let (path, tail) = lua_path(after);
        return tail.trim_start().starts_with('(').then(|| lua_name(path)).flatten().map(|name| (name, "fn"));
    }
    let (path, tail) = lua_path(rest);
    let value = tail.trim_start().strip_prefix('=')?.trim_start();
    let is_function = ident_at(value) == Some("function") && value["function".len()..].trim_start().starts_with('(');
    if is_function { lua_name(path).map(|name| (name, "fn")) } else { None }
}

/// Julia: `#= =#` comments (nesting) first, then `#` comments and strings.
fn julia_mask(text: &str) -> String {
    let blocks = mask(text, &Syntax { line: &[], block: Some(("#=", "=#")), nested: true, quotes: b"", triple: false });
    mask(&blocks, &Syntax { line: &["#"], block: None, nested: false, quotes: b"\"", triple: true })
}

/// Julia blocks open at the block keywords and close at `end`.
fn julia_word(word: &str, _next: Option<u8>) -> Tok {
    match word {
        "function" | "macro" | "struct" | "module" | "baremodule" | "if" | "for" | "while" | "begin" | "let" | "try" | "quote" | "do"
        | "abstract" | "primitive" => Tok::Open,
        "end" => Tok::Close,
        _ => Tok::Neutral,
    }
}

/// `function f(` and `macro m(` (fn); `struct X`, `mutable struct X`, `abstract type X` and `module X` (type).
fn julia_decl(line: &str) -> Option<(String, &'static str)> {
    let rest = skip_modifiers(line, &["mutable"]);
    let word = ident_at(rest)?;
    let after = &rest[word.len()..];
    let (kind, source) = match word {
        "function" | "macro" => ("fn", after),
        "struct" | "module" | "baremodule" => ("type", after),
        "abstract" | "primitive" => {
            let typed = after.trim_start();
            (ident_at(typed)? == "type").then_some(("type", &typed["type".len()..]))?
        }
        _ => return None,
    };
    let source = source.strip_prefix(char::is_whitespace)?.trim_start();
    let end = source.find(|c: char| !(c.is_alphanumeric() || matches!(c, '_' | '!' | '.'))).unwrap_or(source.len());
    let name = source[..end].trim_matches('.');
    if name.is_empty() || name.starts_with(|c: char| c.is_ascii_digit()) || is_denied(name) {
        return None;
    }
    Some((name.to_string(), kind))
}

/// R declarations: `name <- function(` and `name = function(` outside any brace or parenthesis.
fn r_decls(text: &str) -> Vec<Decl> {
    let masked = mask(text, &R_SYNTAX);
    let mut decls: Vec<Decl> = Vec::new();
    let (mut depth, mut parens) = (0usize, 0usize);
    // The declaration whose `{` has not appeared yet, and the one whose braces are open.
    let (mut waiting, mut body): (Option<usize>, Option<usize>) = (None, None);
    let mut recheck = false;
    for (index, (shown, original)) in masked.split('\n').zip(text.split('\n')).enumerate() {
        let number = u32::try_from(index + 1).unwrap_or(u32::MAX);
        let line = shown.trim();
        if line.is_empty() {
            continue;
        }
        // A body that does not start on the line after the header, with the parameters closed, belongs to no one.
        if recheck && !line.starts_with('{') {
            waiting = None;
        }
        if depth == 0
            && parens == 0
            && waiting.is_none()
            && let Some(name) = r_name(line)
        {
            decls.push(Decl { name, kind: "fn", lines: [number, number], signature: signature_of(original) });
            waiting = Some(decls.len() - 1);
        }
        for byte in shown.bytes() {
            match byte {
                b'{' => {
                    depth += 1;
                    if depth == 1 {
                        body = waiting.take();
                    }
                }
                b'}' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0
                        && let Some(at) = body.take()
                    {
                        decls[at].lines[1] = number;
                    }
                }
                b'(' => parens += 1,
                b')' => parens = parens.saturating_sub(1),
                _ => {}
            }
        }
        recheck = waiting.is_some() && parens == 0;
    }
    decls
}

/// The name when `line` starts `name <- function(`, `name <<- function(` or `name = function(`.
fn r_name(line: &str) -> Option<String> {
    let end = line.find(|c: char| !(c.is_alphanumeric() || c == '.' || c == '_')).unwrap_or(line.len());
    let name = &line[..end];
    if !name.starts_with(|c: char| c.is_alphabetic() || c == '.') || is_denied(name) {
        return None;
    }
    let rest = line[end..].trim_start();
    let value = rest.strip_prefix("<<-").or_else(|| rest.strip_prefix("<-")).or_else(|| rest.strip_prefix('=').filter(|after| !after.starts_with('=')))?.trim_start();
    let is_function = ident_at(value) == Some("function") && value["function".len()..].trim_start().starts_with('(');
    is_function.then(|| name.to_string())
}
