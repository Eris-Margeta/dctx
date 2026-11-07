// SPDX-License-Identifier: Apache-2.0
//! Erlang declarations: the `-module(name).` attribute is the module, the `-record(name, ..)` and
//! `-type name(` attributes are types, and the clauses `name(Args) [when ..] ->` written at column 0,
//! separated by `;` and closed by a full stop at the end of a line, are one fn symbol per function.

use super::super::{Decl, Syntax, ident_at, is_denied, mask, signature_of};

/// Comment and string syntax of Erlang: `%` to the end of the line, quoted strings and single-quoted atoms.
const SYNTAX: Syntax = Syntax {
    line: &["%"],
    block: None,
    nested: false,
    quotes: b"\"'",
    triple: true,
};

/// Erlang words that may never start a clause. `is_denied` covers the ones shared with the other families.
const NOT_HEADS: &[&str] = &[
    "after", "begin", "catch", "cond", "end", "fun", "let", "maybe", "not", "of", "receive", "try",
    "when",
];

/// Erlang declarations of `text`.
pub(super) fn decls(text: &str) -> Vec<Decl> {
    let masked = masked(text);
    let mut decls: Vec<Decl> = Vec::new();
    // The clause head whose `->` has not appeared yet, the function whose final full stop has not appeared yet,
    // and the parentheses of an unfinished clause head.
    let (mut waiting, mut body) = (None, None);
    let mut parens = 0usize;
    for (index, (shown, original)) in masked.split('\n').zip(text.split('\n')).enumerate() {
        let number = u32::try_from(index + 1).unwrap_or(u32::MAX);
        let line = shown.trim();
        if line.is_empty() {
            continue;
        }
        // A fresh declaration only starts at column 0, outside any open clause head or body.
        if parens == 0
            && waiting.is_none()
            && body.is_none()
            && !shown.starts_with(char::is_whitespace)
        {
            if let Some(decl) = attribute(line, original, number) {
                decls.push(decl);
                continue;
            }
            if let Some(name) = clause_head(shown) {
                decls.push(Decl {
                    name,
                    kind: "fn",
                    lines: [number, number],
                    signature: signature_of(original),
                });
                waiting = Some(decls.len() - 1);
            }
        }
        // A prepared head becomes the open body once its `->` shows up with closed parameters.
        if let Some(at) = waiting {
            for byte in shown.bytes() {
                match byte {
                    b'(' => parens += 1,
                    b')' => parens = parens.saturating_sub(1),
                    _ => {}
                }
            }
            if parens == 0 && shown.contains("->") {
                waiting = None;
                body = Some(at);
            }
        }
        // The open body only matters through its final full stop.
        if let Some(at) = body
            && line.ends_with('.')
        {
            decls[at].lines[1] = number;
            body = None;
        }
    }
    decls
}

/// The source with `%` comments, quoted strings and `$c` char literals blanked, keeping every byte offset.
/// A `$` outside comments and strings only opens a char literal, so blanking the pair is safe.
pub(in crate::index::generic) fn masked(text: &str) -> String {
    let mut out = mask(text, &SYNTAX).into_bytes();
    let mut at = 0usize;
    while at < out.len() {
        if out[at] == b'$' {
            out[at] = b' ';
            // The character after the `$` may use several bytes; blanking all of them keeps offsets and validity.
            // A `$` at the end of a line is garbage; the line break itself must survive blanking.
            let skip = match out.get(at + 1) {
                None | Some(b'\n' | b'\r') | Some(0..=0xBF) => 1,
                Some(0xC0..=0xDF) => 2,
                Some(0xE0..=0xEF) => 3,
                _ => 4,
            };
            for slot in out.iter_mut().take(at + skip + 1).skip(at) {
                *slot = b' ';
            }
            at += skip + 1;
            continue;
        }
        at += 1;
    }
    String::from_utf8(out).unwrap_or_default()
}

