//! Member-level declarations of the infra formats: Terraform `locals` keys, shell variables and aliases, protobuf fields and
//! enum values, GraphQL fields and enum values, Dockerfile `ARG` and `ENV` names. Every function takes one masked, trimmed
//! line and answers with names and wire kinds; the caller owns nesting, spans and signatures. Never panics on odd input.

use super::super::{ident_at, is_denied};

/// First words of a protobuf line inside a message that are statements, never a field type.
const PROTO_STATEMENTS: &[&str] = &[
    "option", "syntax", "edition", "import", "package", "reserved", "extensions", "returns", "rpc", "message", "enum", "service", "oneof",
    "extend", "stream",
];

/// What a Dockerfile instruction line continues onto the next line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Carry {
    /// An `ENV a=b \` line: the next line holds more `key=value` pairs.
    Env,
    /// An `ARG a \` line: the next line holds more names.
    Arg,
}

/// Round and square bracket balance of a masked line: opens minus closes. Braces are followed by the caller.
pub(super) fn bracket_delta(line: &str) -> i32 {
    line.bytes().fold(0, |sum, byte| match byte {
        b'(' | b'[' => sum + 1,
        b')' | b']' => sum - 1,
        _ => sum,
    })
}

/// True for a plain variable or field name: letters, digits and `_`, not starting with a digit, not a keyword.
fn is_name(word: &str) -> bool {
    word.starts_with(|c: char| c.is_alphabetic() || c == '_') && word.chars().all(|c| c.is_alphanumeric() || c == '_') && !is_denied(word)
}

/// Terraform `key = value` directly inside a `locals` block: the key, as a `const`.
pub(super) fn locals_key(line: &str) -> Option<(String, &'static str)> {
    let end = line.find(|c: char| !(c.is_alphanumeric() || matches!(c, '_' | '-'))).unwrap_or(line.len());
    let name = &line[..end];
    let rest = line[end..].trim_start();
    let valid = name.starts_with(|c: char| c.is_alphabetic() || c == '_') && !is_denied(name);
    (valid && rest.starts_with('=') && !rest.starts_with("==") && !rest.starts_with("=>")).then(|| (name.to_string(), "const"))
}

/// Shell names declared by one top-level line: `alias a=...` (`fn`), `readonly` and `declare -r` (`const`), `export` and
/// the other `declare` forms (`var`). Only `NAME=value` words count; `-f` and `-p` forms declare nothing.
pub(super) fn shell_vars(line: &str) -> Vec<(String, &'static str)> {
    let mut words = line.split_whitespace();
    let Some(first) = words.next() else {
        return Vec::new();
    };
    let words: Vec<&str> = words.collect();
    let mut flags = String::new();
    for word in &words {
        flags.push_str(word.strip_prefix('-').unwrap_or_default());
    }
    let kind = match first {
        "alias" => "fn",
        "readonly" => "const",
        "export" | "declare" | "typeset" if flags.contains(['f', 'F', 'p']) => return Vec::new(),
        "declare" | "typeset" if flags.contains('r') => "const",
        "export" | "declare" | "typeset" => "var",
        _ => return Vec::new(),
    };
    words
        .into_iter()
        .filter(|word| !word.starts_with('-'))
        .filter_map(|word| {
            let name = word.split_once('=')?.0.trim_end_matches('+');
            let ok = if kind == "fn" { alias_name(name) } else { is_name(name) };
            ok.then(|| (name.to_string(), kind))
        })
        .collect()
}

/// True for an alias name: word characters and `-.:@+`, starting with a letter, digit or `_`.
fn alias_name(name: &str) -> bool {
    name.starts_with(|c: char| c.is_alphanumeric() || c == '_') && name.chars().all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | ':' | '@' | '+')) && !is_denied(name)
}

/// Protobuf `[label] type name = N;` inside a message (`field`) or `NAME = N;` inside an enum (`const`).
/// `container` is the keyword of the innermost body; anything else (service, rpc body, file level) has no members.
pub(super) fn proto_member(line: &str, container: Option<&str>) -> Option<(String, &'static str)> {
    let (head, tail) = line.split_once('=')?;
    let number = tail.trim_start().trim_start_matches('-');
    if !number.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    let tokens: Vec<&str> = head.split_whitespace().collect();
    let name = *tokens.last()?;
    if !is_name(name) {
        return None;
    }
    match container? {
        "message" if tokens.len() >= 2 && !PROTO_STATEMENTS.contains(&tokens[0]) => Some((name.to_string(), "field")),
        "enum" if tokens.len() == 1 && !PROTO_STATEMENTS.contains(&name) => Some((name.to_string(), "const")),
        _ => None,
    }
}

