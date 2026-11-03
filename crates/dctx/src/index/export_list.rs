// SPDX-License-Identifier: Apache-2.0
//! A file's explicit export list (DCX-55b): Python `__all__`, CommonJS `module.exports` / `exports.x`, ES `export { .. }`.
//! Pure text scan; `session::export_order_op` uses the list to order a file's outline (DCX-55c).

use std::collections::BTreeSet;

#[cfg(test)]
#[path = "export_list_tests.rs"]
mod tests;

/// The names `text` exports explicitly, or `None` when the file has no export list.
/// Python (`language` "python"): the string items of a top-level `__all__ = [..]` or `(..)` (may span lines) and of every
/// `__all__ += [..]`. JavaScript/TypeScript ("javascript", "typescript"): the key names of `module.exports = { a, b: c }`,
/// every `exports.x =` and `module.exports.x =`, and the local names of `export { a, b as c }` (`a`, `b`); an
/// `export { .. } from ".."` re-export is skipped. Any other language: `None`.
pub(crate) fn names(language: &str, text: &str) -> Option<BTreeSet<String>> {
    let lang = language.trim().to_ascii_lowercase();
    let mut found = BTreeSet::new();
    if lang.contains("python") {
        python_all_names(text, &mut found);
    } else if lang.contains("javascript") || lang.contains("typescript") {
        commonjs_export_names(text, &mut found);
        export_list_names(text, &mut found);
    } else {
        return None;
    }
    (!found.is_empty()).then_some(found)
}

/// True for the bytes that can make up an identifier in Python/JavaScript (`_`, `$`, letters, digits).
fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$'
}

/// Index of the first byte at or after `at` that is not space/tab/CR/LF.
fn skip_ws(bytes: &[u8], mut at: usize) -> usize {
    while bytes.get(at).is_some_and(|b| b.is_ascii_whitespace()) {
        at += 1;
    }
    at
}

/// Index one past an identifier run starting at `at` (so `at` itself when there is none).
fn word_end(bytes: &[u8], mut at: usize) -> usize {
    while bytes.get(at).is_some_and(|b| is_word_byte(*b)) {
        at += 1;
    }
    at
}

/// The word formed by the identifier run starting at `at`, or `None`.
fn word_at(text: &str, at: usize) -> Option<String> {
    let bytes = text.as_bytes();
    let end = word_end(bytes, at);
    (end > at).then(|| text[at..end].to_string())
}

/// The identifier right after `at`, which must be at whitespace before a word.
fn word_after(text: &str, at: usize) -> Option<String> {
    word_at(text, skip_ws(text.as_bytes(), at))
}

/// Scans the single quoted Python/JS string literal starting at `at` (which must hold `'` or `"` or a template
/// backtick); returns the unescaped content and the index just past the closing quote. An unterminated literal
/// runs to the end of `text`.
fn scan_string(text: &str, at: usize) -> (Option<String>, usize) {
    let bytes = text.as_bytes();
    let quote = bytes[at];
    let mut content = String::new();
    let mut i = at + 1;
    while i < bytes.len() {
        let b = bytes[i];
        if b == quote {
            return (Some(content), i + 1);
        }
        if b == b'\\' && i + 1 < bytes.len() {
            let ch = text[i + 1..].chars().next().unwrap_or('\\');
            match ch {
                'n' => content.push('\n'),
                't' => content.push('\t'),
                'r' => content.push('\r'),
                other => content.push(other),
            }
            i += 1 + ch.len_utf8();
            continue;
        }
        let ch = text[i..].chars().next().unwrap_or('\u{fffd}');
        content.push(ch);
        i += ch.len_utf8();
    }
    (None, bytes.len())
}

/// Adds the string items of every Python `__all__` list in `text` (only lines starting with `__all__ =`
/// or `__all__ +=`) to `names`.
fn python_all_names(text: &str, names: &mut BTreeSet<String>) {
    let mut offset = 0usize;
    for line in text.split('\n') {
        let lead = line.trim_start();
        if let Some(after) = lead.strip_prefix("__all__") {
            // Space(s) between `__all__` and the `=` / `+=` are allowed.
            let rest = after.trim_start_matches([' ', '\t']);
            let bytes = rest.as_bytes();
            let after_eq = if bytes.first() == Some(&b'+') && bytes.get(1) == Some(&b'=') {
                2
            } else if bytes.first() == Some(&b'=') {
                1
            } else {
                0
            };
            if after_eq > 0 {
                let skip = after.len() - rest.len();
                let start = offset + (line.len() - lead.len()) + "__all__".len() + skip + after_eq;
                names.extend(all_list_items(text, start));
            }
        }
        offset += line.len() + 1;
    }
}