/// `-module(name).` as a module (kind `module`), `-record(name, ..)` and `-type name(` as types.
/// Every other attribute stays unnamed.
fn attribute(line: &str, original: &str, number: u32) -> Option<Decl> {
    let word = ident_at(line.strip_prefix('-')?)?;
    let rest = line[word.len() + 1..].trim_start();
    let (name, kind) = if word == "module" {
        let close = rest.find(')')?;
        let dotted = rest[close..].trim_end().ends_with('.');
        (atom(rest[1..close].trim()).filter(|_| dotted)?, "module")
    } else if word == "record" {
        let close = rest.find([',', ')'])?;
        (atom(rest[1..close].trim())?, "type")
    } else if word == "type" {
        let open = rest.find('(')?;
        (atom(rest[..open].trim())?, "type")
    } else {
        return None;
    };
    Some(Decl {
        name,
        kind,
        lines: [number, number],
        signature: signature_of(original),
    })
}

/// The plain lower-case atom `[a-z][a-z0-9_@]*` that names a module, a record, a type or a clause.
fn atom(text: &str) -> Option<String> {
    let valid = !text.is_empty()
        && text.starts_with(|c: char| c.is_ascii_lowercase())
        && text
            .chars()
            .all(|c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '@'));
    let name = valid.then(|| text.to_string())?;
    (!is_denied(&name) && !NOT_HEADS.contains(&name.as_str())).then_some(name)
}

/// The function name when a column-0 line opens a clause: `name(Args)`, followed by `->` here or after a guard.
/// A leading `-` is an attribute, and an uppercase- or `_`-initial name is a variable, never a clause.
fn clause_head(shown: &str) -> Option<String> {
    let end = shown
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '@'))
        .unwrap_or(shown.len());
    let name = atom(&shown[..end])?;
    shown[end..].trim_start().starts_with('(').then_some(name)
}

#[cfg(test)]
mod tests {
    use super::super::symbols;

    /// Erlang shapes: the module attribute, the record and type attributes, and a clause list that is one fn symbol
    /// from its first clause to the full stop that ends it.
    #[test]
    fn erlang_lists_module_records_types_and_clause_lists() {
        let text = "-module(shop).\n\n-record(item, {price, label}).\n-type cents() :: integer().\n\n%% price(X) -> hidden is a comment\nprice(#item{price = P}, N) ->\n  P * N;\nprice(_, _) ->\n  0.\n\nshown() ->\n  price(#item{price = 2}, 3).\n";
        let found = symbols("erlang", text);
        let listed: Vec<(&str, &str, [u32; 2])> = found
            .iter()
            .map(|symbol| (symbol.name.as_str(), symbol.kind.as_str(), symbol.lines))
            .collect();
        assert_eq!(
            listed,
            [
                ("shop", "module", [1, 1]),
                ("item", "type", [3, 3]),
                ("cents", "type", [4, 4]),
                ("price", "fn", [7, 10]),
                ("shown", "fn", [12, 13])
            ]
        );
    }

    /// A truncated Erlang file that leaves a head, a clause or a body open never panics.
    #[test]
    fn truncated_erlang_never_panics_and_keeps_the_module() {
        let samples = [
            "-module(shop).",
            "-module(shop).\n\nprice(#item{price = P\n",
            "-record(item, {price\n",
            "-type cents(\ncents() ->\n",
            "-module(shop).\nprice(X) ->\n",
            "price(X);",
            "shown().",
        ];
        for sample in samples {
            let found = symbols("erlang", sample);
            assert!(
                found.iter().all(|symbol| !symbol.name.is_empty()),
                "{sample}: {found:?}"
            );
        }
        let found = symbols("erlang", "-module(shop).\n\nprice(X) ->\n");
        assert!(found.iter().any(|symbol| (
            symbol.name.as_str(),
            symbol.kind.as_str(),
            symbol.lines
        ) == ("shop", "module", [1, 1])));
    }
}
