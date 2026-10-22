//! Token stream for C# source: strings, XML doc comments and preprocessor lines.
//! Implements PARSERS-v1 lexing. Literals are masked, brackets are paired, and
//! nesting past the caller limit is a typed refusal. The structural walker consumes it.
mod scan;

use crate::domain::{Diagnostic, DiagnosticReason, LexError, Limits};

/// Token category. Keywords stay [`Kind::Word`] and are compared by text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// Identifier or keyword.
    Word,
    /// Character literal.
    Char,
    /// Numeric literal.
    Num,
    /// Operator or separator. `<` and `>` are always one character.
    Punct,
    /// A preprocessor line (`#if`, `#region`, …). Never evaluated.
    Preproc,
}

/// One significant token. String text is not a token; interpolation holes are.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Token {
    /// Category.
    pub kind: Kind,
    /// Inclusive byte start.
    pub start: u32,
    /// Exclusive byte end.
    pub end: u32,
}

/// A comment span. Doc comments are `///` (not `////`) and `/** */` (not `/**/`).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Comment {
    /// Inclusive byte start.
    pub start: u32,
    /// Exclusive byte end.
    pub end: u32,
    /// Whether the comment is an XML or doc comment.
    pub doc: bool,
}

/// Lexed tokens, comments, bracket partners and problems.
pub(crate) struct Lexed {
    /// Significant tokens in source order.
    pub tokens: Vec<Token>,
    /// Comments in source order.
    pub comments: Vec<Comment>,
    /// Partner token index, or [`NONE`] when the bracket is unmatched.
    pub partner: Vec<u32>,
    /// Lexical problems in source order.
    pub diagnostics: Vec<Diagnostic>,
    /// Byte offset of each line, starting at zero.
    pub line_starts: Vec<u32>,
}

/// Partner value of a bracket without a match.
pub(crate) const NONE: u32 = u32::MAX;

impl Lexed {
    /// One-based line of a byte offset. Rows advance on `\n` only.
    pub fn line(&self, byte: u32) -> u32 {
        line_of(&self.line_starts, byte)
    }
}

#[allow(clippy::cast_possible_truncation, reason = "Source offsets, line counts and token counts fit u32 because the public extractor enforces the 8 MiB input ceiling.")]
pub(crate) fn line_of(line_starts: &[u32], byte: u32) -> u32 {
    line_starts.partition_point(|&start| start <= byte) as u32
}

/// Whether `word` is a hard C# keyword. Contextual words stay identifiers.
pub(crate) fn keyword(word: &str) -> bool {
    matches!(
        word,
        "abstract"
            | "as"
            | "base"
            | "bool"
            | "break"
            | "byte"
            | "case"
            | "catch"
            | "char"
            | "checked"
            | "class"
            | "const"
            | "continue"
            | "decimal"
            | "default"
            | "delegate"
            | "do"
            | "double"
            | "else"
            | "enum"
            | "event"
            | "explicit"
            | "extern"
            | "false"
            | "finally"
            | "fixed"
            | "float"
            | "for"
            | "foreach"
            | "goto"
            | "if"
            | "implicit"
            | "in"
            | "int"
            | "interface"
            | "internal"
            | "is"
            | "lock"
            | "long"
            | "namespace"
            | "new"
            | "null"
            | "object"
            | "operator"
            | "out"
            | "override"
            | "params"
            | "private"
            | "protected"
            | "public"
            | "readonly"
            | "ref"
            | "return"
            | "sbyte"
            | "sealed"
            | "short"
            | "sizeof"
            | "stackalloc"
            | "static"
            | "string"
            | "struct"
            | "switch"
            | "this"
            | "throw"
            | "true"
            | "try"
            | "typeof"
            | "uint"
            | "ulong"
            | "unchecked"
            | "unsafe"
            | "ushort"
            | "using"
            | "virtual"
            | "void"
            | "volatile"
            | "while"
    )
}

/// Built-in type names, including contextual ones (`dynamic`, `nint`).
pub(crate) fn predefined(word: &str) -> bool {
    matches!(
        word,
        "bool"
            | "byte"
            | "sbyte"
            | "char"
            | "decimal"
            | "double"
            | "float"
            | "int"
            | "uint"
            | "long"
            | "ulong"
            | "short"
            | "ushort"
            | "object"
            | "string"
            | "void"
            | "dynamic"
            | "nint"
            | "nuint"
    )
}

/// Declaration and member modifiers, including contextual ones.
pub(crate) fn modifier(word: &str) -> bool {
    matches!(
        word,
        "public"
            | "private"
            | "protected"
            | "internal"
            | "static"
            | "abstract"
            | "sealed"
            | "readonly"
            | "partial"
            | "virtual"
            | "override"
            | "async"
            | "extern"
            | "unsafe"
            | "volatile"
            | "const"
            | "required"
            | "file"
            | "new"
            | "event"
            | "fixed"
    )
}

