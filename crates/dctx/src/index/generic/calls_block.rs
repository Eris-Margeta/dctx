// SPDX-License-Identifier: Apache-2.0
//! Call references of the block outline family: Lua, Elixir, Julia, R, OCaml, Haskell, Erlang, Verilog and Fortran.
//! Lua, Elixir, Julia and R call with parentheses: an identifier followed by `(` (one space allowed) inside a function span.
//! OCaml and Haskell call by juxtaposition: any identifier in a body is a candidate; `refs.rs` resolves each, so a cross-file
//! callee links while a plain local does not. `decl_pair.rs` pairs a declaration (`.mli` prototype, header, `.d.ts`) with its definition.
//! Erlang calls like Lua: `name(` inside a function span, or `module:name(` for a call that keeps the module, so the
//! resolver looks it up in that module's file. Verilog adds module-scope calls and module instantiation lines in [`calls_verilog`].
//! Every span is scanned on the comment- and string-masked source; `refs.rs` resolves the raw references and drops ambiguous ones.
//! Heuristic by design: no types, no receivers, no imports. Consumes authorized UTF-8 from extract; never panics on malformed input.

use std::collections::HashMap;

use dctx_map_model::{EdgeKind, NodeKind};

use super::{mask, Syntax};
use crate::index::extract::Symbol;
use crate::index::refs::RawRef;
use crate::index::write_stable_id;

/// Words that look like a call (`word (`) but are control flow, a declaration keyword or a language operator.
const NOT_CALLS: &[&str] = &[
    "if", "then", "else", "elseif", "elsif", "end", "do", "while", "for", "function", "return", "local", "not", "and", "or", "when", "case",
    "cond", "fn", "def", "defp", "defmodule", "defmacro", "defmacrop", "defguard", "defguardp", "defdelegate", "defprotocol", "defimpl",
    "unless", "with", "receive", "try", "catch", "rescue", "after", "quote", "unquote", "import", "alias", "require", "use", "repeat", "until",
    "in", "macro", "struct", "module", "let", "begin", "switch", "next", "break", "isa", "where",
];

/// Words after which the next identifier is a declared name, so `name(` there is a declaration, not a call.
const DECLARES: &[&str] = &[
    "def", "defp", "defmacro", "defmacrop", "defguard", "defguardp", "defdelegate", "defmodule", "defprotocol", "function", "macro", "struct",
    "module",
];

/// Lua: `--[[ ]]` comments, `--` comments, quoted strings, `[[ ]]` long strings.
const LUA_LONG_COMMENT: Syntax = Syntax { line: &[], block: Some(("--[[", "]]")), nested: false, quotes: b"", triple: false };
/// Lua: `--` comments and quoted strings.
const LUA_LINE: Syntax = Syntax { line: &["--"], block: None, nested: false, quotes: b"\"'", triple: false };
/// Lua: `[[ ]]` long strings.
const LUA_LONG_STRING: Syntax = Syntax { line: &[], block: Some(("[[", "]]")), nested: false, quotes: b"", triple: false };
/// Elixir: `#` comments, quoted strings and heredocs.
const ELIXIR: Syntax = Syntax { line: &["#"], block: None, nested: false, quotes: b"\"'", triple: true };
/// Julia: nested `#= =#` comments.
const JULIA_BLOCK: Syntax = Syntax { line: &[], block: Some(("#=", "=#")), nested: true, quotes: b"", triple: false };
/// Julia: `#` comments and double-quoted strings.
const JULIA_LINE: Syntax = Syntax { line: &["#"], block: None, nested: false, quotes: b"\"", triple: true };
/// R: `#` comments, quoted strings.
const R: Syntax = Syntax { line: &["#"], block: None, nested: false, quotes: b"\"'", triple: false };
/// OCaml: nested `(* *)` comments and double-quoted strings.
const OCAML: Syntax = Syntax { line: &[], block: Some(("(*", "*)")), nested: true, quotes: b"\"", triple: false };
/// Haskell: `--` comments, nested `{- -}` comments and double-quoted strings.
const HASKELL: Syntax = Syntax { line: &["--"], block: Some(("{-", "-}")), nested: true, quotes: b"\"", triple: false };
/// Verilog: `//` comments.
const VERILOG_LINE: Syntax = Syntax { line: &["//"], block: None, nested: false, quotes: b"\"", triple: false };
/// Verilog: `/* */` comments.
const VERILOG_BLOCK: Syntax = Syntax { line: &[], block: Some(("/*", "*/")), nested: false, quotes: b"", triple: false };
/// Verilog: `"` strings.
const VERILOG_STRING: Syntax = Syntax { line: &[], block: None, nested: false, quotes: b"\"", triple: false };
/// Fortran: `!` comments and quoted strings.
const FORTRAN: Syntax = Syntax { line: &["!"], block: None, nested: false, quotes: b"\"'", triple: false };
/// Fortran words that precede `(` without calling anything (compared case-insensitively).
const FORTRAN_NOT_CALLS: &[&str] = &[
    "if", "else", "elseif", "while", "where", "forall", "select", "case", "do", "end", "function", "subroutine", "program", "module",
    "interface", "type", "class", "result", "real", "integer", "complex", "logical", "character", "double", "dimension", "intent",
    "allocatable", "allocate", "deallocate", "parameter", "write", "read", "print", "open", "close", "pointer", "kind", "len", "only",
    "use", "implicit", "external", "save", "optional", "target", "procedure", "bind", "associate", "return", "stop", "assignment",
    "operator", "pure", "elemental", "recursive", "contains", "data", "common", "equivalence", "format", "inquire", "rewind",
];
/// Erlang words that look like a call but are keywords, operators or guards.
const ERLANG_NOT_CALLS: &[&str] = &[
    "after", "and", "andalso", "band", "begin", "bnot", "bor", "bsl", "bsr", "bxor", "case", "catch", "cond", "div", "end", "fun", "if",
    "let", "maybe", "not", "of", "orelse", "receive", "rem", "try", "when", "xor",
];

