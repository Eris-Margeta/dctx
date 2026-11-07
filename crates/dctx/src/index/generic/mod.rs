// SPDX-License-Identifier: Apache-2.0
//! Heuristic symbol extraction for languages that have no first-party lexer.
//! Every symbol is a guess: the records of these files carry confidence `guess`, edges stay `contains`.
//! Shared helpers live here; a family (brace languages today, lisp, infra and web later) is a sibling file with a
//! `handles` and a `symbols` function and one dispatch line in [`handles`] and [`symbols`] below.
//! Consumes authorized UTF-8 from extract; never reads source, never panics on malformed input.

mod block;
mod brace;
mod calls_block;
mod calls_brace;
mod calls_infra;
mod calls_lisp;
mod calls_verilog;
mod infra;
mod lisp;
#[cfg(test)]
mod tests;
pub(super) mod web;

use dctx_map_model::NodeKind;

use super::extract::Symbol;
use super::span_index::SpanIndex;

/// Confidence label of every record that comes from a file extracted here.
pub(super) const GUESS: &str = "guess";

/// Words that are control flow, never a declaration name.
const DENYLIST: &[&str] = &[
    "if", "else", "for", "while", "match", "switch", "return", "case", "catch", "when", "do",
    "try", "new", "await", "throw", "finally", "elif", "foreach", "loop", "yield", "assert",
    "with",
];

/// Longest signature kept, in bytes, cut at a character boundary.
const SIGNATURE_MAX: usize = 160;

/// True when `language` (a tag of `extract::language_of`) is extracted by a generic family.
pub(super) fn handles(language: &str) -> bool {
    brace::handles(language)
        || block::handles(language)
        || infra::handles(language)
        || lisp::handles(language)
}

/// Symbols of `text` for a language that [`handles`]. Anything else, and any malformed input, yields none.
pub(super) fn symbols(language: &str, text: &str) -> Vec<Symbol> {
    if brace::handles(language) {
        return brace::symbols(language, text);
    }
    if block::handles(language) {
        return block::symbols(language, text);
    }
    if infra::handles(language) {
        return infra::symbols(language, text);
    }
    if lisp::handles(language) {
        return lisp::symbols(language, text);
    }
    Vec::new()
}

/// Raw call references of `text` for a language that [`handles`]. Families without call support yield none.
pub(super) fn references(
    language: &str,
    path: &str,
    text: &str,
    symbols: &[Symbol],
) -> Vec<super::refs::RawRef> {
    if brace::handles(language) {
        return calls_brace::references(language, path, text, symbols);
    }
    if block::handles(language) {
        return calls_block::references(language, path, text, symbols);
    }
    if infra::handles(language) {
        return calls_infra::references(language, path, text, symbols);
    }
    if lisp::handles(language) {
        return calls_lisp::references(language, path, text, symbols);
    }
    Vec::new()
}

/// How one family writes comments and strings, so they can be blanked before scanning.
#[derive(Debug, Clone, Copy)]
pub(super) struct Syntax {
    /// Prefixes that start a comment running to the end of the line.
    pub line: &'static [&'static str],
    /// Block comment delimiters, if the language has them.
    pub block: Option<(&'static str, &'static str)>,
    /// True when block comments nest.
    pub nested: bool,
    /// Quote bytes that delimit strings. A doubled-up triple quote may span lines when `triple` is set.
    pub quotes: &'static [u8],
    /// True when `"""` and `'''` open multi-line strings.
    pub triple: bool,
}

/// One declaration before it becomes a [`Symbol`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Decl {
    /// Name, qualified as `Container::name` inside a type.
    pub name: String,
    /// Wire kind: `type` or `fn`.
    pub kind: &'static str,
    /// Inclusive 1-based lines.
    pub lines: [u32; 2],
    /// The declaration line without its opening brace.
    pub signature: String,
}

/// Copy of `text` with comments and string literals replaced by spaces. Newlines and every byte offset stay put.
pub(super) fn mask(text: &str, syntax: &Syntax) -> String {
    let bytes = text.as_bytes();
    let mut out = bytes.to_vec();
    let mut at = 0usize;
    while at < bytes.len() {
        let rest = &bytes[at..];
        let end = if syntax
            .line
            .iter()
            .any(|prefix| rest.starts_with(prefix.as_bytes()))
        {
            Some(line_end(bytes, at))
        } else if let Some((open, close)) = syntax
            .block
            .filter(|(open, _)| rest.starts_with(open.as_bytes()))
        {
            Some(block_end(bytes, at, open, close, syntax.nested))
        } else if syntax.quotes.contains(&bytes[at]) {
            string_end(bytes, at, syntax.triple)
        } else {
            None
        };
        match end {
            Some(end) => {
                blank(&mut out, at, end);
                at = end.max(at + 1);
            }
            None => at += 1,
        }
    }
    String::from_utf8(out).unwrap_or_default()
}

