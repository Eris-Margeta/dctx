//! What the body lines of an OCaml declaration add: the constructors and record fields of a `type` (`t::Leaf`, `t::name`), and the
//! functions a `let` defines inside its own body (`outer::loop`), which [`Nested`] follows by indentation. Pure functions of masked
//! lines; spans and the declaration list belong to `layout::ocaml`. Never panics on odd input.

use super::super::is_denied;
use super::layout::{ident_run, is_ident_char};

/// Constructors (`const`) and record fields (`field`) a masked, trimmed line of a type declaration lists. With `head` the line
/// is the declaration itself and only the text after its first `=` counts. `depth` is the record brace depth, carried from line to line.
pub(super) fn type_members(line: &str, head: bool, depth: &mut i32) -> Vec<(String, &'static str)> {
    let text = if head {
        match top_level_equals(line) {
            Some(at) => &line[at + 1..],
            None => return Vec::new(),
        }
    } else {
        line
    };
    let mut found = Vec::new();
    let mut parens = 0i32;
    let mut start = true;
    let mut skip_to = 0usize;
    for (at, c) in text.char_indices() {
        if at < skip_to {
            continue;
        }
        match c {
            '(' | '[' => parens += 1,
            ')' | ']' => parens = (parens - 1).max(0),
            '|' if parens == 0 && *depth == 0 => start = true,
            ';' if parens == 0 && *depth == 1 => start = true,
            '{' if parens == 0 => {
                *depth += 1;
                start = *depth == 1;
            }
            '}' if parens == 0 => *depth = (*depth - 1).max(0),
            c if c.is_whitespace() => {}
            _ if start && parens == 0 => {
                let rest = &text[at..];
                let (word, member) = if *depth == 0 { constructor(rest) } else { field(rest) };
                // `type t = private A | B`: the constructors follow the `private`.
                start = *depth == 0 && ident_run(rest) == "private";
                skip_to = at + word;
                if let Some(member) = member {
                    found.push(member);
                }
            }
            _ => {}
        }
    }
    found
}

/// Offset of the first `=` outside parentheses that is not part of `==`, `<=`, `>=` or `->`-like operators.
fn top_level_equals(line: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut parens = 0i32;
    for (at, &byte) in bytes.iter().enumerate() {
        match byte {
            b'(' | b'[' => parens += 1,
            b')' | b']' => parens -= 1,
            b'=' if parens == 0 => {
                let before = at.checked_sub(1).map(|prior| bytes[prior]);
                if !matches!(before, Some(b'=' | b'<' | b'>' | b'!' | b':')) && bytes.get(at + 1) != Some(&b'=') {
                    return Some(at);
                }
            }
            _ => {}
        }
    }
    None
}

/// `Leaf`, `Node of t * t` or `Conv : 'a -> t` at the start of `rest`: the bytes read and the constructor, if it is one.
/// `private` is skipped; a capitalised word followed by `.` is a module path, not a constructor.
fn constructor(rest: &str) -> (usize, Option<(String, &'static str)>) {
    let word = ident_run(rest);
    if word == "private" {
        return (word.len(), None);
    }
    let tail = &rest[word.len()..];
    let tail_trimmed = tail.trim_start();
    let ends = tail_trimmed.is_empty() || tail_trimmed.starts_with(['|', ':', ';', ')', '[']) || ident_run(tail_trimmed) == "of";
    let ok = word.starts_with(|c: char| c.is_uppercase()) && !tail.starts_with('.') && ends;
    (word.len(), ok.then(|| (word.to_string(), "const")))
}

/// `name : t` or `mutable name : t` at the start of `rest` inside a record: the bytes read and the field.
fn field(rest: &str) -> (usize, Option<(String, &'static str)>) {
    let mut skipped = 0usize;
    let mut text = rest;
    if let Some(after) = text.strip_prefix("mutable").filter(|after| after.starts_with(char::is_whitespace)) {
        skipped = rest.len() - after.trim_start().len();
        text = after.trim_start();
    }
    let word = ident_run(text);
    let ok = word.starts_with(|c: char| c.is_lowercase() || c == '_') && word != "_" && !is_denied(word) && text[word.len()..].trim_start().starts_with(':');
    (skipped + word.len(), ok.then(|| (word.to_string(), "field")))
}

/// A function defined inside a body: `let [rec] name args =`, `let name = fun ..` or `let name = function`, and the same after
/// `and`. The second value is true when the definition ends on its own line (`... in`).
pub(super) fn nested_fn(line: &str) -> Option<(String, bool)> {
    let word = ident_run(line);
    if !matches!(word, "let" | "and") {
        return None;
    }
    let mut rest = line[word.len()..].strip_prefix(char::is_whitespace)?.trim_start();
    if ident_run(rest) == "rec" {
        rest = rest["rec".len()..].trim_start();
    }
    let name = ident_run(rest);
    if !name.starts_with(|c: char| c.is_lowercase() || c == '_') || name == "_" || matches!(name, "open" | "module" | "exception" | "rec" | "in") || is_denied(name) {
        return None;
    }
    let after = rest[name.len()..].trim_start();
    let params = after.starts_with(|c: char| c.is_alphabetic() || matches!(c, '_' | '(' | '~' | '?')) && !after.starts_with("in ");
    let lambda = after.strip_prefix('=').is_some_and(|value| matches!(ident_run(value.trim_start()), "fun" | "function"));
    let one_line = line.rsplit(|c: char| !is_ident_char(c)).next() == Some("in");
    (params || lambda).then(|| (name.to_string(), one_line))
}

/// The functions open inside the body being read: declaration index and column, innermost last.
#[derive(Default)]
pub(super) struct Nested {
    /// Open nested functions.
    open: Vec<(usize, usize)>,
}

impl Nested {
    /// Forgets every open nested function (the enclosing declaration ended).
    pub(super) fn clear(&mut self) {
        self.open.clear();
    }

    /// Closes the nested functions a body line at `indent` ends: those indented past it, and one at its column unless the line
    /// is a `|` match arm. True when one at exactly this column closed, so an `and` here is its sibling.
    pub(super) fn close(&mut self, indent: usize, arm: bool) -> bool {
        let mut sibling = false;
        while let Some(&(_, column)) = self.open.last() {
            if column < indent || (column == indent && arm) {
                break;
            }
            sibling |= column == indent;
            self.open.pop();
        }
        sibling
    }

    /// The innermost open nested function, which a new one is named under.
    pub(super) fn innermost(&self) -> Option<usize> {
        self.open.last().map(|&(at, _)| at)
    }

    /// Every open nested function, so the caller can stretch their end lines.
    pub(super) fn indices(&self) -> impl Iterator<Item = usize> + '_ {
        self.open.iter().map(|&(at, _)| at)
    }

    /// Starts following the nested function `at` declared at `indent`.
    pub(super) fn push(&mut self, at: usize, indent: usize) {
        self.open.push((at, indent));
    }
}
