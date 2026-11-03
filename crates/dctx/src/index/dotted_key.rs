// SPDX-License-Identifier: Apache-2.0
//! Dotted config keys (`server.port`, `database.url`) for find (DCX-56b): the query shape, the config symbol that defines
//! the key, and the code lines that read it. Wired into find by `session::dotted_op` (DCX-56c).

#[cfg(test)]
#[path = "dotted_key_tests.rs"]
mod tests;

/// The segments of a dotted key query: two or more segments of `[A-Za-z_][A-Za-z0-9_-]*` joined by `.`, nothing else
/// (no spaces, no empty segment). `None` for any other query.
pub(crate) fn segments(query: &str) -> Option<Vec<&str>> {
    let parts: Vec<&str> = query.split('.').collect();
    if parts.len() < 2 {
        return None;
    }
    for part in &parts {
        if part.is_empty() {
            return None;
        }
        let mut chars = part.chars();
        let first = chars.next()?;
        if !first.is_ascii_alphabetic() && first != '_' {
            return None;
        }
        for ch in chars {
            if !ch.is_ascii_alphanumeric() && ch != '-' && ch != '_' {
                return None;
            }
        }
    }
    Some(parts)
}

/// True when a config symbol's qualified `name` spells `segments` exactly, with `::` or `.` between the parts
/// (`server::port` and `server.port` both match `["server", "port"]`). Case-sensitive.
#[allow(clippy::collapsible_if, clippy::manual_pattern_char_comparison)]
pub(crate) fn defines(name: &str, segments: &[&str]) -> bool {
    let parts: Vec<&str> = name.split(|c: char| c == ':' || c == '.')
        .filter(|s| !s.is_empty())
        .collect();
    if parts.len() != segments.len() {
        return false;
    }
    parts.iter().zip(segments.iter()).all(|(a, b)| *a == *b)
}

/// True when a code `line` reads the key: the whole dotted key quoted (`"a.b"` or `'a.b'`),
/// the last two segments as `["a"]` then optional spaces then `["b"]` (either quote),
/// and `.a.b` not followed by a letter, digit or `_`.
pub(crate) fn reads(line: &str, segments: &[&str]) -> bool {
    // Pattern 1: whole dotted key quoted ("a.b" or 'a.b')
    if try_quoted_key(line, segments) {
        return true;
    }
    // Pattern 2: last two segments as bracket subscripts ["a"] then ["b"] (optional spaces between)
    if try_bracket_subscripts(line, segments) {
        return true;
    }
    // Pattern 3: .a.b not followed by a letter, digit, or _
    if try_attribute_form(line, segments) {
        return true;
    }
    false
}

fn is_id_byte(b: u8) -> bool {
    matches!(b, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_')
}

fn try_quoted_key(line: &str, segments: &[&str]) -> bool {
    let joined = segments.join(".");
    for &quote in &['"', '\''] {
        let mut search_from = 0;
        while let Some(open_pos) = line[search_from..].find(quote) {
            let abs_pos = search_from + open_pos;
            let after_open = &line[abs_pos + 1..];
            if let Some(close_pos) = after_open.find(quote) {
                let content = &after_open[..close_pos];
                if content == joined {
                    // A key quoted at the very start of the line has no byte before it.
                    let before_char = abs_pos.checked_sub(1).and_then(|at| line.as_bytes().get(at));
                    let after_char = line.as_bytes().get(abs_pos + 1 + close_pos);
                    let before_ok = before_char.is_none_or(|&b| !is_id_byte(b));
                    let after_ok = after_char.is_none_or(|&b| !is_id_byte(b));
                    if before_ok && after_ok {
                        return true;
                    }
                }
            }
            search_from = abs_pos + 1;
        }
    }
    false
}

#[allow(clippy::collapsible_if)]
fn try_bracket_subscripts(line: &str, segments: &[&str]) -> bool {
    if segments.len() < 2 {
        return false;
    }
    let a = segments[segments.len() - 2];
    let b = segments[segments.len() - 1];

    // Check for bracket subscript pattern: ["a"] then optional spaces then ["b"]
    // or ['a'] then optional spaces then ['b'], with any combination of quote types
    let a_dq = format!("[\"{}\"]", a);
    let a_sq = format!("['{a}']");
    let b_dq = format!("[\"{b}\"]");
    let b_sq = format!("['{b}']");

    // Possible quote combinations for the two brackets
    let combos = [
        (&a_dq, &b_dq),   // ["a"]["b"]
        (&a_sq, &b_sq),   // ['a']['b']
        (&a_dq, &b_sq),   // ["a"]['b']
        (&a_sq, &b_dq),   // ['a]["b"]
    ];

    for (pat_a, pat_b) in &combos {
        let pat_a_str = &pat_a[..];
        let pat_b_str = &pat_b[..];
        // Check for consecutive pattern: ["a"]["b"]
        if line.contains(pat_a_str) && line.contains(pat_b_str) {
            if let Some(a_pos) = line.find(pat_a_str) {
                let after_a = &line[a_pos + pat_a_str.len()..];
                // Check for second pattern immediately after (no space)
                if after_a.starts_with(pat_b_str) {
                    return true;
                }
                // Check for second pattern with one space after
                if after_a.starts_with(&format!(" {} ", pat_b_str)) {
                    return true;
                }
                // Check for second pattern with leading space before it
                if after_a.starts_with(&format!("{} ", pat_b_str)) {
                    return true;
                }
            }
        }
    }

    // Also check with a space between the bracket pairs
    let with_space_single = format!("['{a}'] ['{b}']");
    let with_space_double = format!("[\"{}\"][\"{}\"]", a, b);
    if line.contains(&with_space_single) || line.contains(&with_space_double) {
        return true;
    }

    false
}

fn try_attribute_form(line: &str, segments: &[&str]) -> bool {
    if segments.len() < 2 {
        return false;
    }
    let a = segments[segments.len() - 2];
    let b = segments[segments.len() - 1];
    let pattern = format!(".{}.{}", a, b);
    if let Some(idx) = line.find(&pattern) {
        let after_b = &line[idx + pattern.len()..];
        let first_char = after_b.chars().next();
        match first_char {
            Some(c) if c.is_alphanumeric() || c == '_' => return false,
            None | Some(_) => return true,
        }
    }
    false
}

/// Config-language symbols of `generation` (files whose `extract::language_of` is `config`: YAML, TOML, JSON) whose qualified
/// name spells `segments` (see [`defines`]), as find records (`query::record_of`) with score 3000, in index order.
pub(crate) fn definitions(generation: &super::Generation, segments: &[&str]) -> Vec<crate::domain::AnswerRecord> {
    let text = &generation.text;
    generation
        .nodes
        .iter()
        .filter(|node| node.letter == b's' && super::extract::language_of(text.get(node.path)) == "config" && defines(&node.name, segments))
        .map(|node| {
            let mut record = super::query::record_of(text, node);
            record.score = DEFINITION_SCORE;
            record
        })
        .collect()
}

/// Score of a defining config symbol in a dotted-key find.
const DEFINITION_SCORE: i64 = 3000;
