// SPDX-License-Identifier: Apache-2.0
//! Web files: Vue, Svelte, Astro and HTML (`.vue`, `.svelte`, `.astro`, `.html`, `.htm`, language tag `web`).
//! Script blocks (and the Astro `---` front matter) go through the first-party JS/TS lexer. They are lexed as one
//! program laid over a copy of the file in which everything else is blanked, so lines, byte spans, ids and call
//! references already are the original file's. `<h1>`..`<h6>` become doc sections like Markdown headings.
//! Consumes authorized UTF-8 from lex_dispatch; never reads source, never panics on unclosed tags.

use dctx_map_model::NodeKind;

use crate::index::extract::Symbol;
use crate::index::lex_dispatch;
use crate::index::refs::RawRef;
use crate::index::span_index::SpanIndex;

/// Longest heading title kept, in characters.
const TITLE_MAX: usize = 200;

/// Farthest a heading's closing tag may be from its opening tag, in bytes.
const HEADING_WINDOW: usize = 2000;

/// Longest opening tag searched for its `>`, in bytes.
const TAG_WINDOW: usize = 8192;

/// Path the script program is lexed under; only its extension matters, the ids it makes are retargeted.
const SYNTHETIC_JS: &str = "web-script.js";

/// Same for TypeScript programs.
const SYNTHETIC_TS: &str = "web-script.ts";

/// A byte range of JavaScript or TypeScript inside the file.
struct Block {
    /// First byte of the code.
    start: usize,
    /// One past the last byte of the code.
    end: usize,
    /// True when the block says it is TypeScript.
    typed: bool,
}

/// One `<hN>` element.
struct Head {
    /// 1 to 6.
    level: u8,
    /// Text without tags, entities decoded, whitespace folded.
    title: String,
    /// Byte offset of the `<`.
    at: usize,
}

/// Script symbols and heading sections of a web file, with the script's call and import references.
pub(in crate::index) fn lexed(path: &str, text: &str) -> (Vec<Symbol>, Vec<RawRef>) {
    let (mut blocks, template) = match front_matter(text) {
        Some((start, end, after)) => (vec![Block { start, end, typed: true }], after),
        None => (Vec::new(), 0),
    };
    let (found, heads) = scan(text, template);
    blocks.extend(found);
    let (mut symbols, refs) = scripts(path, text, &blocks);
    symbols.extend(sections(text, &heads));
    symbols.sort_by_key(|symbol| symbol.lines[0]);
    (symbols, refs)
}

/// Symbols and references of all script blocks lexed as one program, ids retargeted to `path`.
fn scripts(path: &str, text: &str, blocks: &[Block]) -> (Vec<Symbol>, Vec<RawRef>) {
    if blocks.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let mut masked: Vec<u8> = text.bytes().map(|byte| if matches!(byte, b'\n' | b'\r') { byte } else { b' ' }).collect();
    for block in blocks {
        masked[block.start..block.end].copy_from_slice(&text.as_bytes()[block.start..block.end]);
    }
    let Ok(masked) = String::from_utf8(masked) else { return (Vec::new(), Vec::new()) };
    let (language, synthetic) = if blocks.iter().any(|block| block.typed) { ("typescript", SYNTHETIC_TS) } else { ("javascript", SYNTHETIC_JS) };
    let (symbols, mut refs) = lex_dispatch::lexed(language, synthetic, &masked);
    for reference in &mut refs {
        if let Some(tail) = reference.src.get(2..).and_then(|rest| rest.strip_prefix(synthetic)) {
            reference.src = format!("{}{path}{tail}", &reference.src[..2]);
        }
    }
    (symbols, refs)
}

/// Astro front matter: a first line `---` and the next line `---`. Returns the code range and where the template starts.
fn front_matter(text: &str) -> Option<(usize, usize, usize)> {
    let mut lines = text.split_inclusive('\n');
    let first = lines.next()?;
    if first.trim_end() != "---" {
        return None;
    }
    let mut at = first.len();
    for line in lines {
        if line.trim_end() == "---" {
            return Some((first.len(), at, at + line.len()));
        }
        at += line.len();
    }
    None
}

/// Script blocks and headings of the template part of `text`, starting at byte `from`.
fn scan(text: &str, from: usize) -> (Vec<Block>, Vec<Head>) {
    let bytes = text.as_bytes();
    let (mut blocks, mut heads) = (Vec::new(), Vec::new());
    let mut at = from;
    while let Some(offset) = bytes.get(at..).and_then(|rest| rest.iter().position(|byte| *byte == b'<')) {
        let lt = at + offset;
        let rest = &bytes[lt..];
        at = lt + 1;
        if rest.starts_with(b"<!--") {
            at = find_ci(bytes, lt + 4, b"-->").map_or(bytes.len(), |end| end + 3);
        } else if tag_is(rest, b"style") {
            at = find_ci(bytes, lt, b"</style").map_or(bytes.len(), |end| end + 7);
        } else if tag_is(rest, b"script") {
            let Some(gt) = tag_end(bytes, lt) else { continue };
            let attrs = text.get(lt..gt).unwrap_or_default().to_ascii_lowercase();
            let end = if attrs.ends_with('/') { gt + 1 } else { find_ci(bytes, gt + 1, b"</script").unwrap_or(bytes.len()) };
            if !attrs.ends_with('/') && script_type_ok(&attrs) {
                let lang = attr(&attrs, "lang").unwrap_or_default();
                let typed = lang == "ts" || lang == "typescript" || attrs.contains("typescript");
                blocks.push(Block { start: gt + 1, end, typed });
            }
            at = end.max(gt + 1);
        } else if let Some(level) = heading_level(rest)
            && let Some(gt) = tag_end(bytes, lt)
        {
            let window = bytes.len().min(gt + 1 + HEADING_WINDOW);
            let closing = [b'<', b'/', b'h', b'0' + level];
            if let Some(close) = find_ci(&bytes[..window], gt + 1, &closing) {
                let title = clean(text.get(gt + 1..close).unwrap_or_default());
                if !title.is_empty() {
                    heads.push(Head { level, title, at: lt });
                }
                at = close;
            } else {
                at = gt + 1;
            }
        }
    }
    (blocks, heads)
}

