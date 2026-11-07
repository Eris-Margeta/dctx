// SPDX-License-Identifier: Apache-2.0
//! Lisp-family outlines: Clojure, Common Lisp, Scheme, Emacs Lisp (here) and Nix (in `nix`).
//! A symbol is a top-level `(def... name ...)` form found on comment- and string-masked text; the form's closing paren sets the end
//! line. `(define (name args) ...)` names the first symbol inside, a `(def name (fn ...))` value is a function, Clojure's `(ns x)` is
//! listed as a `type` (a module summary has no place in a symbol list), and a `defprotocol` nests its methods as `Proto::method`.
//! Heuristic by design: bodies are never entered and wrappers such as `progn` or `when` hide what they contain.
//! Consumes authorized UTF-8 from extract; never reads source, never panics on malformed input.

mod nix;
#[cfg(test)]
mod tests;

use super::{Decl, is_denied, signature_of, to_symbols};
use crate::index::extract::Symbol;

/// The language tags this family owns, as `extract::language_of` spells them.
const TAGS: &[&str] = &["clojure", "lisp", "scheme", "elisp", "nix"];

/// What a head word declares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    /// A function or macro.
    Fn,
    /// A protocol, record, class or namespace.
    Type,
    /// A structure whose name may be wrapped in a list of options: `(defstruct (point (:conc-name p-)) x y)`.
    Struct,
    /// A variable.
    Var,
    /// A constant.
    Const,
    /// Clojure `def`: a function when the value is `(fn ...)`, else a variable.
    Def,
    /// Scheme `define`: a function when the name is a list or the value is a lambda, else a variable.
    Define,
}

/// True when `language` is one of the five tags of this family.
pub(super) fn handles(language: &str) -> bool {
    TAGS.contains(&language)
}

/// Symbols of `text` for a lisp or `nix` tag. Anything else, and any malformed input, yields none.
pub(super) fn symbols(language: &str, text: &str) -> Vec<Symbol> {
    let decls = match language {
        "nix" => nix::decls(text),
        "clojure" | "lisp" | "scheme" | "elisp" => decls(language, text),
        _ => Vec::new(),
    };
    to_symbols(text, decls)
}

/// Byte offsets of line starts, to turn an offset into a 1-based line number.
pub(super) struct Lines(Vec<usize>);

impl Lines {
    /// Index of `text`.
    pub(super) fn new(text: &str) -> Self {
        let mut starts = vec![0usize];
        starts.extend(
            text.bytes()
                .enumerate()
                .filter(|(_, byte)| *byte == b'\n')
                .map(|(at, _)| at + 1),
        );
        Self(starts)
    }

    /// The 1-based line that holds byte `offset`.
    pub(super) fn of(&self, offset: usize) -> u32 {
        u32::try_from(self.0.partition_point(|start| *start <= offset)).unwrap_or(u32::MAX)
    }
}

/// Copy of `text` with comments, strings and character literals replaced by spaces. Newlines and every byte offset stay put.
/// `;` comments, `#| |#` block comments (nested; not Clojure), strings, `#\x` characters, Clojure `\x` and Emacs `?x`.
pub(super) fn mask_lisp(text: &str, language: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = bytes.to_vec();
    let mut at = 0usize;
    while at < bytes.len() {
        let next = bytes.get(at + 1).copied();
        let end = match bytes[at] {
            b';' => Some(
                bytes[at..]
                    .iter()
                    .position(|byte| *byte == b'\n')
                    .map_or(bytes.len(), |offset| at + offset),
            ),
            b'"' => Some(string_end(bytes, at)),
            b'#' if next == Some(b'|') && language != "clojure" => Some(block_end(bytes, at)),
            b'#' if next == Some(b'\\') => Some(char_end(text, at + 2)),
            b'\\' if language == "clojure" => Some(char_end(text, at + 1)),
            b'?' if language == "elisp"
                && at > 0
                && !bytes[at - 1].is_ascii_alphanumeric()
                && !matches!(bytes[at - 1], b'-' | b'_') =>
            {
                let from = if next == Some(b'\\') { at + 2 } else { at + 1 };
                text.get(from..)
                    .is_some_and(|rest| rest.chars().next().is_some_and(|c| !c.is_whitespace()))
                    .then(|| char_end(text, from))
            }
            _ => None,
        };
        match end {
            Some(end) => {
                for byte in out.iter_mut().take(end).skip(at) {
                    if *byte != b'\n' && *byte != b'\r' {
                        *byte = b' ';
                    }
                }
                at = end.max(at + 1);
            }
            None => at += 1,
        }
    }
    String::from_utf8(out).unwrap_or_default()
}

