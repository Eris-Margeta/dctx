//! Call references of the brace family: Scala, Dart, Zig, Solidity, Gleam and Groovy.
//! Each function symbol's span is scanned on the comment- and string-masked source for `name(` call sites.
//! The references are raw: `refs.rs` resolves them (same file first, then a unique project name) and drops ambiguous ones.
//! Heuristic by design: no types, no receivers; a call is a bare identifier followed by an opening parenthesis.

use std::collections::HashMap;

use dctx_map_model::{EdgeKind, NodeKind};

use super::{brace, is_denied, mask};
use crate::index::extract::Symbol;
use crate::index::refs::RawRef;
use crate::index::write_stable_id;

/// Words that look like a call (`word (`) but are control flow, a declaration keyword or a language operator.
const NOT_CALLS: &[&str] = &[
    "if", "while", "for", "switch", "match", "return", "catch", "function", "fn", "def", "new", "sizeof", "require", "emit", "assert",
    "returns", "modifier", "event", "constructor", "import", "extends", "with", "this", "super",
];

/// Words after which the next identifier is a declared name, so `name(` there is a declaration, not a call.
const DECLARES: &[&str] = &["def", "fn", "function", "event", "modifier", "constructor", "class", "object", "trait", "interface", "contract", "library", "type", "enum"];

/// Raw `calls` references of every function symbol of `text`. Anything but a brace-family tag yields none.
///
/// `src` is the caller's stable id exactly as the index writes it (`s:<path>#<name>`, `~N` for a repeated name) and
/// `scope` is the caller's qualified name, so `refs.rs` looks callees up from the caller's own scope outward.
pub(super) fn references(language: &str, path: &str, text: &str, symbols: &[Symbol]) -> Vec<RawRef> {
    let Some(syntax) = brace::syntax_of(language) else {
        return Vec::new();
    };
    let masked = mask(text, &syntax);
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

/// True for a byte that can be part of an identifier. Non-ASCII bytes count, so words never split a character.
fn is_word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$' || byte >= 0x80
}

