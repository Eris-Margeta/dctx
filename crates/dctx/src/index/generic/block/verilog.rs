//! Verilog/SystemVerilog: modules, interfaces, packages, classes, functions and tasks.
//! Verilog is a block-family language: `module`, `macromodule`, `interface`, `package`, `class`, `function` and `task`
//! open a block and `endmodule`, `endinterface`, `endpackage`, `endclass`, `endfunction`, `endtask` close it, while
//! `begin` and `end` are neutral. `module name` (also `module name #(` and `module name(`) is a type;
//! `function [automatic] [type] name(` and `task [automatic] name(` are fns named `module::name` by the scanner.
//! Calls live in `calls_verilog.rs`. Heuristic by design: one declaration per line, bodies never entered.
//! Consumes authorized UTF-8 from extract; never panics on malformed or truncated input.

use super::super::{ident_at, is_denied, mask, Syntax};
use super::{Block, Tok};

/// How Verilog reads: `//` and `/* */` comments, `"` strings, `begin`/`end` neutral.
pub(super) const SCAN: Block = Block { mask: mask_of, decl: decl_of, word: word_of, flat: false, nested: true };

/// Verilog: `//` line comments and `/* */` block comments, then `"` strings on the masked text.
fn mask_of(text: &str) -> String {
    let comments = mask(text, &Syntax { line: &["//"], block: Some(("/*", "*/")), nested: false, quotes: b"", triple: false });
    mask(&comments, &Syntax { line: &[], block: None, nested: false, quotes: b"\"", triple: false })
}

/// Verilog blocks open at `module`, `macromodule`, `interface`, `package`, `class`, `function`, `task`
/// and close at `endmodule`, `endinterface`, `endpackage`, `endclass`, `endfunction`, `endtask`; `begin`/`end` are neutral.
fn word_of(word: &str, _next: Option<u8>) -> Tok {
    match word {
        "module" | "macromodule" | "interface" | "package" | "class" | "function" | "task" => Tok::Open,
        "endmodule" | "endinterface" | "endpackage" | "endclass" | "endfunction" | "endtask" => Tok::Close,
        _ => Tok::Neutral,
    }
}

/// Openers after which a line declares a type: the construct words of the language.
const TYPE_OPENERS: &[&str] = &["module", "macromodule", "interface", "package", "class"];

/// Type words that may stand between `function`/`task` and the declared name; none of them is ever the name itself.
const TYPE_WORDS: &[&str] = &[
    "automatic", "static", "logic", "reg", "wire", "bit", "byte", "int", "integer", "longint", "shortint", "time", "real", "realtime",
    "signed", "unsigned", "void", "string", "input", "output", "inout",
];

/// True for a byte that continues an identifier (SystemVerilog names may carry `$`).
fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

/// The identifier run that ends `text` after modifiers and type words are dropped, or `None`.
/// Uses `char_indices` so the slice after the cut is always on a character boundary;
/// never panics on malformed or truncated input.
fn trailing_ident(text: &str) -> Option<&str> {
    let word = text
        .char_indices()
        .rfind(|(_, c)| !is_word(*c))
        .map_or(text, |(cut, c)| &text[cut + c.len_utf8()..]);
    (!word.is_empty() && !word.starts_with(|c: char| c.is_ascii_digit()) && !TYPE_WORDS.contains(&word) && !is_denied(word)).then_some(word)
}

/// `module name` (also `module name #(` and `module name(`) is a type;
/// `function [automatic] [type] name(` and `task [automatic] name(` are fns named `module::name` by the scanner.
/// The name is the last identifier before the parameter list, so `automatic` and width types are skipped.
fn decl_of(line: &str, _source: &str, _owner: Option<&str>) -> Option<(String, &'static str)> {
    for opener in TYPE_OPENERS {
        if let Some(rest) = line.strip_prefix(opener).filter(|rest| rest.starts_with([' ', '\t', '('])) {
            let name = ident_at(rest.trim_start())?;
            return (!is_denied(name)).then(|| (name.to_string(), "type"));
        }
    }
    for opener in ["function", "task"] {
        let Some(rest) = line.strip_prefix(opener).filter(|rest| rest.starts_with([' ', '\t'])) else { continue };
        let head = rest.find('(').map_or(rest, |cut| &rest[..cut]);
        let name = trailing_ident(head.trim_end_matches(';').trim_end())?;
        return Some((name.to_string(), "fn"));
    }
    None
}