/// End of the string that opens at `from`; an unclosed string runs to the end of input.
fn string_end(bytes: &[u8], from: usize) -> usize {
    let mut at = from + 1;
    while at < bytes.len() {
        match bytes[at] {
            b'\\' => at += 2,
            b'"' => return at + 1,
            _ => at += 1,
        }
    }
    bytes.len()
}

/// End of the nested `#| ... |#` comment that opens at `from`; an unclosed comment runs to the end of input.
fn block_end(bytes: &[u8], from: usize) -> usize {
    let mut depth = 1usize;
    let mut at = from + 2;
    while at < bytes.len() {
        if bytes[at..].starts_with(b"|#") {
            depth -= 1;
            at += 2;
            if depth == 0 {
                return at;
            }
        } else if bytes[at..].starts_with(b"#|") {
            depth += 1;
            at += 2;
        } else {
            at += 1;
        }
    }
    bytes.len()
}

/// End of the character literal whose first character starts at `from`: one character, or a whole name such as `space`.
fn char_end(text: &str, from: usize) -> usize {
    let mut chars = text.get(from..).unwrap_or("").char_indices();
    let Some((_, first)) = chars.next() else {
        return text.len();
    };
    let mut end = from + first.len_utf8();
    if first.is_alphabetic() {
        for (offset, c) in chars {
            if !c.is_alphanumeric() {
                break;
            }
            end = from + offset + c.len_utf8();
        }
    }
    end
}

/// `(start, end)` byte ranges of the lists at paren depth 0 of `masked`; an unclosed list runs to the end.
fn forms(masked: &str) -> Vec<(usize, usize)> {
    let mut found = Vec::new();
    let mut depth = 0usize;
    let mut open = 0usize;
    for (at, byte) in masked.bytes().enumerate() {
        match byte {
            b'(' => {
                if depth == 0 {
                    open = at;
                }
                depth += 1;
            }
            b')' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    found.push((open, at + 1));
                }
            }
            _ => {}
        }
    }
    if depth > 0 {
        found.push((open, masked.len()));
    }
    found
}

/// True for a byte that separates tokens.
fn is_gap(byte: u8) -> bool {
    byte.is_ascii_whitespace()
        || matches!(
            byte,
            b',' | b'(' | b')' | b'[' | b']' | b'{' | b'}' | b'"' | b';'
        )
}

/// Offset of the first byte at or after `from` that is not whitespace or a comma.
fn skip_space(bytes: &[u8], mut from: usize) -> usize {
    while bytes
        .get(from)
        .is_some_and(|byte| byte.is_ascii_whitespace() || *byte == b',')
    {
        from += 1;
    }
    from
}

/// The token that starts at `from` (after blanks) and the offset where it ends.
pub(super) fn token(masked: &str, from: usize) -> (&str, usize) {
    let bytes = masked.as_bytes();
    let start = skip_space(bytes, from);
    let mut end = start;
    while end < bytes.len() && !is_gap(bytes[end]) {
        end += 1;
    }
    (masked.get(start..end).unwrap_or(""), end)
}

/// What the head word `head` declares, if anything.
fn shape_of(head: &str, language: &str) -> Option<Shape> {
    Some(match head {
        "defn" | "defn-" | "defmacro" | "defun" | "defgeneric" | "defsubst" | "defmulti"
        | "define-syntax" | "define-syntax-rule" | "cl-defun" | "cl-defmacro" | "cl-defgeneric" => {
            Shape::Fn
        }
        "defprotocol" | "defrecord" | "deftype" | "defclass" | "definterface" => Shape::Type,
        "ns" if language == "clojure" => Shape::Type,
        "defstruct" | "cl-defstruct" | "define-record-type" => Shape::Struct,
        "defvar" | "defparameter" | "defcustom" | "defvar-local" | "defglobal" => Shape::Var,
        "defconstant" | "defconst" => Shape::Const,
        "def" => Shape::Def,
        "define" => Shape::Define,
        _ => return None,
    })
}