/// Called names in `body`, in source order and without repeats.
///
/// A call is an identifier followed by `(`, with one space or tab allowed between. Keywords, declarations, `@builtin(`
/// forms and the function's own name on its declaration line (the first line of `body`) are not calls.
fn call_names<'a>(body: &'a str, own: &str) -> Vec<&'a str> {
    let bytes = body.as_bytes();
    let first_line_end = body.find('\n').unwrap_or(body.len());
    let mut found: Vec<&str> = Vec::new();
    let mut previous: Option<(&str, usize)> = None;
    let mut at = 0usize;
    while at < bytes.len() {
        if !is_word(bytes[at]) {
            at += 1;
            continue;
        }
        let end = bytes[at..].iter().position(|byte| !is_word(*byte)).map_or(bytes.len(), |offset| at + offset);
        let word = &body[at..end];
        let declared = previous.is_some_and(|(prior, prior_end)| DECLARES.contains(&prior) && body[prior_end..at].trim().is_empty());
        previous = Some((word, end));
        let open = end + usize::from(matches!(bytes.get(end), Some(b' ' | b'\t')));
        let calls = bytes.get(open) == Some(&b'(') && !word.starts_with(|c: char| c.is_ascii_digit());
        let builtin = at > 0 && bytes[at - 1] == b'@';
        let own_declaration = at < first_line_end && word == own;
        if calls && !declared && !builtin && !own_declaration && !NOT_CALLS.contains(&word) && !is_denied(word) && !found.contains(&word) {
            found.push(word);
        }
        at = end;
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::generic::symbols;

    fn refs_of(tag: &str, path: &str, text: &str) -> Vec<RawRef> {
        references(tag, path, text, &symbols(tag, text))
    }

    /// (caller name, callee) pairs.
    fn pairs(tag: &str, text: &str) -> Vec<(String, String)> {
        refs_of(tag, "a.src", text).into_iter().map(|reference| (reference.scope, reference.name)).collect()
    }

    fn pair(caller: &str, callee: &str) -> (String, String) {
        (caller.to_string(), callee.to_string())
    }

    #[test]
    fn every_brace_language_links_a_caller_to_its_callee() {
        let scala = "object M {\n  def scRun(x: Int): Int = scHelp(x)\n  def scHelp(y: Int): Int = y + 1\n}\n";
        assert_eq!(pairs("scala", scala), [pair("M::scRun", "scHelp")]);
        let dart = "int dHelp(int x) {\n  return x;\n}\nint dRun() {\n  return dHelp(1);\n}\n";
        assert_eq!(pairs("dart", dart), [pair("dRun", "dHelp")]);
        let zig = "fn zHelp() u32 {\n    return 1;\n}\npub fn zRun() u32 {\n    return zHelp();\n}\n";
        assert_eq!(pairs("zig", zig), [pair("zRun", "zHelp")]);
        let sol = "contract T {\n    function solHelp() internal returns (uint) {\n        return 1;\n    }\n    function solRun() public returns (uint) {\n        return solHelp();\n    }\n}\n";
        assert_eq!(pairs("solidity", sol), [pair("T::solRun", "solHelp")]);
        let gleam = "fn gHelp() {\n  1\n}\npub fn gRun() {\n  gHelp()\n}\n";
        assert_eq!(pairs("gleam", gleam), [pair("gRun", "gHelp")]);
        let groovy = "class G {\n  def gvHelp(a) {\n    return a\n  }\n  def gvRun(a) {\n    return gvHelp (a)\n  }\n}\n";
        assert_eq!(pairs("groovy", groovy), [pair("G::gvRun", "gvHelp")]);
    }

    #[test]
    fn the_source_id_is_spelled_as_the_index_writes_it() {
        let text = "object M {\n  def one() = two()\n  def two() = 1\n}\nobject N {\n  def one() = two()\n  def two() = 1\n}\n";
        let found = refs_of("scala", "src/m.scala", text);
        let sources: Vec<_> = found.iter().map(|reference| (reference.src.as_str(), reference.scope.as_str(), reference.name.as_str())).collect();
        assert_eq!(sources, [("s:src/m.scala#M::one", "M::one", "two"), ("s:src/m.scala#N::one", "N::one", "two")]);
        assert!(found.iter().all(|reference| reference.kind == EdgeKind::Calls && reference.alias.is_none()));
        let twice = "def f() = g()\ndef f() = h()\n";
        let found = refs_of("scala", "t.scala", twice);
        assert_eq!(found.iter().map(|reference| reference.src.as_str()).collect::<Vec<_>>(), ["s:t.scala#f", "s:t.scala#f~2"]);
    }

    #[test]
    fn keywords_are_not_calls() {
        let body = "x = if (a) 1 else 2\nwhile (b) {}\nfor (i <- 1) {}\nswitch (c) {}\nmatch (d) {}\nreturn (e)\ncatch (f) {}\nfunction (g) {}\nfn (h) {}\ndef (i) {}\nnew (j)\nsizeof(k)\nrequire(l)\nemit (m)\nassert(n)\nreal (1)\nother(2)\n";
        assert_eq!(call_names(body, "none"), ["real", "other"]);
        let sol = "contract C {\n    function f() public returns (uint) {\n        emit Sent(1);\n        require(ok);\n        return g();\n    }\n}\n";
        // `emit Name(..)` names an event, which is a function symbol: the event is called, `emit` is not.
        assert_eq!(pairs("solidity", sol), [pair("C::f", "Sent"), pair("C::f", "g")]);
    }

    #[test]
    fn a_definition_never_calls_itself_on_its_declaration_line() {
        assert!(pairs("zig", "pub fn zRun() u32 {\n    return 1;\n}\n").is_empty());
        assert!(pairs("dart", "int dRun(int x) {\n  return x;\n}\n").is_empty());
        assert_eq!(pairs("scala", "object M {\n  def run(x: Int): Int = other(run2(x))\n}\n"), [pair("M::run", "other"), pair("M::run", "run2")]);
        // A nested declaration is not a call either.
        assert!(pairs("gleam", "fn outer() {\n  fn inner() {\n    1\n  }\n}\n").is_empty());
    }

    #[test]
    fn masked_comments_and_strings_produce_no_calls() {
        let text = "def run(x: Int): Int = {\n  // hidden(1)\n  /* nested(2) /* inner(3) */ still(4) */\n  val s = \"text(5) \\\" quoted(6)\"\n  \"\"\"triple(7)\n  more(8)\"\"\"\n  shown(9)\n}\n";
        assert_eq!(pairs("scala", text), [pair("run", "shown")]);
        let zig = "fn zRun() void {\n    // zHidden();\n    const s = \"zStr()\";\n    zShown();\n    @import(\"std\");\n}\n";
        assert_eq!(pairs("zig", zig), [pair("zRun", "zShown")]);
    }

    #[test]
    fn types_events_and_odd_input_stay_quiet() {
        assert!(pairs("scala", "class A(x: Int) {\n  val y = make(x)\n}\n").is_empty());
        assert!(refs_of("unknown", "a.src", "def f() = g()").is_empty());
        for sample in ["", "def f(", "fn f() {", "def f() = \u{e9}\u{1F600}(1)\n", "\u{0}\r\n\r\ndef g() = h ()\r\n"] {
            for tag in ["scala", "dart", "zig", "solidity", "gleam", "groovy"] {
                let _ = refs_of(tag, "a.src", sample);
            }
        }
        assert_eq!(pairs("scala", "def g() = h ()\r\n"), [pair("g", "h")]);
    }
}
