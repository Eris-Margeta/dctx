// SPDX-License-Identifier: Apache-2.0
//! Lua: functions (`function a.b(`, `local function x(`, `name = function(`), table fields (`M.x = ...`), local tables and upper-case constants.
//! A function is `fn`, `Table.field = value` a `field`, `local t = {` a `var` and `local MAX = 1` a `const`.

use super::super::{ident_at, is_denied, mask, Syntax};
use super::{Block, Tok};

/// How Lua reads: `--` comments, `[[ ]]` strings, blocks closed by `end` or `until`.
pub(super) const SCAN: Block = Block { mask: mask_of, decl: decl_of, word: word_of, flat: false, nested: true };

/// Lua: `--[[ ]]` comments first, then `--` comments and strings, then `[[ ]]` long strings.
fn mask_of(text: &str) -> String {
    let long_comments = mask(text, &Syntax { line: &[], block: Some(("--[[", "]]")), nested: false, quotes: b"", triple: false });
    let strings = mask(&long_comments, &Syntax { line: &["--"], block: None, nested: false, quotes: b"\"'", triple: false });
    mask(&strings, &Syntax { line: &[], block: Some(("[[", "]]")), nested: false, quotes: b"", triple: false })
}

/// Lua blocks open at `function`, `if`, `do` (loops included) and `repeat`; `end` and `until` close them.
fn word_of(word: &str, _next: Option<u8>) -> Tok {
    match word {
        "function" | "if" | "do" | "repeat" => Tok::Open,
        "end" | "until" => Tok::Close,
        _ => Tok::Neutral,
    }
}

/// The dotted or colon path that starts `text`, and what follows it.
fn path_of(text: &str) -> (&str, &str) {
    let end = text.find(|c: char| !(c.is_alphanumeric() || matches!(c, '_' | '.' | ':'))).unwrap_or(text.len());
    text.split_at(end)
}

/// `Table::name` for `Table.name` and `Table:name`, plain `name` without a table.
fn name_of(path: &str) -> Option<String> {
    let (parent, last) = match path.rfind(['.', ':']) {
        Some(cut) => (&path[..cut], &path[cut + 1..]),
        None => ("", path),
    };
    if last.is_empty() || last.starts_with(|c: char| c.is_ascii_digit()) || is_denied(last) {
        return None;
    }
    Some(if parent.is_empty() { last.to_string() } else { format!("{parent}::{last}") })
}

/// True for `MAX_SIZE`: at least two characters, upper case letters, digits and underscores only.
fn is_constant(name: &str) -> bool {
    name.len() > 1 && name.starts_with(|c: char| c.is_ascii_uppercase()) && name.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

/// `function a.b(`, `function a:b(`, `local function x(`, `name = function(`, `tests["name"] = function(` (fn); `a.b = value` (field);
/// `t = {` (var); `MAX = value` (const).
fn decl_of(line: &str, source: &str, _owner: Option<&str>) -> Option<(String, &'static str)> {
    let mut rest = line;
    let local = ident_at(rest) == Some("local");
    if local {
        rest = rest["local".len()..].trim_start();
    }
    if ident_at(rest) == Some("function") {
        let after = rest["function".len()..].strip_prefix(char::is_whitespace)?.trim_start();
        let (path, tail) = path_of(after);
        return tail.trim_start().starts_with('(').then(|| name_of(path)).flatten().map(|name| (name, "fn"));
    }
    let (path, tail) = path_of(rest);
    if tail.starts_with('[') {
        return keyed(path, tail, source.get(line.len() - tail.len()..)?);
    }
    let value = tail.trim_start().strip_prefix('=').filter(|after| !after.starts_with('='))?.trim_start();
    let name = name_of(path)?;
    if ident_at(value) == Some("function") && value["function".len()..].trim_start().starts_with('(') {
        return Some((name, "fn"));
    }
    if path.contains(':') {
        return None;
    }
    if path.contains('.') {
        return Some((name, "field"));
    }
    if value.starts_with('{') {
        return Some((name, "var"));
    }
    is_constant(&name).then_some((name, "const"))
}

/// `tests["lume.clamp"] = function(`: the function stored under a string key, named `tests["lume.clamp"]` as written.
/// `tail` is the masked text from the `[` on and `source` the same text unmasked.
fn keyed(path: &str, tail: &str, source: &str) -> Option<(String, &'static str)> {
    let close = tail.find(']')?;
    let key = source.get(1..close)?.trim();
    let quoted = key.len() >= 2 && ((key.starts_with('"') && key.ends_with('"')) || (key.starts_with('\'') && key.ends_with('\'')));
    let value = tail[close + 1..].trim_start().strip_prefix('=').filter(|after| !after.starts_with('='))?.trim_start();
    let function = ident_at(value) == Some("function") && value["function".len()..].trim_start().starts_with('(');
    let table = name_of(path)?;
    (quoted && function && key.chars().count() <= 80 && !path.contains(':')).then(|| (format!("{table}[{key}]"), "fn"))
}
