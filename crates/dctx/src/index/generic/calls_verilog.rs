// SPDX-License-Identifier: Apache-2.0
//! Call references of Verilog/SystemVerilog, used for `fn` and module-scope `type` spans.
//! A call is `<name>(` with at most one space before the parenthesis inside a module, function or task span; a keyword,
//! a digit-led word, a `.member(` or `$system(` word and the span's own name never call. A line that reads
//! `<module> [#(..)] <instance> (` instantiates a module: it reports `<module>` only, so the ports stay noise.
//! Heuristic by design: no types, no imports. Consumes authorized UTF-8 from extract; never panics on malformed input.

/// Words of the language that begin a statement, a declaration or a direction; none of them is an instantiation or a call.
const KEYWORDS: &[&str] = &[
    "module", "macromodule", "interface", "package", "class", "typedef", "enum", "struct", "function", "task", "input", "output",
    "inout", "wire", "reg", "logic", "bit", "byte", "int", "integer", "longint", "shortint", "time", "real", "realtime", "signed",
    "unsigned", "void", "string", "automatic", "static", "pure", "virtual", "parameter", "localparam", "assign", "import", "export",
    "genvar", "generate", "initial", "always", "always_comb", "always_ff", "always_latch", "if", "else", "case", "casex", "casez",
    "endcase", "wait", "for", "while", "repeat", "forever", "return", "begin", "end", "posedge", "negedge", "edge", "new", "this", "super",
];

/// Words that open a declaration line, whose name is a declaration and never a call.
const DECLARATIONS: &[&str] = &["module", "macromodule", "interface", "package", "class", "function", "task"];

/// True for a byte that continues an identifier (SystemVerilog names may carry `$`).
fn is_word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$' || byte >= 0x80
}

/// True when `line` starts with a declaration opener followed by a name, so the whole line carries no calls.
fn declares(line: &str) -> bool {
    DECLARATIONS.iter().any(|word| line.starts_with(word) && line[word.len()..].starts_with([' ', '\t', '(', ';']))
}

/// The identifier that starts `text`, or `None`.
fn word_at(text: &str) -> Option<&str> {
    let end = text.find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$')).unwrap_or(text.len());
    let word = &text[..end];
    (!word.is_empty() && !word.starts_with(|c: char| c.is_ascii_digit())).then_some(word)
}

/// `text` after a leading balanced `(...)` group, or unchanged when there is none.
fn after_group(text: &str) -> &str {
    let mut depth = 0usize;
    for (at, c) in text.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return &text[at + 1..];
                }
            },
            _ => {}
        }
    }
    text
}

/// The instantiated module's name when `line` reads `<module> [#(..)] <instance> (`, or `None`.
fn instantiation(line: &str) -> Option<&str> {
    let module = word_at(line)?;
    if KEYWORDS.contains(&module) {
        return None;
    }
    let mut rest = line[module.len()..].trim_start();
    if let Some(after) = rest.strip_prefix('#') {
        rest = after_group(after.trim_start());
    }
    let head = rest.trim_start();
    let instance = word_at(head)?;
    if KEYWORDS.contains(&instance) || instance == module {
        return None;
    }
    head[instance.len()..].trim_start().starts_with('(').then_some(module)
}

/// Called names in `body` (a comment- and string-masked span), in source order and without repeats.
/// Declaration lines report nothing; instantiation lines report the instantiated module.
pub(super) fn call_names<'a>(body: &'a str, own: &str) -> Vec<&'a str> {
    let mut found = Vec::new();
    for line in body.split('\n') {
        let line = line.trim();
        if line.is_empty() || declares(line) {
            continue;
        }
        if let Some(instantiated) = instantiation(line) {
            if instantiated != own && !found.contains(&instantiated) {
                found.push(instantiated);
            }
            continue;
        }
        line_calls(line, own, &mut found);
    }
    found
}

/// `<name>(` calls on one already masked line, in source order and without repeats.
/// Words inside a `"..."` literal never call, so odd unmasked input stays quiet.
fn line_calls<'a>(line: &'a str, own: &str, found: &mut Vec<&'a str>) {
    let bytes = line.as_bytes();
    let mut at = 0usize;
    while at < bytes.len() {
        if bytes[at] == b'"' {
            // Skip to the closing quote; an unclosed literal swallows the rest of the line.
            match bytes[at + 1..].iter().position(|byte| *byte == b'"') {
                Some(end) => at += end + 2,
                None => break,
            }
            continue;
        }
        if !is_word(bytes[at]) {
            at += 1;
            continue;
        }
        let start = at;
        while at < bytes.len() && is_word(bytes[at]) {
            at += 1;
        }
        let word = &line[start..at];
        let before = start.checked_sub(1).map(|index| bytes[index]);
        let gap = bytes[at..].iter().position(|byte| !matches!(byte, b' ' | b'\t'));
        let open = gap.is_some_and(|gap| gap <= 1 && bytes.get(at + gap) == Some(&b'('));
        let skipped = KEYWORDS.contains(&word) || word.starts_with(|c: char| c.is_ascii_digit() || c == '$') || before.is_some_and(|byte| byte == b'.' || byte == b'$') || word == own;
        if open && !skipped && !found.contains(&word) {
            found.push(word);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calls_and_instantiations_are_named_and_noise_is_quiet() {
        let module = "module alu\n  input p\n  add(p, q)\n  show(p)\n  alu #(.W(8)) u_alu (.a(p), .b(q), .y(r));\n  counter c0 (.clk(p));\nendmodule\n";
        assert_eq!(call_names(module, "alu"), ["add", "show", "counter"]);
        let own_is_quiet = "module alu\n  alu(self)\n  .add(self)\n  $display(1)\n  3(add)\nendmodule\n";
        assert_eq!(call_names(own_is_quiet, "alu"), Vec::<&str>::new());
        let declaration = "module alu\n  function automatic logic add(input logic [W-1:0] x, z);\nendmodule\n";
        assert_eq!(call_names(declaration, "alu"), Vec::<&str>::new());
        let broken = "module alu #(.W(8))\nfunction automatic logic [W-1:0] add(input\nal#(; $display(\"add(\", p);\nendmodule\n";
        assert_eq!(call_names(broken, "none"), Vec::<&str>::new());
    }
}