/// Lexes `source` and pairs `()`, `[]` and `{}`.
#[allow(clippy::cast_possible_truncation, reason = "Source offsets, line counts and token counts fit u32 because the public extractor enforces the 8 MiB input ceiling.")]
pub(crate) fn lex(source: &str, limits: &Limits) -> Result<Lexed, LexError> {
    let mut line_starts = vec![0];
    for (index, byte) in source.bytes().enumerate() {
        if byte == b'\n' {
            line_starts.push(index as u32 + 1);
        }
    }
    let scanned = scan::scan(source, limits.max_depth);
    if let Some(byte) = scanned.too_deep_at {
        return Err(LexError::TooDeep {
            line: line_of(&line_starts, byte),
            limit: limits.max_depth,
        });
    }
    let mut problems = scanned.problems;
    let partner = pair(source, &scanned.tokens, &mut problems, limits.max_depth, &line_starts)?;
    problems.sort_by_key(|problem| problem.0);
    let diagnostics = problems
        .into_iter()
        .map(|(byte, reason)| Diagnostic {
            line: line_of(&line_starts, byte),
            byte,
            reason,
        })
        .collect();
    Ok(Lexed {
        tokens: scanned.tokens,
        comments: scanned.comments,
        partner,
        diagnostics,
        line_starts,
    })
}

#[allow(clippy::cast_possible_truncation, reason = "Source offsets, line counts and token counts fit u32 because the public extractor enforces the 8 MiB input ceiling.")]
fn pair(
    source: &str,
    tokens: &[Token],
    problems: &mut Vec<(u32, DiagnosticReason)>,
    max_depth: u32,
    line_starts: &[u32],
) -> Result<Vec<u32>, LexError> {
    let mut partner = vec![NONE; tokens.len()];
    let mut stack: Vec<(usize, u8)> = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        if token.kind != Kind::Punct {
            continue;
        }
        let text = crate::domain::slice(source, token.start, token.end);
        let Some(code) = bracket_code(text) else {
            continue;
        };
        if code < 3 {
            if u32::try_from(stack.len()).unwrap_or(u32::MAX) >= max_depth {
                return Err(LexError::TooDeep {
                    line: line_of(line_starts, token.start),
                    limit: max_depth,
                });
            }
            stack.push((index, code));
            continue;
        }
        match stack.pop() {
            Some((open, kind)) if kind + 3 == code => {
                partner[open] = index as u32;
                partner[index] = open as u32;
            }
            Some(unmatched) => {
                stack.push(unmatched);
                problems.push((token.start, DiagnosticReason::UnbalancedBracket));
            }
            None => problems.push((token.start, DiagnosticReason::UnbalancedBracket)),
        }
    }
    for (open, _) in stack {
        problems.push((tokens[open].start, DiagnosticReason::UnbalancedBracket));
    }
    Ok(partner)
}

fn bracket_code(text: &str) -> Option<u8> {
    match text {
        "(" => Some(0),
        "[" => Some(1),
        "{" => Some(2),
        ")" => Some(3),
        "]" => Some(4),
        "}" => Some(5),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(source: &str) -> Vec<String> {
        let lexed = lex(source, &Limits::default()).expect("lex");
        lexed
            .tokens
            .iter()
            .map(|token| source[token.start as usize..token.end as usize].to_string())
            .collect()
    }

    #[test]
    fn literals_hide_declaration_shaped_text_and_keep_holes() {
        let source = "class A { string s = \"class Decoy {}\"; string v = @\"class Hidden {}\"; string i = $\"x {sink.Flush()} y\"; string r = \"\"\"\nclass Raw {}\n\"\"\"; }";
        let lexed = lex(source, &Limits::default()).expect("lex");
        assert!(lexed.diagnostics.is_empty(), "{:?}", lexed.diagnostics);
        let owned = texts(source);
        assert!(!owned.iter().any(|text| text == "Decoy" || text == "Hidden" || text == "Raw"));
        assert!(owned.iter().any(|text| text == "Flush"));
        let nested = texts("class A<B<C>> {}");
        assert_eq!(nested.iter().filter(|text| text.as_str() == ">").count(), 2);
    }

    #[test]
    fn preprocessor_lines_are_tokens_and_depth_is_typed() {
        let lexed = lex("#if DEBUG\nclass A { }\n#endregion\n", &Limits::default()).expect("lex");
        assert!(lexed.tokens.iter().any(|token| token.kind == Kind::Preproc));
        assert_eq!(lexed.diagnostics, []);
        let open = lex("void M() {", &Limits::default()).expect("lex");
        assert!(open.diagnostics.iter().any(|d| d.reason == DiagnosticReason::UnbalancedBracket));
        let limits = Limits { max_depth: 2, ..Limits::default() };
        assert!(matches!(lex("{{{", &limits), Err(LexError::TooDeep { limit: 2, .. })));
        assert!(lex("{{}}", &limits).is_ok());
        let bad = lex("string s = \"unterminated\nclass Z {}", &Limits::default()).expect("lex");
        assert!(bad.diagnostics.iter().any(|d| d.reason == DiagnosticReason::UnterminatedString));
        assert!(texts("string s = \"unterminated\nclass Z {}").iter().any(|text| text == "Z"));
    }
}
