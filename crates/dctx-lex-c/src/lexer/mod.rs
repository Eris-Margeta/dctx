//! Token stream for C and C++ after line-splice removal.
//! Implements PARSERS-v1 lexing: comments, strings, raw strings, characters,
//! preprocessor lines and bracket nesting bounded by `Limits`.
//! The structural walker consumes [`Lexed`] and does not expand macros.
mod preprocessor;
mod scan;

use crate::domain::{Diagnostic, DiagnosticReason, LexError, Limits};

pub(crate) use preprocessor::{classify, hidden_include, Dir};

/// Token category. Keywords stay [`Kind::Ident`] and are compared by text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// Identifier or keyword.
    Ident,
    /// Numeric literal.
    Number,
    /// String, character or raw-string literal, quotes included.
    String,
    /// Unterminated string, character or raw string.
    Bad,
    /// Operator or delimiter.
    Punct,
    /// One preprocessor directive, from `#` through the end of the logical line.
    Directive,
}

/// One significant token. Offsets index the logical (splice-stripped) source.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Token {
    /// Category.
    pub kind: Kind,
    /// First logical byte.
    pub start: u32,
    /// One past the last logical byte.
    pub end: u32,
}

/// A comment span in the logical source.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Comment {
    /// First logical byte.
    pub start: u32,
    /// One past the last logical byte. Newlines are excluded for `//`.
    pub end: u32,
    /// `///`, `//!`, `/**` or `/*!`.
    pub doc: bool,
}

/// One backslash-newline removed from the physical source.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Splice {
    /// Logical offset where the splice used to sit.
    pub at: u32,
    /// Physical bytes removed, including the backslash and the newline.
    pub removed: u32,
    /// Physical newlines removed.
    pub newlines: u32,
}

/// Maps logical offsets back onto the original file.
#[derive(Debug, Clone)]
pub(crate) struct SourceMap {
    /// Splices in order.
    pub splices: Vec<Splice>,
    /// Logical byte offset of each line. Index 0 is byte 0.
    pub logical_line_starts: Vec<u32>,
    /// Physical line count, at least 1.
    pub physical_lines: u32,
}

impl SourceMap {
    /// One-based physical line of a logical byte.
    pub fn line(&self, logical: u32) -> u32 {
        let logical_line = self
            .logical_line_starts
            .partition_point(|&start| start <= logical) as u32;
        let extra: u32 = self
            .splices
            .iter()
            .take_while(|splice| splice.at <= logical)
            .map(|splice| splice.newlines)
            .sum();
        logical_line + extra
    }

    /// Physical byte of a logical byte. Exclusive ends stay exclusive.
    pub fn physical(&self, logical: u32) -> u32 {
        let extra: u32 = self
            .splices
            .iter()
            .take_while(|splice| splice.at <= logical)
            .map(|splice| splice.removed)
            .sum();
        logical.saturating_add(extra)
    }
}

/// Lexer output: logical text, tokens, comments, partners and problems.
pub(crate) struct Lexed {
    /// Source with line splices removed.
    pub logical: String,
    /// Significant tokens, directives included and macro bodies not rescanned.
    pub tokens: Vec<Token>,
    /// Comment spans in logical coordinates.
    pub comments: Vec<Comment>,
    /// Matching bracket index, or [`NONE`].
    pub partner: Vec<u32>,
    /// Lexical problems in physical coordinates.
    pub diagnostics: Vec<Diagnostic>,
    /// Logical-to-physical map.
    pub map: SourceMap,
}

/// Partner value of a bracket without a match.
pub(crate) const NONE: u32 = u32::MAX;

/// Lexes `source`, strips line splices and pairs `()`, `[]` and `{}`.
pub(crate) fn lex(source: &str, limits: &Limits) -> Result<Lexed, LexError> {
    let (logical, map) = prepare(source);
    let (tokens, comments, mut problems) = {
        let mut lexer = scan::Lexer::new(&logical);
        lexer.run();
        (lexer.tokens, lexer.comments, lexer.problems)
    };
    let partner = pair(&logical, &map, &tokens, &mut problems, limits)?;
    problems.sort_by_key(|problem| problem.0);
    let diagnostics = problems
        .into_iter()
        .map(|(logical_byte, reason)| Diagnostic {
            line: map.line(logical_byte).clamp(1, map.physical_lines),
            byte: map.physical(logical_byte).min(source.len() as u32),
            reason,
        })
        .collect();
    Ok(Lexed {
        logical,
        tokens,
        comments,
        partner,
        diagnostics,
        map,
    })
}