/// Replaces `from..to` with spaces, keeping line breaks.
fn blank(out: &mut [u8], from: usize, to: usize) {
    for byte in out.iter_mut().take(to).skip(from) {
        if *byte != b'\n' && *byte != b'\r' {
            *byte = b' ';
        }
    }
}

/// Offset of the line break after `from`, or the end of input.
fn line_end(bytes: &[u8], from: usize) -> usize {
    bytes[from..]
        .iter()
        .position(|byte| *byte == b'\n')
        .map_or(bytes.len(), |offset| from + offset)
}

/// End of the block comment that opens at `from`. An unclosed comment runs to the end of input.
fn block_end(bytes: &[u8], from: usize, open: &str, close: &str, nested: bool) -> usize {
    let mut depth = 1usize;
    let mut at = from + open.len();
    while at < bytes.len() {
        let rest = &bytes[at..];
        if rest.starts_with(close.as_bytes()) {
            depth -= 1;
            at += close.len();
            if depth == 0 {
                return at;
            }
        } else if nested && rest.starts_with(open.as_bytes()) {
            depth += 1;
            at += open.len();
        } else {
            at += 1;
        }
    }
    bytes.len()
}

/// End of the string that opens at `from`, or `None` when the quote is only an apostrophe.
fn string_end(bytes: &[u8], from: usize, triple: bool) -> Option<usize> {
    let quote = bytes[from];
    if triple && bytes[from..].starts_with(&[quote, quote, quote]) {
        let body = &bytes[from + 3..];
        return Some(
            body.windows(3)
                .position(|window| window == [quote; 3])
                .map_or(bytes.len(), |offset| from + 3 + offset + 3),
        );
    }
    let mut at = from + 1;
    while at < bytes.len() {
        match bytes[at] {
            b'\\' => at += 2,
            byte if byte == quote => return Some(at + 1),
            b'\n' => break,
            _ => at += 1,
        }
    }
    // An unclosed double quote swallows its line; an unclosed single quote was an apostrophe.
    (quote == b'"').then(|| at.min(bytes.len()))
}

/// The identifier that starts `text`, or `None`.
pub(super) fn ident_at(text: &str) -> Option<&str> {
    let end = text
        .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$'))
        .unwrap_or(text.len());
    let word = &text[..end];
    (!word.is_empty() && !word.starts_with(|c: char| c.is_ascii_digit())).then_some(word)
}

/// True for a word that is never a symbol name.
pub(super) fn is_denied(word: &str) -> bool {
    DENYLIST.contains(&word)
}

/// `text` without leading modifier words and annotations. A `[scope]` after a modifier is dropped with it.
pub(super) fn skip_modifiers<'a>(mut text: &'a str, modifiers: &[&str]) -> &'a str {
    loop {
        text = text.trim_start();
        if let Some(annotation) = text.strip_prefix('@') {
            let name = annotation
                .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '.'))
                .unwrap_or(annotation.len());
            text = skip_group(&annotation[name..], '(', ')');
            continue;
        }
        let Some(word) = ident_at(text).filter(|word| modifiers.contains(word)) else {
            return text;
        };
        text = skip_group(&text[word.len()..], '[', ']');
    }
}

/// `text` after a leading balanced `open ... close` group on one line, or unchanged when there is none.
fn skip_group(text: &str, open: char, close: char) -> &str {
    let trimmed = text.trim_start();
    if !trimmed.starts_with(open) {
        return text;
    }
    let mut depth = 0usize;
    for (offset, c) in trimmed.char_indices() {
        if c == open {
            depth += 1;
        } else if c == close {
            depth -= 1;
            if depth == 0 {
                return &trimmed[offset + c.len_utf8()..];
            }
        } else if c == '\n' {
            break;
        }
    }
    text
}

/// The declaration line trimmed, without a trailing `{`, cut to [`SIGNATURE_MAX`] bytes.
pub(super) fn signature_of(line: &str) -> String {
    let mut text = line.trim().trim_end_matches('{').trim_end();
    if text.len() > SIGNATURE_MAX {
        let mut cut = SIGNATURE_MAX;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        text = &text[..cut];
    }
    text.to_string()
}

/// Turns declarations into symbols: kind as given, summary source `sig`, summary equal to the signature.
pub(super) fn to_symbols(text: &str, decls: Vec<Decl>) -> Vec<Symbol> {
    let spans = SpanIndex::build(text);
    decls
        .into_iter()
        .map(|decl| Symbol {
            name: decl.name,
            kind: decl.kind.to_string(),
            graph: NodeKind::Symbol,
            lines: decl.lines,
            span: spans.line_span(decl.lines),
            summary: decl.signature.clone(),
            summary_source: "sig".to_string(),
            signature: decl.signature,
        })
        .collect()
}
