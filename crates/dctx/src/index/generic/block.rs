//! Block-structured declarations: Elixir, Lua and Julia (blocks closed by `end`), R (braces), and through [`layout`] OCaml and Haskell.
//! The `end` languages share one scanner: a table says which words open or close a block and how a declaration line reads.
//! A declaration's frame is the first block opened on its line, so its end line is where that block closes; bodies are never entered.
//! Heuristic by design: one declaration per line, nesting as `Parent::child` inside a module or struct.
//! Each language lives in a sibling file: its comment mask, its block words and how a line declares a type, a function or a member.

mod elixir;
mod haskell;
mod julia;
mod layout;
mod ocaml_body;
mod lua;
mod r;
mod verilog;
#[cfg(test)]
mod tests;

use super::{signature_of, to_symbols, Decl};
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
    /// Name and wire kind when a trimmed, masked line declares something. The second argument is the same line unmasked (same
    /// offsets, for names inside strings), the third the qualified name of the enclosing type.
    decl: fn(&str, &str, Option<&str>) -> Option<(String, &'static str)>,
    /// What a word does to the block stack, given the byte that follows it.
    word: fn(&str, Option<u8>) -> Tok,
    /// True when words inside brackets are ignored (Julia's `a[end]` and comprehensions).
    flat: bool,
    /// True when functions declared inside a function body are symbols too, named `outer::inner` (Lua).
    nested: bool,
}

/// True for the six tags of this family.
pub(super) fn handles(language: &str) -> bool {
    matches!(language, "elixir" | "lua" | "ocaml" | "r" | "julia" | "haskell" | "verilog")
}

/// Symbols of `text`, or none for an unknown tag.
pub(super) fn symbols(tag: &str, text: &str) -> Vec<Symbol> {
    let decls = match tag {
        "elixir" => elixir::finish(scan(&elixir::SCAN, text)),
        "lua" => scan(&lua::SCAN, text),
        "julia" => scan(&julia::SCAN, text),
        "r" => r::decls(text),
        "ocaml" => layout::ocaml(text),
        "haskell" => haskell::haskell(text),
        "verilog" => scan(&verilog::SCAN, text),
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
    /// True inside a function body (with [`Block::nested`]): only nested functions are declared here.
    body: bool,
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
        let in_body = stack.last().is_some_and(|frame| frame.body);
        let owner = stack.last().and_then(|frame| frame.container.as_deref());
        let source = original.get(shown.len() - shown.trim_start().len()..).unwrap_or("");
        if open.is_none()
            && in_scope
            && at_top
            && let Some((name, kind)) = (block.decl)(line, source, owner)
            && (!in_body || kind == "fn")
        {
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
                    let (container, body) = match decl {
                        Some(at) if decls[at].kind == "type" => (Some(decls[at].name.clone()), false),
                        Some(at) if block.nested && decls[at].kind == "fn" => (Some(decls[at].name.clone()), true),
                        Some(_) => (None, false),
                        // A loop or branch inside a function body is still that body.
                        None => match stack.last() {
                            Some(frame) if frame.body => (frame.container.clone(), true),
                            _ => (None, false),
                        },
                    };
                    stack.push(Frame { container, decl, body });
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