/// Problems found while lexing, as `(logical byte, reason)`.
pub(crate) type Problems = Vec<(u32, DiagnosticReason)>;

fn prepare(source: &str) -> (String, SourceMap) {
    let bytes = source.as_bytes();
    let mut logical = String::with_capacity(source.len());
    let mut splices = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        let removed = splice_len(&bytes[index..]);
        if removed > 0 {
            splices.push(Splice {
                at: logical.len() as u32,
                removed: removed as u32,
                newlines: 1,
            });
            index += removed;
            continue;
        }
        let Some(ch) = source[index..].chars().next() else {
            break;
        };
        logical.push(ch);
        index += ch.len_utf8();
    }
    let mut logical_line_starts = vec![0];
    for (offset, byte) in logical.bytes().enumerate() {
        if byte == b'\n' {
            logical_line_starts.push(offset as u32 + 1);
        }
    }
    let physical_lines = source.bytes().filter(|byte| *byte == b'\n').count() as u32 + 1;
    (
        logical,
        SourceMap {
            splices,
            logical_line_starts,
            physical_lines,
        },
    )
}

fn splice_len(bytes: &[u8]) -> usize {
    if bytes.first() != Some(&b'\\') {
        return 0;
    }
    if bytes.get(1) == Some(&b'\n') {
        return 2;
    }
    if bytes.get(1) == Some(&b'\r') && bytes.get(2) == Some(&b'\n') {
        return 3;
    }
    if bytes.get(1) == Some(&b'\r') {
        2
    } else {
        0
    }
}

fn pair(
    source: &str,
    map: &SourceMap,
    tokens: &[Token],
    problems: &mut Problems,
    limits: &Limits,
) -> Result<Vec<u32>, LexError> {
    let mut partner = vec![NONE; tokens.len()];
    let mut stack = Vec::new();
    let mut angle = 0i32;
    for (index, token) in tokens.iter().enumerate() {
        if token.kind != Kind::Punct {
            continue;
        }
        let text = slice(source, token.start, token.end);
        match text {
            "(" | "[" | "{" => push_open(map, tokens, index, &mut stack, limits)?,
            ")" | "]" | "}" => {
                close_bracket(source, tokens, index, text, &mut stack, &mut partner, problems);
            }
            "<" => {
                angle += 1;
                if angle as u32 > limits.max_depth {
                    return Err(LexError::TooDeep {
                        line: map.line(token.start).clamp(1, map.physical_lines),
                        limit: limits.max_depth,
                    });
                }
            }
            ">" => angle = angle.saturating_sub(1),
            ">>" => angle = angle.saturating_sub(2),
            _ => {}
        }
    }
    for open in stack {
        problems.push((tokens[open].start, DiagnosticReason::UnbalancedBracket));
    }
    Ok(partner)
}

fn push_open(
    map: &SourceMap,
    tokens: &[Token],
    index: usize,
    stack: &mut Vec<usize>,
    limits: &Limits,
) -> Result<(), LexError> {
    stack.push(index);
    if stack.len() as u32 > limits.max_depth {
        return Err(LexError::TooDeep {
            line: map.line(tokens[index].start).clamp(1, map.physical_lines),
            limit: limits.max_depth,
        });
    }
    Ok(())
}

fn close_bracket(
    source: &str,
    tokens: &[Token],
    index: usize,
    text: &str,
    stack: &mut Vec<usize>,
    partner: &mut [u32],
    problems: &mut Problems,
) {
    let wanted = match text {
        ")" => "(",
        "]" => "[",
        _ => "{",
    };
    let found = stack.iter().rposition(|&open| {
        let token = &tokens[open];
        slice(source, token.start, token.end) == wanted
    });
    let Some(position) = found else {
        problems.push((tokens[index].start, DiagnosticReason::UnbalancedBracket));
        return;
    };
    while stack.len() > position + 1 {
        if let Some(orphan) = stack.pop() {
            problems.push((tokens[orphan].start, DiagnosticReason::UnbalancedBracket));
        }
    }
    if let Some(open) = stack.pop() {
        partner[open] = index as u32;
        partner[index] = open as u32;
    }
}

pub(crate) fn slice(source: &str, start: u32, end: u32) -> &str {
    source.get(start as usize..end as usize).unwrap_or("")
}