/// The source with comments and strings blanked, or `None` for a tag of another family.
fn masked_of(language: &str, text: &str) -> Option<String> {
    Some(match language {
        "lua" => mask(&mask(&mask(text, &LUA_LONG_COMMENT), &LUA_LINE), &LUA_LONG_STRING),
        "elixir" => mask(text, &ELIXIR),
        "julia" => mask(&mask(text, &JULIA_BLOCK), &JULIA_LINE),
        "r" => mask(text, &R),
        "ocaml" => mask(text, &OCAML),
        // Erlang: `%` comments, quoted strings and single-quoted atoms, plus `$c` char literals as pairs.
        "erlang" => super::block::masked(text),
        "haskell" => mask(text, &HASKELL),
        "fortran" => mask(text, &FORTRAN),
        "verilog" => mask(&mask(&mask(text, &VERILOG_LINE), &VERILOG_BLOCK), &VERILOG_STRING),
        _ => return None,
    })
}

/// Raw `calls` references of every function symbol of `text`. Anything but a block-family tag yields none.
///
/// `src` is the caller's stable id exactly as the index writes it (`s:<path>#<name>`, `~N` for a repeated name) and
/// `scope` is the caller's qualified name, so `refs.rs` looks callees up from the caller's own scope outward.
pub(super) fn references(language: &str, path: &str, text: &str, symbols: &[Symbol]) -> Vec<RawRef> {
    let Some(masked) = masked_of(language, text) else {
        return Vec::new();
    };
    let juxtaposed = matches!(language, "ocaml" | "haskell");
    // In Verilog the enclosing module is a caller too, so module-scope `type` spans are scanned like function bodies.
    let verilog = language == "verilog";
    let mut seen: HashMap<&str, u32> = HashMap::new();
    let mut refs = Vec::new();
    for symbol in symbols {
        let count = seen.entry(symbol.name.as_str()).or_insert(0);
        *count += 1;
        let ordinal = if *count == 1 { 0 } else { *count };
        let scan = if verilog { matches!(symbol.kind.as_str(), "fn" | "type") } else { symbol.kind == "fn" };
        if !scan || symbol.graph != NodeKind::Symbol {
            continue;
        }
        let Some(body) = body_of(&masked, symbol.span) else {
            continue;
        };
        let own = tail(language, &symbol.name);
        let names = if language == "erlang" {
            erlang_call_names(body, own)
        } else if language == "fortran" {
            fortran_call_names(body, own)
        } else if verilog {
            super::calls_verilog::call_names(body, own)
        } else if juxtaposed {
            used_names(body, language, own)
        } else {
            call_names(body, language, own)
        };
        let mut src = String::new();
        write_stable_id(&mut src, b's', path, &symbol.name, ordinal);
        for name in names {
            refs.extend(RawRef::new(&src, EdgeKind::Calls, name, &symbol.name, None));
        }
    }
    refs
}