/// GraphQL `name: Type` / `name(args): Type` inside a `type`, `input` or `interface` (`field`), or a bare value in an `enum` (`const`).
pub(super) fn graphql_member(line: &str, container: Option<&str>) -> Option<(String, &'static str)> {
    let name = ident_at(line).filter(|name| !is_denied(name) && !name.contains('$'))?;
    let rest = line[name.len()..].trim_start();
    match container? {
        "type" | "input" | "interface" if rest.starts_with([':', '(']) => Some((name.to_string(), "field")),
        "enum" if rest.is_empty() || rest.starts_with('@') || rest.starts_with(',') => Some((name.to_string(), "const")),
        _ => None,
    }
}

/// Words of a Dockerfile line, split on white space outside quotes. A trailing line-continuation backslash is dropped.
fn docker_words(line: &str) -> Vec<String> {
    let mut words: Vec<String> = Vec::new();
    let mut word = String::new();
    let mut quote: Option<char> = None;
    for c in line.trim().trim_end_matches('\\').chars() {
        match quote {
            Some(open) => {
                word.push(c);
                if c == open {
                    quote = None;
                }
            }
            None if c.is_whitespace() => words.extend((!word.is_empty()).then(|| std::mem::take(&mut word))),
            None => {
                if c == '"' || c == '\'' {
                    quote = Some(c);
                }
                word.push(c);
            }
        }
    }
    words.extend((!word.is_empty()).then_some(word));
    words
}

/// Names an `ARG` or `ENV` line declares, and what carries onto the next line. `carry` is what the previous line left open.
/// `ENV key value` (the legacy form) names its key; `ENV a=1 b=2` and `ARG a b=1` name every word.
pub(super) fn docker_names(line: &str, carry: Option<Carry>) -> (Vec<String>, Option<Carry>) {
    let mut words = docker_words(line).into_iter();
    let kind = match carry {
        Some(kind) => kind,
        None => match words.next().map(|word| word.to_ascii_uppercase()).as_deref() {
            Some("ARG") => Carry::Arg,
            Some("ENV") => Carry::Env,
            _ => return (Vec::new(), None),
        },
    };
    let rest: Vec<String> = words.collect();
    let legacy = carry.is_none() && kind == Carry::Env && rest.first().is_some_and(|word| !word.contains('='));
    let names: Vec<String> = match (kind, legacy) {
        (_, true) => rest.first().cloned().into_iter().collect(),
        (Carry::Env, false) => rest.iter().filter_map(|word| word.split_once('=')).map(|(name, _)| name.to_string()).collect(),
        (Carry::Arg, false) => rest.iter().map(|word| word.split_once('=').map_or(word.as_str(), |(name, _)| name).to_string()).collect(),
    };
    let names = names.into_iter().filter(|name| is_name(name)).collect();
    let next = (line.trim_end().ends_with('\\') && !legacy).then_some(kind);
    (names, next)
}

/// The quoted text at the start of `text` (`"..."` or `'...'`, a backslash escapes in double quotes) and what follows it.
/// `None` when it does not close on the line.
fn quoted(text: &str) -> Option<(&str, &str)> {
    let quote = text.chars().next().filter(|c| matches!(c, '"' | '\''))?;
    let body = &text[1..];
    let mut escaped = false;
    for (at, c) in body.char_indices() {
        if escaped {
            escaped = false;
        } else if c == '\\' && quote == '"' {
            escaped = true;
        } else if c == quote {
            return Some((&body[..at], &body[at + 1..]));
        }
    }
    None
}

/// Terraform `dynamic "ingress" {` (masked `line`, unmasked `source`, both trimmed at the start): the block label.
pub(super) fn dynamic_label(line: &str, source: &str) -> Option<String> {
    let after = line.strip_prefix("dynamic").filter(|after| after.starts_with(char::is_whitespace))?;
    let (label, _) = quoted(source.get(line.len() - after.len()..)?.trim_start())?;
    let valid = !label.is_empty() && label.chars().all(|c| c.is_alphanumeric() || matches!(c, '_' | '-'));
    (valid && line.ends_with('{')).then(|| label.to_string())
}

/// A bats `@test "name" {` block (masked `line`, unmasked `source`, both trimmed at the start): `@test "name"`, as a `fn`.
/// A name over 80 characters is cut there.
pub(super) fn bats_test(line: &str, source: &str) -> Option<(String, &'static str)> {
    let after = line.strip_prefix("@test").filter(|after| after.starts_with(char::is_whitespace))?;
    let (name, tail) = quoted(source.get(line.len() - after.len()..)?.trim_start())?;
    let name: String = name.chars().take(80).collect();
    (tail.trim() == "{").then(|| (format!("@test \"{name}\""), "fn"))
}
