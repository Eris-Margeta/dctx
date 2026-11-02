//! Perl extraction for the brace family.
//!
//! A `package A::B;` (or a `package A { .. }` block) opens a type that holds every later `sub` until the next package,
//! so a sub is named `A::B::name`. POD (`=pod`/`=head1`/`=item`/`=begin` at line start up to `=cut`) is masked like a
//! comment and never names a symbol. Calls are `name(..)`, a bareword `name` followed by whitespace and an argument, and
//! `&name`; each names a sub defined in the file or the project and is never reported after `->`, `sub`, `$`, `@` or `%`.
//! Every function is pure, line-oriented and panic-free on truncated or malformed input.

use std::collections::HashSet;

use super::super::{Decl, ident_at, is_denied, mask, signature_of, to_symbols};
use super::PERL;
use crate::index::extract::Symbol;

/// POD directive words that open a documentation block.
const POD_OPEN: &[&str] = &[
    "=pod",
    "=head1",
    "=head2",
    "=head3",
    "=head4",
    "=head5",
    "=item",
    "=begin",
    "=over",
    "=encoding",
];

/// Perl builtins and keywords a bare `name argument` or `&name` must not report as a call.
const PERL_BUILTINS: &[&str] = &[
    "print",
    "printf",
    "say",
    "warn",
    "die",
    "return",
    "bless",
    "ref",
    "defined",
    "undef",
    "local",
    "our",
    "my",
    "push",
    "pop",
    "shift",
    "unshift",
    "splice",
    "int",
    "rand",
    "srand",
    "sprintf",
    "join",
    "split",
    "chomp",
    "chop",
    "length",
    "ord",
    "chr",
    "index",
    "rindex",
    "substr",
    "keys",
    "values",
    "each",
    "delete",
    "exists",
    "sort",
    "map",
    "grep",
    "system",
    "exec",
    "fork",
    "open",
    "close",
    "read",
    "write",
    "seek",
    "tell",
    "binmode",
    "select",
    "lock",
    "caller",
    "eval",
    "goto",
    "wantarray",
    "scalar",
    "prototype",
    "require",
    "do",
    "next",
    "last",
    "redo",
    "break",
    "continue",
    "tie",
    "untie",
    "tied",
    "mkdir",
    "rmdir",
    "glob",
    "umask",
    "chmod",
    "chown",
    "unlink",
    "rename",
    "link",
    "truncate",
];

/// One `package` seen in the masked text: its line (1-based), qualified name, and the line on which a `{ .. }` block
/// package closes (so a block package stops holding subs at its end).
struct Package {
    /// 1-based line of the `package`.
    line: u32,
    /// Qualified package name.
    name: String,
    /// 1-based closing line of the block, when the package opens one.
    end: Option<u32>,
}

/// Copy of `text` with every POD block blanked like a comment. Each line keeps its length and position, so the masked
/// text stays aligned with `text` (which keeps the per-line zip and the byte spans in the brace scanner intact).
pub(super) fn mask_pod(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_pod = false;
    for (index, line) in text.split('\n').enumerate() {
        if index > 0 {
            out.push('\n');
        }
        let trimmed = line.trim_start();
        let word = trimmed.split_whitespace().next().unwrap_or("");
        if in_pod {
            if word == "=cut" {
                in_pod = false;
            }
            out.push_str(" ".repeat(line.len()).as_str());
            continue;
        }
        if POD_OPEN.contains(&word) {
            in_pod = true;
            out.push_str(" ".repeat(line.len()).as_str());
            continue;
        }
        out.push_str(line);
    }
    out
}

/// Perl symbols of `text`: one type per `package` and one function per `sub`, each qualified by the enclosing package.
/// Returns none for anything but the `perl` tag.
pub(super) fn symbols(tag: &str, text: &str) -> Vec<Symbol> {
    if tag != PERL.tag {
        return Vec::new();
    }
    let masked = mask(&mask_pod(text), &PERL.syntax);
    let packages = packages(&masked);
    let mut decls: Vec<Decl> = Vec::new();
    let mut current: Option<usize> = None;
    let mut next_package = 0usize;
    for (index, line) in masked.split('\n').enumerate() {
        let number = u32::try_from(index + 1).unwrap_or(u32::MAX);
        while next_package < packages.len() && packages[next_package].line <= number {
            current = Some(next_package);
            next_package += 1;
        }
        if current.is_some_and(|at| packages[at].end.is_some_and(|end| number > end)) {
            current = None;
        }
        let trimmed = line.trim();
        if package_at(trimmed).is_some() {
            if let Some(name) = current.map(|at| packages[at].name.clone()) {
                decls.push(Decl {
                    name,
                    kind: "type",
                    lines: [number, number],
                    signature: signature_of(trimmed),
                });
            }
            continue;
        }
        if let Some(name) = sub_at(trimmed) {
            let name = match current.map(|at| packages[at].name.as_str()) {
                Some(owner) if !owner.is_empty() => format!("{owner}::{name}"),
                _ => name,
            };
            decls.push(Decl {
                name,
                kind: "fn",
                lines: [number, sub_end(&masked, index)],
                signature: signature_of(trimmed),
            });
        }
    }
    // A `package A::B;` (not a block) spans the subs it holds, so stretch its type to the last member in its namespace.
    let stretch: Vec<(usize, u32)> = decls
        .iter()
        .enumerate()
        .filter(|(_, decl)| decl.kind == "type")
        .filter_map(|(at, decl)| {
            let prefix = format!("{}::", decl.name);
            decls
                .iter()
                .filter(|other| other.kind == "fn" && other.name.starts_with(&prefix))
                .map(|other| other.lines[1])
                .max()
                .map(|last| (at, last))
        })
        .collect();
    for (at, last) in stretch {
        decls[at].lines[1] = decls[at].lines[1].max(last);
    }
    to_symbols(text, decls)
}