/// Called names in a Fortran `body`: `call name` and `name(`, case-insensitively not the function's own name or a keyword.
fn fortran_call_names<'a>(body: &'a str, own: &str) -> Vec<&'a str> {
    let bytes = body.as_bytes();
    let mut found: Vec<&str> = Vec::new();
    let mut after_call = false;
    let mut at = 0usize;
    while at < bytes.len() {
        if !is_word(bytes[at], false) {
            at += 1;
            continue;
        }
        let end = word_end(bytes, at, false);
        let word = &body[at..end];
        let next = bytes[end..].iter().find(|byte| !matches!(byte, b' ' | b'\t')).copied();
        let called = after_call || next == Some(b'(');
        let skipped = word.starts_with(|c: char| c.is_ascii_digit())
            || word.eq_ignore_ascii_case(own)
            || FORTRAN_NOT_CALLS.iter().any(|keyword| word.eq_ignore_ascii_case(keyword));
        if word.eq_ignore_ascii_case("call") && !after_call {
            after_call = true;
        } else {
            if called && !skipped && !found.contains(&word) {
                found.push(word);
            }
            after_call = false;
        }
        at = end;
    }
    found
}

/// Last segment of a qualified symbol name: `Mod::f`, and `Base.show` in Julia (a dot is part of an R name).
fn tail<'a>(language: &str, name: &'a str) -> &'a str {
    let last = name.rsplit("::").next().unwrap_or(name);
    if language == "julia" { last.rsplit('.').next().unwrap_or(last) } else { last }
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
fn is_word(byte: u8, dotted: bool) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte >= 0x80 || (dotted && byte == b'.')
}

/// End of the run of word bytes that starts at `at`.
fn word_end(bytes: &[u8], at: usize, dotted: bool) -> usize {
    bytes[at..].iter().position(|byte| !is_word(*byte, dotted)).map_or(bytes.len(), |offset| at + offset)
}

/// Called names in `body` for a parenthesis-call language, in source order and without repeats.
///
/// A call is an identifier followed by `(`, with one space or tab allowed between; `a.b(` and `a:b(` call `b`. Keywords,
/// declarations (`def name(`, `function a.b(`), `@macro(` forms, the function's own name and numbers are not calls.
fn call_names<'a>(body: &'a str, language: &str, own: &str) -> Vec<&'a str> {
    let bytes = body.as_bytes();
    let dotted = language == "r";
    let suffix: &[u8] = match language {
        "elixir" => b"?!",
        "julia" => b"!",
        _ => b"",
    };
    let mut found: Vec<&str> = Vec::new();
    let mut declaring = false;
    let mut at = 0usize;
    while at < bytes.len() {
        if !is_word(bytes[at], dotted) {
            at += 1;
            continue;
        }
        let mut end = word_end(bytes, at, dotted);
        if bytes.get(end).is_some_and(|byte| suffix.contains(byte)) && bytes.get(end + 1) == Some(&b'(') {
            end += 1;
        }
        let word = &body[at..end];
        let following = bytes[end..].iter().position(|byte| !matches!(byte, b' ' | b'\t'));
        let next = following.map(|offset| (offset, bytes[end + offset]));
        let before = at.checked_sub(1).map(|index| bytes[index]);
        if DECLARES.contains(&word) {
            declaring = next.is_some_and(|(_, byte)| is_word(byte, dotted));
        } else if declaring {
            // The name of a declaration, or a segment of its path: `function a.b(` declares `b`.
            declaring = matches!(bytes.get(end), Some(b'.' | b':'));
        } else {
            let open = matches!(next, Some((offset, b'(')) if offset <= 1);
            let skipped = before == Some(b'@') || word.starts_with(|c: char| c.is_ascii_digit()) || word == own || NOT_CALLS.contains(&word);
            if open && !skipped && !found.contains(&word) {
                found.push(word);
            }
        }
        at = end;
    }
    found
}