/// Offset after the balanced `open ... close` group that starts at `from`.
fn skip_group(bytes: &[u8], from: usize, open: u8, close: u8) -> usize {
    let mut depth = 0usize;
    for (offset, byte) in bytes[from..].iter().enumerate() {
        if *byte == open {
            depth += 1;
        } else if *byte == close {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return from + offset + 1;
            }
        }
    }
    bytes.len()
}

/// The declared name and wire kind of the form that opened at `open`.
/// Clojure metadata (`^:private`, `^{...}`) before the name is skipped.
fn declared(masked: &str, open: usize, language: &str) -> Option<(String, &'static str)> {
    let bytes = masked.as_bytes();
    let (head, mut at) = token(masked, open + 1);
    let shape = shape_of(head, language)?;
    loop {
        at = skip_space(bytes, at);
        let meta = match (bytes.get(at), bytes.get(at + 1)) {
            (Some(b'^'), _) => at + 1,
            (Some(b'#'), Some(b'^')) => at + 2,
            _ => break,
        };
        at = if bytes.get(meta) == Some(&b'{') {
            skip_group(bytes, meta, b'{', b'}')
        } else {
            token(masked, meta).1
        };
    }
    let mut listed = false;
    if bytes.get(at) == Some(&b'(') && matches!(shape, Shape::Define | Shape::Struct) {
        listed = true;
        while bytes.get(at) == Some(&b'(') {
            at = skip_space(bytes, at + 1);
        }
    }
    let (name, end) = token(masked, at);
    let bad_start = name.starts_with(|c: char| {
        c.is_ascii_digit() || matches!(c, ':' | '\'' | '`' | '#' | '@' | '.' | '~')
    });
    if name.is_empty() || bad_start || is_denied(name) {
        return None;
    }
    let kind = match shape {
        Shape::Fn => "fn",
        Shape::Type | Shape::Struct => "type",
        Shape::Var => "var",
        Shape::Const => "const",
        Shape::Def | Shape::Define if listed => "fn",
        Shape::Def | Shape::Define => {
            let rest = skip_space(bytes, end);
            let lambda = bytes.get(rest) == Some(&b'(')
                && matches!(
                    token(masked, rest + 1).0,
                    "fn" | "fn*" | "lambda" | "named-lambda" | "\u{3bb}"
                );
            if lambda { "fn" } else { "var" }
        }
    };
    Some((name.to_string(), kind))
}

/// Declarations of the top-level forms of `text`, protocol methods right after their protocol.
fn decls(language: &str, text: &str) -> Vec<Decl> {
    let masked = mask_lisp(text, language);
    let lines = Lines::new(text);
    let first_line = |from: usize| {
        signature_of(
            text.get(from..)
                .and_then(|rest| rest.lines().next())
                .unwrap_or(""),
        )
    };
    let mut out = Vec::new();
    for (open, end) in forms(&masked) {
        let Some((name, kind)) = declared(&masked, open, language) else {
            continue;
        };
        out.push(Decl {
            name: name.clone(),
            kind,
            lines: [lines.of(open), lines.of(end.saturating_sub(1).max(open))],
            signature: first_line(open),
        });
        if token(&masked, open + 1).0 != "defprotocol" {
            continue;
        }
        let inner_end = if end > 0 && masked.as_bytes().get(end - 1) == Some(&b')') {
            end - 1
        } else {
            end
        };
        let inner = masked.get(open + 1..inner_end).unwrap_or("");
        for (from, to) in forms(inner) {
            let member = token(inner, from + 1).0;
            if member.is_empty()
                || is_denied(member)
                || member.starts_with(|c: char| {
                    c.is_ascii_digit() || matches!(c, ':' | '\'' | '`' | '#')
                })
            {
                continue;
            }
            let at = open + 1 + from;
            let last = open + 1 + to.saturating_sub(1).max(from);
            out.push(Decl {
                name: format!("{name}::{member}"),
                kind: "fn",
                lines: [lines.of(at), lines.of(last)],
                signature: first_line(at),
            });
        }
    }
    out
}
