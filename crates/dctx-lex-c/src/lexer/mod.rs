//! Token stream for C and C++ after line-splice removal.
//! Implements PARSERS-v1 lexing: comments, strings, raw strings, characters,
//! preprocessor lines and bracket nesting bounded by `Limits`.
//! The structural walker consumes [`Lexed`] and does not expand macros.
//! Template `<` `>` count toward depth; comparisons and shifts do not.
mod angles;
mod preprocessor;
mod scan;

use crate::domain::{Diagnostic, DiagnosticReason, LexError, Limits};
use std::collections::BTreeMap;

pub(crate) use angles::{opens_template_args, AngleDepth};
pub(crate) use preprocessor::{classify, define_attribute, define_brace, hidden_include, Dir};

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
    /// One preprocessor directive, from `#` through its terminating newline.
    /// Strings, raw strings and block comments on that directive keep their newlines.
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
    #[allow(clippy::cast_possible_truncation, reason = "Source offsets, line counts and token counts fit u32 because the public extractor enforces the 8 MiB input ceiling.")]
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
#[allow(clippy::cast_possible_truncation, reason = "Source offsets, line counts and token counts fit u32 because the public extractor enforces the 8 MiB input ceiling.")]
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

#[allow(clippy::cast_possible_truncation, reason = "Source offsets, line counts and token counts fit u32 because the public extractor enforces the 8 MiB input ceiling.")]
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

/// One unmatched opener. `kind` is the bracket spelling, kept when a macro
/// stands in for `{` so a later `#undef` cannot hide the match.
#[derive(Clone, Copy)]
struct Open {
    index: usize,
    kind: &'static str,
}

fn pair(
    source: &str,
    map: &SourceMap,
    tokens: &[Token],
    problems: &mut Problems,
    limits: &Limits,
) -> Result<Vec<u32>, LexError> {
    let mut partner = vec![NONE; tokens.len()];
    let mut stack: Vec<Open> = Vec::new();
    // Only a `<` that a template closer matches counts. Comparisons and `>>`
    // shifts stay at depth zero so a long function is not `TooDeep`.
    let mut angles = AngleDepth::new();
    let mut braces: BTreeMap<String, bool> = BTreeMap::new();
    // `#if` / `#else` branches share the outer brace stack. Each branch restores
    // the snapshot so duplicated braces are alternatives, not extras.
    let mut conds: Vec<(Vec<Open>, AngleDepth)> = Vec::new();
    for index in 0..tokens.len() {
        let token = tokens[index];
        if token.kind == Kind::Directive {
            let text = slice(source, token.start, token.end);
            match classify(text) {
                Dir::Define(Some(def)) => {
                    if let Some(open) = define_brace(text) {
                        braces.insert(def.name, open);
                    } else {
                        braces.remove(&def.name);
                    }
                }
                Dir::Undef(name) => {
                    braces.remove(&name);
                }
                Dir::Open => conds.push((stack.clone(), angles.clone())),
                Dir::Branch => {
                    if let Some((saved, saved_angles)) = conds.last() {
                        stack.clear();
                        stack.extend(saved.iter().copied());
                        angles = saved_angles.clone();
                    }
                }
                Dir::Close => {
                    conds.pop();
                }
                _ => {}
            }
            continue;
        }
        let spelling = token_spelling(source, &token, &braces);
        let opens = spelling == "<"
            && opens_template_args(
                |at| match tokens.get(at) {
                    Some(next) => token_spelling(source, next, &braces),
                    None => "",
                },
                tokens.len(),
                index,
            );
        match spelling {
            "(" | "[" | "{" => {
                if spelling == "{" {
                    angles.boundary();
                }
                let kind = match spelling {
                    "(" => "(",
                    "[" => "[",
                    _ => "{",
                };
                push_open(map, tokens, index, kind, &mut stack, limits)?;
                angles.observe(spelling, false);
            }
            ")" | "]" | "}" => {
                let wanted = match spelling {
                    ")" => "(",
                    "]" => "[",
                    _ => "{",
                };
                close_bracket(tokens, index, wanted, &mut stack, &mut partner, problems);
                angles.observe(spelling, false);
            }
            "<" => {
                let before = angles.depth();
                angles.observe(spelling, opens);
                if angles.depth() > before && angles.depth() > limits.max_depth {
                    return Err(LexError::TooDeep {
                        line: map.line(token.start).clamp(1, map.physical_lines),
                        limit: limits.max_depth,
                    });
                }
            }
            ">" | ">>" => angles.observe(spelling, false),
            ";" => angles.boundary(),
            _ => {}
        }
    }
    for open in stack {
        // An object-like `{` macro may be closed by a macro whose body is not a
        // token yet. A real bracket still reports.
        if tokens[open.index].kind != Kind::Ident {
            problems.push((tokens[open.index].start, DiagnosticReason::UnbalancedBracket));
        }
    }
    Ok(partner)
}

fn token_spelling<'a>(source: &'a str, token: &Token, braces: &BTreeMap<String, bool>) -> &'a str {
    let text = slice(source, token.start, token.end);
    if token.kind == Kind::Ident {
        return match braces.get(text) {
            Some(true) => "{",
            Some(false) => "}",
            None => text,
        };
    }
    text
}

#[allow(clippy::cast_possible_truncation, reason = "Source offsets, line counts and token counts fit u32 because the public extractor enforces the 8 MiB input ceiling.")]
fn push_open(
    map: &SourceMap,
    tokens: &[Token],
    index: usize,
    kind: &'static str,
    stack: &mut Vec<Open>,
    limits: &Limits,
) -> Result<(), LexError> {
    stack.push(Open { index, kind });
    if stack.len() as u32 > limits.max_depth {
        return Err(LexError::TooDeep {
            line: map.line(tokens[index].start).clamp(1, map.physical_lines),
            limit: limits.max_depth,
        });
    }
    Ok(())
}

#[allow(clippy::cast_possible_truncation, reason = "Source offsets, line counts and token counts fit u32 because the public extractor enforces the 8 MiB input ceiling.")]
fn close_bracket(
    tokens: &[Token],
    index: usize,
    wanted: &str,
    stack: &mut Vec<Open>,
    partner: &mut [u32],
    problems: &mut Problems,
) {
    let found = stack.iter().rposition(|open| open.kind == wanted);
    let Some(position) = found else {
        // `END` whose body is `}` is not a syntax error when the `{` lived in
        // another macro. A real closer with nothing to close still reports.
        if tokens[index].kind != Kind::Ident {
            problems.push((tokens[index].start, DiagnosticReason::UnbalancedBracket));
        }
        return;
    };
    while stack.len() > position + 1 {
        if let Some(orphan) = stack.pop() {
            problems.push((tokens[orphan.index].start, DiagnosticReason::UnbalancedBracket));
        }
    }
    if let Some(open) = stack.pop() {
        partner[open.index] = index as u32;
        partner[index] = open.index as u32;
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
mod tests;