/// Names in `body` for a juxtaposition language, in source order and without repeats.
///
/// A call is any identifier that is not the function's own name, not a digit or quoted word, and not the head of a Haskell
/// equation line. Local bindings (`let`, parameters) are candidates too; `refs.rs` resolves each to the unique project symbol,
/// so a cross-file callee (e.g. `main.ml` calling `parse` from `lib.ml`) links while a plain local does not.
fn used_names<'a>(body: &'a str, language: &str, own: &str) -> Vec<&'a str> {
    let first_end = body.find('\n').map_or(body.len(), |at| at + 1);
    let head = &body[..first_end];
    let from = if (language == "haskell" && head.contains("::")) || head.trim_start().starts_with("val ") || head.trim_start().starts_with("external ") {
        first_end
    } else {
        0
    };
    let bytes = body.as_bytes();
    let mut found: Vec<&str> = Vec::new();
    let mut at = from;
    while at < bytes.len() {
        if !is_ident(bytes[at]) {
            at += 1;
            continue;
        }
        let end = bytes[at..].iter().position(|byte| !is_ident(*byte)).map_or(bytes.len(), |offset| at + offset);
        let word = &body[at..end];
        let equation_head = language == "haskell" && (at == 0 || bytes[at - 1] == b'\n');
        let usable = !equation_head && !word.starts_with(|c: char| c.is_ascii_digit() || c == '\'') && word != own;
        if usable && !found.contains(&word) {
            found.push(word);
        }
        at = end;
    }
    found
}

/// True for a byte that can be part of an OCaml or Haskell identifier (primes included).
fn is_ident(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'\'' || byte >= 0x80
}

/// Called names in `body` of an Erlang function, in source order and without repeats.
///
/// A call is an identifier followed by `(` (one space or tab allowed): a bare `name(`, or a qualified
/// `module:name(` that is spelled with its module so the resolver looks through the project's modules
/// and drops calls to modules outside the project. Erlang variables are uppercase-initial, keywords
/// and guards are never calls, and `?MACRO(` is not a call. The function never calls itself.
fn erlang_call_names<'a>(body: &'a str, own: &str) -> Vec<&'a str> {
    let bytes = body.as_bytes();
    let mut found: Vec<&str> = Vec::new();
    let mut at = 0usize;
    while at < bytes.len() {
        if !is_word(bytes[at], false) {
            at += 1;
            continue;
        }
        let start = at;
        let end = word_end(bytes, at, false);
        at = end;
        let word = &body[start..end];
        // Variables start uppercase or `_`, macros start `?`; only lowercase atoms call.
        let guarded = start > 0 && bytes[start - 1] == b'?';
        let variable = word.starts_with(|c: char| c.is_ascii_uppercase() || c == '_') || guards_word(word);
        if guarded || variable || word == own {
            continue;
        }
        if bytes.get(end) == Some(&b':') {
            // `module:name(`: the callee keeps its module, so `refs.rs` resolves it through the module's file.
            let callee_start = end + 1;
            let line_end = word_end(bytes, callee_start, false);
            let callee = &body[callee_start..line_end];
            if callee.is_empty() || callee.starts_with(|c: char| c.is_ascii_uppercase() || c == '_') {
                continue;
            }
            at = line_end;
            let gap = bytes[line_end..].iter().position(|byte| !matches!(byte, b' ' | b'\t')).unwrap_or(bytes.len());
            let spelled = &body[start..line_end];
            if gap <= 1 && bytes.get(line_end + gap) == Some(&b'(') && callee != own && !found.contains(&spelled) {
                found.push(spelled);
            }
            continue;
        }
        let gap = bytes[end..].iter().position(|byte| !matches!(byte, b' ' | b'\t')).unwrap_or(bytes.len());
        if gap <= 1 && bytes.get(end + gap) == Some(&b'(') && !found.contains(&word) {
            found.push(word);
        }
    }
    found
}

