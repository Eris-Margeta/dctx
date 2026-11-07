// SPDX-License-Identifier: Apache-2.0
//! Call references of the lisp outline family: Clojure, Common Lisp, Scheme and Emacs Lisp (Nix has none).
//! Each function symbol's span is scanned on the comment- and string-masked source for `(name ...)` forms.
//! The references are raw: `refs.rs` resolves them (same file first, then a unique project name) and drops the rest.
//! Heuristic by design: no macro expansion; a call is a `(` directly followed by a symbol that is no special form.

use std::collections::HashMap;

use dctx_map_model::{EdgeKind, NodeKind};

use super::is_denied;
use super::lisp::{handles, mask_lisp, token};
use crate::index::extract::Symbol;
use crate::index::refs::RawRef;
use crate::index::write_stable_id;

/// Heads of special forms, binding forms and definitions: `(word ...)` with these is syntax, not a call.
const SYNTAX: &[&str] = &[
    "if",
    "when",
    "unless",
    "cond",
    "case",
    "let",
    "let*",
    "letrec",
    "letrec*",
    "lambda",
    "fn",
    "fn*",
    "do",
    "begin",
    "progn",
    "loop",
    "recur",
    "quote",
    "quasiquote",
    "unquote",
    "and",
    "or",
    "not",
    "define",
    "define-syntax",
    "define-record-type",
    "defn",
    "defn-",
    "defmacro",
    "def",
    "defun",
    "defvar",
    "defparameter",
    "defconstant",
    "defconst",
    "defcustom",
    "defclass",
    "defstruct",
    "defprotocol",
    "defrecord",
    "deftype",
    "defmulti",
    "defmethod",
    "defgeneric",
    "set!",
    "setq",
    "setf",
    "dolist",
    "dotimes",
    "while",
    "try",
    "catch",
    "finally",
    "throw",
    "binding",
    "for",
    "doseq",
    "ns",
    "require",
    "import",
    "use",
    "declare",
    "comment",
    "when-let",
    "if-let",
    "let-values",
    "case-lambda",
    "lambda-case",
    "destructuring-bind",
    "multiple-value-bind",
    "labels",
    "flet",
    "macrolet",
    "function",
    "with-eval-after-load",
    "provide",
    "cl-defun",
];

/// Raw `calls` references of every function symbol of `text`. Anything but a lisp tag yields none.
///
/// `src` is the caller's stable id exactly as the index writes it (`s:<path>#<name>`, `~N` for a repeated name) and
/// `scope` is the caller's qualified name, so `refs.rs` looks callees up from the caller's own scope outward.
pub(super) fn references(
    language: &str,
    path: &str,
    text: &str,
    symbols: &[Symbol],
) -> Vec<RawRef> {
    if !handles(language) || language == "nix" {
        return Vec::new();
    }
    let masked = mask_lisp(text, language);
    let mut seen: HashMap<&str, u32> = HashMap::new();
    let mut refs = Vec::new();
    for symbol in symbols {
        let count = seen.entry(symbol.name.as_str()).or_insert(0);
        *count += 1;
        let ordinal = if *count == 1 { 0 } else { *count };
        if symbol.kind != "fn" || symbol.graph != NodeKind::Symbol {
            continue;
        }
        let Some(body) = body_of(&masked, symbol.span) else {
            continue;
        };
        let mut src = String::new();
        write_stable_id(&mut src, b's', path, &symbol.name, ordinal);
        let own = symbol.name.rsplit("::").next().unwrap_or(&symbol.name);
        for name in call_names(body, own) {
            refs.extend(RawRef::new(&src, EdgeKind::Calls, name, &symbol.name, None));
        }
    }
    refs
}

/// The masked source of one symbol's byte span, or `None` when the span is empty or not on character boundaries.
fn body_of(masked: &str, span: [u64; 2]) -> Option<&str> {
    let start = usize::try_from(span[0]).ok()?;
    let end = usize::try_from(span[1]).ok()?;
    if start >= end {
        return None;
    }
    masked.get(start..end)
}

/// Called names in `body`, in source order and without repeats.
///
/// A call is a `(` that is not quoted (`'(`, `` `( ``) followed at once by a symbol that starts with a letter or `_`, is no
/// special form and is not the function's own name on its declaration line (the first line of `body`).
fn call_names<'a>(body: &'a str, own: &str) -> Vec<&'a str> {
    let bytes = body.as_bytes();
    let first_line_end = body.find('\n').unwrap_or(body.len());
    let mut found: Vec<&str> = Vec::new();
    for (at, byte) in bytes.iter().enumerate() {
        if *byte != b'(' || (at > 0 && matches!(bytes[at - 1], b'\'' | b'`')) {
            continue;
        }
        let (word, _) = token(body, at + 1);
        let adjacent = bytes
            .get(at + 1)
            .is_some_and(|next| !next.is_ascii_whitespace());
        let named = word.starts_with(|c: char| c.is_alphabetic() || c == '_');
        let own_declaration = at < first_line_end && word == own;
        if adjacent
            && named
            && !own_declaration
            && !SYNTAX.contains(&word)
            && !is_denied(word)
            && !found.contains(&word)
        {
            found.push(word);
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::generic::symbols;

    /// `(src, name, scope)` of every reference `text` yields for `tag`.
    fn calls(tag: &str, path: &str, text: &str) -> Vec<(String, String, String)> {
        references(tag, path, text, &symbols(tag, text))
            .into_iter()
            .map(|r| (r.src, r.name, r.scope))
            .collect()
    }

    /// One expected `(src, name, scope)` row.
    fn row(src: &str, name: &str, scope: &str) -> (String, String, String) {
        (src.to_string(), name.to_string(), scope.to_string())
    }

    /// A function calls the symbols in head position; syntax, quoted lists, comments, strings and its own declaration are not calls.
    #[test]
    fn heads_naming_functions_are_calls() {
        let clj = "(defn helper [x] x)\n(defn run [a]\n  ; (hidden a)\n  (let [b (helper a)]\n    (when b (println \"(nope 1)\" '(quoted 2) (helper b)))))\n";
        assert_eq!(
            calls("clojure", "a.clj", clj),
            [
                row("s:a.clj#run", "helper", "run"),
                row("s:a.clj#run", "println", "run")
            ]
        );
        let scm = "(define (cube x) (* x x x))\n(define (twice y)\n  (cube (cube y)))\n";
        assert_eq!(
            calls("scheme", "c.scm", scm),
            [row("s:c.scm#twice", "cube", "twice")]
        );
    }

    /// The second copy of a name carries the `~2` ordinal the index gives it; a protocol method is scoped by its protocol.
    #[test]
    fn duplicate_names_get_ordinals_and_scopes_are_qualified() {
        let text = "(defun f () 1)\n(defun f ()\n  (g))\n(defun g () 2)\n";
        assert_eq!(
            calls("lisp", "d.lisp", text),
            [row("s:d.lisp#f~2", "g", "f")]
        );
        let protocol = "(defprotocol Shape\n  (area [s])\n  (grow [s] (area s)))\n";
        assert_eq!(
            calls("clojure", "p.clj", protocol),
            [row("s:p.clj#Shape::grow", "area", "Shape::grow")]
        );
        assert!(calls("nix", "a.nix", "{ f = x: g x; }\n").is_empty());
    }
}