/// Whether `word` is a C or C++ keyword. Macros are ordinary identifiers.
pub(crate) fn keyword(word: &str) -> bool {
    matches!(
        word,
        "alignas"
            | "alignof"
            | "asm"
            | "auto"
            | "bool"
            | "break"
            | "case"
            | "catch"
            | "char"
            | "char8_t"
            | "char16_t"
            | "char32_t"
            | "class"
            | "const"
            | "consteval"
            | "constexpr"
            | "constinit"
            | "const_cast"
            | "continue"
            | "co_await"
            | "co_return"
            | "co_yield"
            | "decltype"
            | "default"
            | "delete"
            | "do"
            | "double"
            | "dynamic_cast"
            | "else"
            | "enum"
            | "explicit"
            | "export"
            | "extern"
            | "false"
            | "float"
            | "for"
            | "friend"
            | "goto"
            | "if"
            | "inline"
            | "int"
            | "long"
            | "mutable"
            | "namespace"
            | "new"
            | "noexcept"
            | "nullptr"
            | "operator"
            | "private"
            | "protected"
            | "public"
            | "register"
            | "reinterpret_cast"
            | "requires"
            | "return"
            | "restrict"
            | "short"
            | "signed"
            | "sizeof"
            | "static"
            | "static_assert"
            | "static_cast"
            | "struct"
            | "switch"
            | "template"
            | "this"
            | "thread_local"
            | "throw"
            | "true"
            | "try"
            | "typedef"
            | "typeid"
            | "typename"
            | "union"
            | "unsigned"
            | "using"
            | "virtual"
            | "void"
            | "volatile"
            | "wchar_t"
            | "while"
            | "_Alignas"
            | "_Alignof"
            | "_Atomic"
            | "_Bool"
            | "_Complex"
            | "_Generic"
            | "_Imaginary"
            | "_Noreturn"
            | "_Pragma"
            | "_Static_assert"
            | "_Thread_local"
    )
}

/// A declaration specifier that may sit in front of a type.
pub(crate) fn specifier(word: &str) -> bool {
    matches!(
        word,
        "static"
            | "extern"
            | "inline"
            | "constexpr"
            | "consteval"
            | "constinit"
            | "virtual"
            | "explicit"
            | "friend"
            | "mutable"
            | "register"
            | "thread_local"
            | "_Thread_local"
            | "restrict"
            | "_Noreturn"
            | "__restrict"
            | "__inline"
            | "__inline__"
            | "__forceinline"
            | "__cdecl"
            | "__stdcall"
            | "__fastcall"
            | "export"
            | "volatile"
            | "const"
    )
}

/// Built-in type names recorded as type references.
pub(crate) fn type_word(word: &str) -> bool {
    matches!(
        word,
        "void" | "bool" | "_Bool" | "char" | "wchar_t" | "char8_t" | "char16_t" | "char32_t"
            | "short" | "int" | "long" | "signed" | "unsigned" | "float" | "double" | "auto"
    )
}

/// Control words and operators that are not call callees.
pub(crate) fn non_callee(word: &str) -> bool {
    matches!(
        word,
        "if" | "for" | "while" | "switch" | "catch" | "return" | "sizeof" | "alignof"
            | "_Alignof" | "decltype" | "static_assert" | "_Static_assert" | "typeof"
            | "typeof_unqual" | "new" | "delete" | "case" | "else" | "static_cast"
            | "dynamic_cast" | "reinterpret_cast" | "const_cast" | "co_await" | "co_return"
            | "co_yield"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splices_strings_and_directives_do_not_leak_tokens() {
        let source = "int spl\\\nice(int v);\n#define NAME \\\nint\n\"int hid\\\nden()\"\n";
        let lexed = lex(source, &Limits::default()).unwrap();
        assert!(lexed.logical.contains("splice"));
        assert!(!lexed.logical.contains('\\'));
        let words: Vec<&str> = lexed
            .tokens
            .iter()
            .map(|token| slice(&lexed.logical, token.start, token.end))
            .collect();
        assert!(words.contains(&"splice"), "{words:?}");
        assert!(!words.contains(&"hidden"), "{words:?}");
        assert!(words.iter().any(|word| word.starts_with("#define")));
        assert!(lexed.diagnostics.is_empty(), "{:?}", lexed.diagnostics);
        let raw = lex("const char* s = R\"c(int Hidden() { fake(); })c\";\n", &Limits::default()).unwrap();
        let raw_words: Vec<&str> = raw
            .tokens
            .iter()
            .map(|token| slice(&raw.logical, token.start, token.end))
            .collect();
        assert!(!raw_words.contains(&"Hidden"), "{raw_words:?}");
        assert!(!raw_words.contains(&"fake"), "{raw_words:?}");
    }
}
