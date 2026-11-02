//! Line-oriented Rust and Markdown symbols, plus the signature text shared with the syn walk.
use super::*;

pub(super) fn rust_symbols(text: &str) -> Vec<Symbol> {
    let mut symbols = Vec::new();
    let mut pending: Option<String> = None;
    let mut offset = 0usize;
    for (line_no, line) in (1u32..).zip(text.split_inclusive('\n')) {
        let body_len = line.trim_end_matches(['\r', '\n']).len();
        let body = &line[..body_len];
        let trimmed = body.trim();
        if trimmed.is_empty() {
            pending = None;
        } else if let Some(stripped) = trimmed.strip_prefix("///") {
            let rest = stripped.strip_prefix(' ').unwrap_or(stripped);
            if pending.is_none() && !rest.is_empty() {
                pending = Some(rest.to_string());
            }
        } else if trimmed.starts_with("#[") || trimmed.starts_with("//!") {
            // An attribute keeps a pending doc. A module doc is not a symbol.
        } else if let Some((kind, name)) = rust_decl(trimmed) {
            let mut symbol = placed(name, kind, NodeKind::Symbol, line_no, offset, body_len);
            symbol.signature = signature_text(trimmed);
            symbol.summary_source = if pending.is_some() { "doc".to_string() } else { "sig".to_string() };
            symbol.summary = pending.take().unwrap_or_else(|| symbol.signature.clone());
            symbols.push(symbol);
        } else {
            pending = None;
        }
        offset += line.len();
    }
    symbols
}

fn rust_decl(line: &str) -> Option<(&'static str, String)> {
    for (keyword, kind) in [("fn", "fn"), ("struct", "struct"), ("enum", "enum"), ("trait", "trait"), ("mod", "mod")] {
        if let Some(name) = keyword_name(line, keyword) {
            return Some((kind, name));
        }
    }
    None
}

pub(super) fn markdown_symbols(bytes: &[u8]) -> Vec<Symbol> {
    let Ok(document) = dctx_lex_md::extract(bytes) else { return Vec::new() };
    let mut symbols = Vec::new();
    for heading in document.headings {
        if heading.title.is_empty() {
            continue;
        }
        let summary = if heading.summary.is_empty() { heading.title.clone() } else { heading.summary };
        symbols.push(Symbol {
            name: heading.title,
            kind: "heading".to_string(),
            graph: NodeKind::Doc,
            lines: heading.lines,
            span: [
                u64::try_from(heading.span.start).unwrap_or(u64::MAX),
                u64::try_from(heading.span.end).unwrap_or(u64::MAX),
            ],
            summary,
            summary_source: "heading".to_string(),
            signature: String::new(),
        });
    }
    symbols
}

fn keyword_name(line: &str, keyword: &str) -> Option<String> {
    if keyword.is_empty() {
        return ident_of(line);
    }
    let bytes = line.as_bytes();
    let key = keyword.as_bytes();
    let mut index = 0usize;
    while index + key.len() <= bytes.len() {
        let before_ok = index == 0 || !is_ident(bytes[index - 1]);
        let after = index + key.len();
        let after_ok = after == bytes.len() || !is_ident(bytes[after]);
        if before_ok && after_ok && bytes[index..].starts_with(key)
            && let Some(name) = ident_of(line[after..].trim_start())
        {
            return Some(name);
        }
        index += 1;
    }
    None
}

fn ident_of(text: &str) -> Option<String> {
    let mut name = String::new();
    for (index, ch) in text.chars().enumerate() {
        let ok = ch == '_' || ch.is_ascii_alphanumeric() || ch == '-';
        let start_ok = index != 0 || ch == '_' || ch.is_ascii_alphabetic() || ch == '-';
        if index == 0 && !start_ok {
            return None;
        }
        if !ok {
            break;
        }
        name.push(ch);
    }
    if name.is_empty() { None } else { Some(name) }
}

fn is_ident(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphanumeric() || byte == b'-'
}

fn placed(name: String, kind: &str, graph: NodeKind, line_no: u32, offset: usize, body_len: usize) -> Symbol {
    Symbol {
        name,
        kind: kind.to_string(),
        graph,
        lines: [line_no, line_no],
        span: [u64::try_from(offset).unwrap_or(u64::MAX), u64::try_from(offset + body_len).unwrap_or(u64::MAX)],
        summary: String::new(),
        summary_source: String::new(),
        signature: String::new(),
    }
}

/// Longest signature kept, in characters. A signature is a label for a record page, never the body.
pub(super) const SIGNATURE_CAP: usize = 160;

/// Signature ending at the matching parenthesis, or the text before a body brace. The result is one line of at most
/// [`SIGNATURE_CAP`] characters cut on a char boundary; a call-style declaration `test('name', () => ...)` stops after its first argument.
pub(crate) fn signature_text(text: &str) -> String {
    let trimmed = text.trim();
    let whole = if let Some(head) = call_head(trimmed) {
        head
    } else if let Some(start) = trimmed.find('(')
        && let Some(end) = matching_paren(trimmed, start)
    {
        trimmed[..=end].trim().to_string()
    } else {
        let cut = trimmed.find('{').unwrap_or(trimmed.len());
        trimmed[..cut].trim().trim_end_matches(';').trim().to_string()
    };
    let line = whole.lines().next().unwrap_or_default().trim_end();
    match line.char_indices().nth(SIGNATURE_CAP) {
        Some((cut, _)) => line[..cut].trim_end().to_string(),
        None => line.to_string(),
    }
}

/// `callee('literal', ...)` for a call whose first argument is a string literal followed by more arguments, else `None`.
fn call_head(text: &str) -> Option<String> {
    let open = text.find('(')?;
    let callee = &text[..open];
    if callee.is_empty() || !callee.bytes().all(|byte| is_ident(byte) || byte == b'.') {
        return None;
    }
    let rest = &text[open + 1..];
    let quote = rest.chars().next().filter(|ch| matches!(ch, '\'' | '"' | '`'))?;
    let mut escaped = false;
    let mut close = None;
    for (index, ch) in rest.char_indices().skip(1) {
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == quote {
            close = Some(index);
            break;
        }
    }
    let literal_end = close? + quote.len_utf8();
    rest[literal_end..].trim_start().starts_with(',').then(|| format!("{callee}({}, ...)", &rest[..literal_end]))
}

fn matching_paren(text: &str, open: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut depth = 0i32;
    for (index, byte) in bytes.iter().enumerate().skip(open) {
        match byte {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}