/// True when `word` is an Erlang keyword or guard word that may never name a call.
fn guards_word(word: &str) -> bool {
    ERLANG_NOT_CALLS.contains(&word)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
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

    /// Raw references whose callee is a function defined in `text`.
    ///
    /// Juxtaposition languages emit every identifier in a body as a candidate, so `refs.rs` decides which ones are calls.
    /// Keeping only the candidates that name a defined function restores the function-to-function links these tests assert.
    fn defined_refs(tag: &str, path: &str, text: &str) -> Vec<RawRef> {
        let symbols = symbols(tag, text);
        let declared: HashSet<&str> = symbols.iter().filter(|symbol| symbol.kind == "fn").map(|symbol| tail(tag, &symbol.name)).collect();
        refs_of(tag, path, text).into_iter().filter(|reference| declared.contains(reference.name.as_str())).collect()
    }

    #[test]
    fn every_block_language_links_a_caller_to_its_callee() {
        let lua = "local function luaHelp()\n  return 1\nend\nfunction luaRun()\n  return luaHelp()\nend\n";
        assert_eq!(pairs("lua", lua), [pair("luaRun", "luaHelp")]);
        let lua_methods = "function M.help()\n  return 1\nend\nfunction M:run()\n  return self:help() + M.help ()\nend\n";
        assert_eq!(pairs("lua", lua_methods), [pair("M::run", "help")]);
        let elixir = "defmodule Ex do\n  def ex_run(x) do\n    ex_help(x)\n  end\n\n  defp ex_help(y), do: y\nend\n";
        assert_eq!(pairs("elixir", elixir), [pair("Ex::ex_run", "ex_help")]);
        let one_liner = "defmodule Ex do\n  def go(x), do: x |> check?() |> step!()\nend\n";
        assert_eq!(pairs("elixir", one_liner), [pair("Ex::go", "check?"), pair("Ex::go", "step!")]);
        let julia = "function jHelp(x)\n    return x\nend\nfunction jRun(x)\n    return jHelp (x)\nend\n";
        assert_eq!(pairs("julia", julia), [pair("jRun", "jHelp")]);
        let r = "r_help <- function(x) {\n  x\n}\nr_run <- function(y) {\n  r_help(y)\n}\n";
        assert_eq!(pairs("r", r), [pair("r_run", "r_help")]);
        let erlang = "-module(shop).\n\nshop_run(X) ->\n    shop_help(X).\n\nshop_help(Y) ->\n    lists:sum(Y).\n";
        assert_eq!(pairs("erlang", erlang), [pair("shop_run", "shop_help"), pair("shop_help", "lists:sum")]);
        let fortran = "module geom\ncontains\n  function area(r)\n    area = square(r)\n  end function\n  function square(x)\n    square = x * x\n  end function\n  subroutine report()\n    call log_it()\n  end subroutine\n  subroutine log_it()\n  end subroutine\nend module\n";
        assert_eq!(pairs("fortran", fortran), [pair("geom::area", "square"), pair("geom::report", "log_it")]);
        let ocaml = "let ml_help x = x + 1\nlet ml_run y = ml_help y\n";
        assert_eq!(defined_refs("ocaml", "a.src", ocaml).into_iter().map(|reference| (reference.scope, reference.name)).collect::<Vec<_>>(), [pair("ml_run", "ml_help")]);
        let haskell = "hsHelp :: Int -> Int\nhsHelp x = x + 1\n\nhsRun :: Int -> Int\nhsRun y = hsHelp y\n";
        assert_eq!(defined_refs("haskell", "a.src", haskell).into_iter().map(|reference| (reference.scope, reference.name)).collect::<Vec<_>>(), [pair("hsRun", "hsHelp")]);
        let verilog = "module alu\n  function add(input int x)\n    return x\n  endfunction\n  task run(input v)\n    add(v)\n  endtask\nendmodule\nmodule top\n  alu #(.W(1)) u_alu (.a(p));\nendmodule\n";
        assert_eq!(pairs("verilog", verilog), [pair("alu", "add"), pair("alu::run", "add"), pair("top", "alu")]);
    }

    #[test]
    fn the_source_id_is_spelled_as_the_index_writes_it() {
        let text = "defmodule M do\n  def one(), do: two()\n  def two(), do: 1\nend\ndefmodule N do\n  def one(), do: two()\n  def two(), do: 1\nend\n";
        let found = refs_of("elixir", "lib/m.ex", text);
        let sources: Vec<_> = found.iter().map(|reference| (reference.src.as_str(), reference.scope.as_str(), reference.name.as_str())).collect();
        assert_eq!(sources, [("s:lib/m.ex#M::one", "M::one", "two"), ("s:lib/m.ex#N::one", "N::one", "two")]);
        assert!(found.iter().all(|reference| reference.kind == EdgeKind::Calls && reference.alias.is_none()));
        let twice = "let f x = g x\nlet g x = x\nlet f y = g y\n";
        let found = defined_refs("ocaml", "t.ml", twice);
        assert_eq!(found.iter().map(|reference| reference.src.as_str()).collect::<Vec<_>>(), ["s:t.ml#f", "s:t.ml#f~2"]);
    }

    #[test]
    fn keywords_are_not_calls() {
        let body = "if (a) then (b) elseif (c) end\nwhile (d) do (e)\nfor (f) in (g)\nfunction (h) return (i)\nlocal (j)\nnot (k) and (l) or (m)\nwhen (n) case (o) cond (p) fn (q)\ndef (r) defp (s) defmodule (t)\n3 (u) @attr (v) real (1) other(2)\n";
        for tag in ["lua", "elixir", "julia", "r"] {
            assert_eq!(call_names(body, tag, "none"), ["real", "other"], "{tag}");
        }
        let lua = "function f()\n  local g = function(x) return h(x) end\n  while (ready()) do\n    step()\n  end\nend\n";
        // The nested `g` is a symbol of its own (`f::g`); the outer body still holds every call.
        assert_eq!(pairs("lua", lua), [pair("f", "h"), pair("f", "ready"), pair("f", "step"), pair("f::g", "h")]);
    }

    #[test]
    fn a_definition_never_calls_itself() {
        assert!(pairs("lua", "function luaRun(x)\n  return x\nend\n").is_empty());
        assert!(pairs("lua", "function luaRun(x)\n  return luaRun(x - 1)\nend\n").is_empty());
        assert!(pairs("elixir", "defmodule E do\n  def run(x) do\n    run(x - 1)\n  end\nend\n").is_empty());
        assert!(pairs("julia", "function jRun(x)\n    jRun(x)\nend\n").is_empty());
        assert!(pairs("r", "r_run <- function(x) {\n  r_run(x)\n}\n").is_empty());
        assert!(pairs("erlang", "-module(s).\n\ns_gen(X) ->\n    s_gen(X).\n").is_empty());
        assert!(defined_refs("ocaml", "a.src", "let rec go n = if n = 0 then 0 else go (n - 1)\n").is_empty());
        assert!(defined_refs("haskell", "a.src", "go :: Int -> Int\ngo 0 = 0\ngo n = go (n - 1)\n").is_empty());
        // Only the juxtaposition languages need a declaration in the same file.
        assert!(defined_refs("ocaml", "a.src", "let run y = unknown y\n").is_empty());
        assert!(defined_refs("haskell", "a.src", "data Box = Box\nrun :: Box -> Box\nrun b = id b\n").is_empty());
    }

    #[test]
    fn masked_comments_and_strings_produce_no_calls() {
        let lua = "function luaRun()\n  -- hidden()\n  --[[ block() ]]\n  local s = \"str() \\\" quoted()\" .. [[long()]]\n  shown()\nend\n";
        assert_eq!(pairs("lua", lua), [pair("luaRun", "shown")]);
        let elixir = "defmodule E do\n  def run() do\n    # hidden()\n    s = \"str()\"\n    d = \"\"\"\n    doc()\n    \"\"\"\n    shown()\n  end\nend\n";
        assert_eq!(pairs("elixir", elixir), [pair("E::run", "shown")]);
        let julia = "function jRun()\n    #= nested(1) #= inner(2) =# still(3) =#\n    s = \"str()\" # tail()\n    shown()\nend\n";
        assert_eq!(pairs("julia", julia), [pair("jRun", "shown")]);
        let r = "r_run <- function() {\n  # hidden()\n  s <- 'str()'\n  shown()\n}\n";
        assert_eq!(pairs("r", r), [pair("r_run", "shown")]);
        let erlang = "-module(s).\n\ns_run(X) ->\n    %% hidden()\n    io:format(\"~p~n\", [str()]),\n    shown(X),\n    T = $X.\n";
        assert_eq!(pairs("erlang", erlang), [pair("s_run", "io:format"), pair("s_run", "str"), pair("s_run", "shown")]);
        let ocaml = "let a x = x\nlet b y =\n  (* a y *)\n  let s = \"a\" in\n  s ^ y\n";
        assert!(defined_refs("ocaml", "a.src", ocaml).is_empty());
        let haskell = "a :: Int\na = 1\nb :: Int\nb =\n  -- a\n  {- a -} 2 + length \"a\"\n";
        assert!(defined_refs("haskell", "a.src", haskell).is_empty());
        let verilog = "module alu\n  // hidden(x)\n  string s = \"str(x)\";\n  /* block(1) */\n  shown()\nendmodule\n";
        assert_eq!(pairs("verilog", verilog), [pair("alu", "shown")]);
    }

    #[test]
    fn odd_input_stays_quiet() {
        assert!(refs_of("unknown", "a.src", "function f() g() end").is_empty());
        for sample in ["", "function f(", "let f x =", "f <- function(", "def f() = \u{e9}\u{1F600}(1)\n", "\u{0}\r\n\r\nlet g = h ()\r\n"] {
            for tag in ["lua", "elixir", "julia", "r", "erlang", "ocaml", "haskell", "verilog", "fortran"] {
                let _ = refs_of(tag, "a.src", sample);
            }
        }
        assert_eq!(pairs("r", "my.run <- function() {\r\n  my.help ()\r\n}\r\n"), [pair("my.run", "my.help")]);
    }
}