/// The quoted-string items of the Python list whose opening `(` / `[` is searched from `start` (right after
/// the `=` / `+=` of an `__all__` assignment); items may span lines, and `# ..` comments are skipped.
fn all_list_items(text: &str, start: usize) -> BTreeSet<String> {
    let bytes = text.as_bytes();
    let mut i = start;
    // Whitespace and `# ..` comments before the opening delimiter.
    while i < bytes.len() {
        match bytes[i] {
            b'#' => i += bytes[i..].iter().take_while(|b| **b != b'\n').count() + 1,
            b if b.is_ascii_whitespace() => i += 1,
            _ => break,
        }
    }
    if !matches!(bytes.get(i), Some(b'[') | Some(b'(')) {
        return BTreeSet::new();
    }
    let mut items = BTreeSet::new();
    let mut depth = 1usize;
    i += 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\'' | b'"' => {
                let (item, next) = scan_string(text, i);
                if let Some(item) = item.filter(|item| !item.is_empty()) {
                    items.insert(item);
                }
                i = next;
            }
            b'#' => i += bytes[i..].iter().take_while(|b| **b != b'\n').count() + 1,
            b'[' | b'(' | b'{' => {
                depth += 1;
                i += 1;
            }
            b']' | b')' | b'}' => {
                depth -= 1;
                i += 1;
                if depth == 0 {
                    return items;
                }
            }
            _ => i += 1,
        }
    }
    items
}

/// The leading key of one `{`-list entry: the entry's first identifier (the word before `:` /
/// ` as `, or the shorthand name), or the content when it starts with a string literal.
fn entry_head_word(entry: &str) -> Option<String> {
    let bytes = entry.as_bytes();
    let start = skip_ws(bytes, 0);
    match bytes.get(start) {
        Some(b'\'') | Some(b'"') => scan_string(entry, start).0,
        _ => word_at(entry, start),
    }
}

/// Reads the `{`-delimited list starting at `at` (which must hold `{`) up to its matching `}`; returns
/// the index just past that `}` plus one key per comma-separated entry (see `entry_head_word`) — for
/// `{ a, b }` the entries are `a` and `b`, for `{ a: c, "d": e }` they are `a` and `"d"`.
fn brace_entries(text: &str, at: usize) -> Option<(usize, Vec<String>)> {
    let bytes = text.as_bytes();
    let mut entries = Vec::new();
    let mut depth = 0usize;
    let mut entry_start = at + 1;
    let mut i = at + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\'' | b'"' | b'`' => i = scan_string(text, i).1,
            b'{' | b'[' | b'(' => {
                depth += 1;
                i += 1;
            }
            b'}' | b']' | b')' => {
                i += 1;
                if depth == 0 {
                    if let Some(word) = entry_head_word(&text[entry_start..i - 1]) {
                        entries.push(word);
                    }
                    return Some((i, entries));
                }
                depth -= 1;
            }
            b',' if depth == 0 => {
                if let Some(word) = entry_head_word(&text[entry_start..i]) {
                    entries.push(word);
                }
                entry_start = i + 1;
                i += 1;
            }
            _ => i += 1,
        }
    }
    None
}

/// Adds the key names of every `module.exports = { .. }` object and the property names of every
/// `exports.<name> =` / `module.exports.<name> =` assignment in JS/TS `text` to `names`.
fn commonjs_export_names(text: &str, names: &mut BTreeSet<String>) {
    let bytes = text.as_bytes();

    // `module.exports = { .. }`.
    let mut from = 0usize;
    while let Some(rel) = text[from..].find("module.exports") {
        let at = from + rel;
        from = at + 1;
        if at > 0 && is_word_byte(bytes[at - 1]) {
            continue;
        }
        let s = skip_ws(bytes, at + "module.exports".len());
        if bytes.get(s) != Some(&b'=') {
            continue;
        }
        let s = skip_ws(bytes, s + 1);
        if bytes.get(s) != Some(&b'{') {
            continue;
        }
        if let Some((end, keys)) = brace_entries(text, s) {
            names.extend(keys);
            from = end;
        }
    }

    // `exports.<name> = ...` (also after `module.`).
    let mut from = 0usize;
    while let Some(rel) = text[from..].find("exports.") {
        let at = from + rel;
        from = at + 1;
        if at > 0 && is_word_byte(bytes[at - 1]) {
            continue;
        }
        let name_end = word_end(bytes, at + "exports.".len());
        if name_end == at + "exports.".len() {
            continue;
        }
        let s = skip_ws(bytes, name_end);
        if bytes.get(s) != Some(&b'=') || bytes.get(s + 1) == Some(&b'=') {
            continue;
        }
        names.insert(text[at + "exports.".len()..name_end].to_string());
    }
}

/// Adds the local names of every ES `export { .. }` list in JS/TS `text` to `names`, skipping
/// `export { .. } from ..` re-exports.
fn export_list_names(text: &str, names: &mut BTreeSet<String>) {
    let bytes = text.as_bytes();
    let mut from = 0usize;
    while let Some(rel) = text[from..].find("export") {
        let at = from + rel;
        from = at + 1;
        // A word of its own: not part of `module.exports` (`..exportx`) or a longer identifier.
        if at > 0 && is_word_byte(bytes[at - 1]) {
            continue;
        }
        if bytes
            .get(at + "export".len())
            .is_some_and(|b| is_word_byte(*b))
        {
            continue;
        }
        let brace = skip_ws(bytes, at + "export".len());
        if bytes.get(brace) != Some(&b'{') {
            continue;
        }
        if let Some((end, locals)) = brace_entries(text, brace) {
            let reexport = word_after(text, end).is_some_and(|w| w == "from");
            if !reexport {
                names.extend(locals);
            }
            from = end;
        }
    }
}