/// The `package` declarations in the masked `text`, in source order.
fn packages(masked: &str) -> Vec<Package> {
    let mut found: Vec<Package> = Vec::new();
    for (index, line) in masked.split('\n').enumerate() {
        let number = u32::try_from(index + 1).unwrap_or(u32::MAX);
        let trimmed = line.trim();
        let Some(name) = package_at(trimmed) else {
            continue;
        };
        let end = if trimmed.contains('{') {
            block_end(masked, index)
        } else {
            None
        };
        found.push(Package {
            line: number,
            name,
            end,
        });
    }
    found
}

/// The qualified name a `package` line declares, or `None` when the line is not one.
fn package_at(line: &str) -> Option<String> {
    let rest = line.strip_prefix("package")?;
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let rest = rest.trim_start();
    let end = rest
        .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == ':'))
        .unwrap_or(rest.len());
    let name = &rest[..end];
    (!name.is_empty() && !name.starts_with(|c: char| c.is_ascii_digit()) && !is_denied(name))
        .then(|| name.to_string())
}

/// The name a `sub` line declares, or `None` when the line is not one.
fn sub_at(line: &str) -> Option<String> {
    let rest = line.strip_prefix("sub")?;
    if !rest.starts_with(|c: char| c.is_whitespace() || c == '{') {
        return None;
    }
    ident_at(rest.trim_start())
        .filter(|name| !is_denied(name) && !name.starts_with(|c: char| c.is_ascii_digit()))
        .map(ToString::to_string)
}

/// The 1-based line on which the block opened on line `start` (0-based) closes, or `None` when it never does.
fn block_end(masked: &str, start: usize) -> Option<u32> {
    let mut depth = 0u32;
    let mut seen = false;
    for (index, scan) in masked.split('\n').enumerate().skip(start) {
        for byte in scan.bytes() {
            if byte == b'{' {
                depth = depth.saturating_add(1);
                seen = true;
            } else if byte == b'}' && seen {
                depth = depth.saturating_sub(1);
            }
        }
        if seen && depth == 0 {
            return u32::try_from(index + 1).ok();
        }
    }
    None
}

/// The 1-based line on which the `sub` declared on line `start` (0-based) closes, or the declaration line when its body
/// never closes. Hash-reference braces count as nested braces, so they cannot close the function body early.
fn sub_end(masked: &str, start: usize) -> u32 {
    block_end(masked, start).unwrap_or_else(|| u32::try_from(start + 1).unwrap_or(u32::MAX))
}

/// Called names in one function's masked body, in source order without repeats. A call is `name(..)`, a bareword `name`
/// followed by whitespace and an argument, or `&name`; never a Perl builtin, a method after `->`, or a name after
/// `sub`, `$`, `@` or `%`.
pub(super) fn call_names<'a>(body: &'a str, own: &str, defined: &HashSet<&str>) -> Vec<&'a str> {
    let bytes = body.as_bytes();
    let first_line_end = body.find('\n').unwrap_or(body.len());
    let mut found: Vec<&str> = Vec::new();
    let mut at = 0usize;
    while at < bytes.len() {
        if !bytes[at].is_ascii_alphabetic() && bytes[at] != b'_' {
            at += 1;
            continue;
        }
        let start = at;
        while at < bytes.len() && (bytes[at].is_ascii_alphanumeric() || bytes[at] == b'_') {
            at += 1;
        }
        let word = &body[start..at];
        let before = if start > 0 {
            Some(bytes[start - 1])
        } else {
            None
        };
        let receiver = matches!(before, Some(b'>'));
        let variable = matches!(before, Some(b'$' | b'@' | b'%'));
        let after = &body[at..];
        let open = after_startswith(after, b'(');
        let amp = before == Some(b'&');
        let arg = amp || bareword_arg(after);
        let own_declaration = start < first_line_end && word == own;
        let name = if !receiver
            && !variable
            && !own_declaration
            && (open || arg)
            && !is_denied(word)
            && !PERL_BUILTINS.contains(&word)
            && defined.contains(word)
        {
            Some(word)
        } else {
            None
        };
        if let Some(word) = name
            && !found.contains(&word)
        {
            found.push(word);
        }
    }
    found
}

/// True when `text` starts with `needle`.
fn after_startswith(text: &str, needle: u8) -> bool {
    text.as_bytes().first() == Some(&needle)
}

/// True when `text` is whitespace followed by an argument byte: a bareword call with an argument.
fn bareword_arg(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut saw_space = false;
    for byte in bytes.iter() {
        if *byte == b' ' || *byte == b'\t' {
            saw_space = true;
        } else {
            return saw_space;
        }
    }
    false
}