/// True when `rest` (starting at `<`) opens the element `name`.
fn tag_is(rest: &[u8], name: &[u8]) -> bool {
    let Some(word) = rest.get(1..1 + name.len()) else { return false };
    word.eq_ignore_ascii_case(name) && rest.get(1 + name.len()).is_some_and(|byte| byte.is_ascii_whitespace() || matches!(byte, b'>' | b'/'))
}

/// Level of an `<h1>`..`<h6>` open tag at the start of `rest`.
fn heading_level(rest: &[u8]) -> Option<u8> {
    let (h, digit, after) = (*rest.get(1)?, *rest.get(2)?, *rest.get(3)?);
    (h.eq_ignore_ascii_case(&b'h') && (b'1'..=b'6').contains(&digit) && (after.is_ascii_whitespace() || matches!(after, b'>' | b'/')))
        .then_some(digit - b'0')
}

/// Offset of the `>` that closes the open tag at `lt`, skipping quoted attribute values.
fn tag_end(bytes: &[u8], lt: usize) -> Option<usize> {
    let mut quote = 0u8;
    for (index, byte) in bytes.iter().enumerate().skip(lt).take(TAG_WINDOW) {
        match (quote, *byte) {
            (0, b'"' | b'\'') => quote = *byte,
            (0, b'>') => return Some(index),
            (open, current) if open == current => quote = 0,
            _ => {}
        }
    }
    None
}

/// First offset at or after `from` where `needle` (lowercase) occurs, ignoring ASCII case.
fn find_ci(bytes: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    bytes.get(from..)?.windows(needle.len()).position(|window| window.eq_ignore_ascii_case(needle)).map(|offset| from + offset)
}

/// Value of attribute `name` in the lowercase open tag `attrs`, quotes removed.
fn attr<'a>(attrs: &'a str, name: &str) -> Option<&'a str> {
    let needle = format!("{name}=");
    let mut from = 0usize;
    while let Some(offset) = attrs.get(from..)?.find(&needle) {
        let at = from + offset;
        from = at + needle.len();
        if attrs[..at].ends_with(char::is_whitespace) {
            let value = attrs[from..].trim_start_matches(['"', '\'']);
            return Some(value.split(['"', '\'', ' ', '\t', '\r', '\n', '>']).next().unwrap_or_default());
        }
    }
    None
}

/// False for script elements that hold data or templates (`application/json`, `importmap`, `text/template`).
fn script_type_ok(attrs: &str) -> bool {
    attr(attrs, "type").is_none_or(|kind| kind.is_empty() || ["javascript", "ecmascript", "module", "typescript", "jsx", "tsx"].iter().any(|word| kind.contains(word)))
}

/// Element text with tags removed, common entities decoded and whitespace folded.
fn clean(raw: &str) -> String {
    let mut plain = String::with_capacity(raw.len());
    let mut inside = false;
    for c in raw.chars() {
        match c {
            '<' => inside = true,
            '>' if inside => inside = false,
            _ if !inside => plain.push(c),
            _ => {}
        }
    }
    let decoded = plain.replace("&nbsp;", " ").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'").replace("&amp;", "&");
    decoded.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(TITLE_MAX).collect()
}

/// Doc symbols for `heads`: each section runs to the line before the next heading of the same or a higher level.
fn sections(text: &str, heads: &[Head]) -> Vec<Symbol> {
    let starts: Vec<usize> = std::iter::once(0).chain(text.match_indices('\n').map(|(at, _)| at + 1)).filter(|start| *start < text.len()).collect();
    let line_of = |offset: usize| u32::try_from(starts.partition_point(|start| *start <= offset)).unwrap_or(u32::MAX).max(1);
    let spans = SpanIndex::build(text);
    let total = u32::try_from(starts.len()).unwrap_or(u32::MAX).max(1);
    heads
        .iter()
        .enumerate()
        .map(|(index, head)| {
            let first = line_of(head.at);
            let next = heads.get(index + 1..).unwrap_or_default().iter().find(|other| other.level <= head.level);
            let last = next.map_or(total, |other| line_of(other.at).saturating_sub(1)).max(first);
            let lines = [first, last];
            Symbol {
                name: head.title.clone(),
                kind: "heading".to_string(),
                graph: NodeKind::Doc,
                lines,
                span: spans.line_span(lines),
                summary: head.title.clone(),
                summary_source: "heading".to_string(),
                signature: String::new(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests;
