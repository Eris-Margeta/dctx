// SPDX-License-Identifier: Apache-2.0
//! Infra and config formats: Terraform/HCL, shell, protobuf, GraphQL and Dockerfile stages (here), SQL, Makefile and Pascal
//! (in `lines`); member-level parsers (locals keys, shell variables, protobuf and GraphQL members, Dockerfile `ARG`/`ENV`) sit in
//! `members`. Every symbol is a guess. A declaration is picked from one masked line; brace-bodied formats follow brace
//! depth for the end line, and protobuf nests as `Parent::child`. Heuristic by design: bodies are never entered.

mod lines;
mod members;
#[cfg(test)]
mod tests;

use super::{ident_at, is_denied, mask, signature_of, to_symbols, Decl, Syntax};
use crate::index::extract::Symbol;

/// The language tags this family owns, as `extract::language_of` spells them.
const TAGS: &[&str] = &["hcl", "sql", "shell", "make", "docker", "proto", "pascal", "graphql"];

/// HCL: `#`, `//` and `/* */` comments; labels are quoted strings, masked here and read back from the original line.
const HCL: Syntax = Syntax { line: &["#", "//"], block: Some(("/*", "*/")), nested: false, quotes: b"\"", triple: false };
/// Shell: `#` comments, single and double quotes.
const SHELL: Syntax = Syntax { line: &["#"], block: None, nested: false, quotes: b"\"'", triple: false };
/// Protobuf: C-style comments and both quotes.
const PROTO: Syntax = Syntax { line: &["//"], block: Some(("/*", "*/")), nested: false, quotes: b"\"'", triple: false };
/// GraphQL: `#` comments and `"""` block descriptions.
const GRAPHQL: Syntax = Syntax { line: &["#"], block: None, nested: false, quotes: b"\"", triple: true };
/// Dockerfile: `#` comments only.
const DOCKER: Syntax = Syntax { line: &["#"], block: None, nested: false, quotes: b"", triple: false };

/// True when `language` is one of the eight infra tags.
pub(super) fn handles(language: &str) -> bool {
    TAGS.contains(&language)
}

/// Symbols of `text` for an infra tag. Anything else, and any malformed input, yields none.
pub(super) fn symbols(language: &str, text: &str) -> Vec<Symbol> {
    let decls = match language {
        "hcl" => hcl(text),
        "shell" => shell(text),
        "proto" => proto(text),
        "graphql" => graphql(text),
        "docker" => docker(text),
        "sql" => lines::sql(text),
        "make" => lines::make(text),
        "pascal" => lines::pascal(text),
        _ => Vec::new(),
    };
    to_symbols(text, decls)
}

/// 1-based line number, masked line and original line, for every line of `text`.
pub(super) fn numbered<'a>(masked: &'a str, text: &'a str) -> impl Iterator<Item = (u32, &'a str, &'a str)> + 'a {
    masked
        .split('\n')
        .zip(text.split('\n'))
        .enumerate()
        .map(|(index, (shown, original))| (u32::try_from(index + 1).unwrap_or(u32::MAX), shown, original))
}

/// One open brace: the type whose body it opens (so members are visible) and the declaration it closes.
struct Frame {
    /// Qualified name of the type whose body this is.
    container: Option<String>,
    /// Index of the declaration whose end line this brace sets.
    decl: Option<usize>,
    /// Keyword that opened the container (`message`, `enum`, `type`, ...), empty outside a container.
    word: String,
}

/// Brace bookkeeping shared by the brace-bodied formats.
struct Walk {
    /// Declarations found so far, in source order.
    decls: Vec<Decl>,
    /// Open braces, innermost last.
    stack: Vec<Frame>,
    /// A declaration whose `{` has not appeared yet: it may open on a following line of its own.
    pending: Option<usize>,
    /// True when a `type` declaration is a container whose members are qualified with its name.
    nest: bool,
    /// Set for a line whose brace group (a protobuf `oneof`) keeps the surrounding container; consumed by the next `finish`.
    inherit: bool,
}

impl Walk {
    /// A walk that nests members under `type` declarations when `nest` is set.
    fn new(nest: bool) -> Self {
        Self { decls: Vec::new(), stack: Vec::new(), pending: None, nest, inherit: false }
    }

    /// The declaration waiting for this line's brace, if any. Call first on every non-blank line.
    fn begin(&mut self, line: &str) -> Option<usize> {
        if !line.starts_with('{') {
            self.pending = None;
        }
        self.pending.take()
    }

    /// True at file level and directly inside a container, where declarations may appear.
    fn in_scope(&self) -> bool {
        self.stack.last().is_none_or(|frame| frame.container.is_some())
    }

    /// The keyword of the container directly around this point (`message`, `enum`, `type`, ...), if any.
    fn container_word(&self) -> Option<&str> {
        self.stack.last().filter(|frame| frame.container.is_some()).map(|frame| frame.word.as_str())
    }

    /// Records `found` (unless a brace-only line continues `open`) and follows the braces of the masked line.
    fn finish(&mut self, number: u32, shown: &str, original: &str, mut open: Option<usize>, found: Option<(String, &'static str)>) {
        if open.is_none()
            && let Some((name, kind)) = found
        {
            let owner = self.stack.last().and_then(|frame| frame.container.as_deref());
            let name = match owner {
                Some(owner) => format!("{owner}::{name}"),
                None => name,
            };
            self.decls.push(Decl { name, kind, lines: [number, number], signature: signature_of(original) });
            open = Some(self.decls.len() - 1);
        }
        for byte in shown.bytes() {
            match byte {
                b'{' => {
                    let decl = open.take();
                    let container = decl.filter(|&at| self.nest && self.decls[at].kind == "type").map(|at| self.decls[at].name.clone());
                    let word = decl.filter(|_| container.is_some()).and_then(|at| ident_at(&self.decls[at].signature)).unwrap_or("").to_string();
                    let (container, word) = match self.stack.last() {
                        Some(parent) if self.inherit && decl.is_none() => (parent.container.clone(), parent.word.clone()),
                        _ => (container, word),
                    };
                    self.stack.push(Frame { container, decl, word });
                }
                b'}' => {
                    if let Some(frame) = self.stack.pop()
                        && let Some(at) = frame.decl
                    {
                        self.decls[at].lines[1] = number;
                    }
                }
                _ => {}
            }
        }
        // No brace yet and the line is not an expression or a statement end: the body may start on the next line.
        let line = shown.trim();
        self.pending = open.filter(|_| !line.ends_with(';') && !line.contains('='));
        self.inherit = false;
    }
}

/// Terraform/HCL blocks at column 0: `resource "t" "n"` is `t.n`, `data "t" "n"` is `data.t.n` (and also `t.n`), the rest are
/// `kind "n"`. The keys of a `locals` block are symbols of their own.
fn hcl(text: &str) -> Vec<Decl> {
    let masked = mask(text, &HCL);
    let mut walk = Walk::new(false);
    let mut locals = false;
    let mut depth = 0i32;
    for (number, shown, original) in numbered(&masked, text) {
        let line = shown.trim();
        if line.is_empty() {
            continue;
        }
        if walk.stack.is_empty() {
            locals = !shown.starts_with(char::is_whitespace) && ident_at(line) == Some("locals");
            depth = 0;
        }
        let open = walk.begin(line);
        let mut found = if open.is_none() { hcl_decl(shown, original) } else { None };
        if found.is_none() && open.is_none() && locals && walk.stack.len() == 1 && depth == 0 {
            found = members::locals_key(line);
        }
        // `dynamic "ingress" {` anywhere inside a block: `<block>::dynamic.ingress`.
        if found.is_none()
            && open.is_none()
            && let Some(top) = walk.stack.first().and_then(|frame| frame.decl)
            && let Some(label) = members::dynamic_label(line, original.trim_start())
        {
            found = Some((format!("{}::dynamic.{label}", walk.decls[top].name), "type"));
        }
        walk.finish(number, shown, original, open, found);
        depth = (depth + members::bracket_delta(shown)).max(0);
    }
    let mut decls = Vec::with_capacity(walk.decls.len());
    for decl in walk.decls {
        let alias = decl.name.strip_prefix("data.").filter(|_| decl.signature.starts_with("data ")).map(str::to_string);
        decls.push(decl.clone());
        if let Some(name) = alias {
            decls.push(Decl { name, ..decl });
        }
    }
    decls
}

/// Name and wire kind when the line (masked and original) opens a top-level HCL block.
fn hcl_decl(shown: &str, original: &str) -> Option<(String, &'static str)> {
    if shown.starts_with(char::is_whitespace) {
        return None;
    }
    let word = ident_at(shown)?;
    let labels = quoted_labels(original.get(word.len()..)?);
    let name = match (word, labels.as_slice()) {
        ("resource", [kind, name]) => format!("{kind}.{name}"),
        ("data", [kind, name]) => format!("data.{kind}.{name}"),
        ("variable" | "output" | "module" | "provider", [name]) => name.clone(),
        _ => return None,
    };
    (!is_denied(&name)).then_some((name, "type"))
}

/// The run of quoted labels at the start of `rest`. Empty when a label is empty or has an odd character.
fn quoted_labels(mut rest: &str) -> Vec<String> {
    let mut labels = Vec::new();
    while let Some(body) = rest.trim_start().strip_prefix('"') {
        let Some((label, tail)) = body.split_once('"') else {
            return Vec::new();
        };
        if labels.len() >= 3 || label.is_empty() || !label.chars().all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.')) {
            return Vec::new();
        }
        labels.push(label.to_string());
        rest = tail;
    }
    labels
}

/// Shell functions (`name() {`, `function name {`), bats `@test "name" {` blocks and, at file level, `alias` names and
/// `export`/`readonly`/`declare` variables.
/// Heredoc bodies are skipped; nesting is not tracked.
fn shell(text: &str) -> Vec<Decl> {
    let masked = mask(text, &SHELL);
    let mut walk = Walk::new(false);
    let mut heredoc: Option<String> = None;
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (number, shown, original) in numbered(&masked, text) {
        if let Some(tag) = &heredoc {
            if original.trim() == tag {
                heredoc = None;
            }
            continue;
        }
        let line = shown.trim();
        if line.is_empty() {
            continue;
        }
        let open = walk.begin(line);
        if open.is_none() && walk.stack.is_empty() {
            for (name, kind) in members::shell_vars(line) {
                if seen.insert(name.clone()) {
                    walk.decls.push(Decl { name, kind, lines: [number, number], signature: signature_of(original) });
                }
            }
        }
        let found = if open.is_none() { shell_decl(line).or_else(|| members::bats_test(line, original.trim_start())) } else { None };
        walk.finish(number, shown, original, open, found);
        heredoc = heredoc_tag(shown, original);
    }
    walk.decls
}

/// The delimiter word when the line starts a `<<WORD`, `<<-WORD` or `<<'WORD'` heredoc (not `<<<` and not a shift).
fn heredoc_tag(shown: &str, original: &str) -> Option<String> {
    let at = shown.find("<<")?;
    if shown[at + 2..].starts_with('<') {
        return None;
    }
    let rest = original.get(at + 2..)?.trim_start_matches('-').trim_start();
    let rest = rest.strip_prefix(['"', '\'']).unwrap_or(rest);
    let end = rest.find(|c: char| !(c.is_alphanumeric() || c == '_')).unwrap_or(rest.len());
    let word = &rest[..end];
    (word.starts_with(|c: char| c.is_alphabetic() || c == '_')).then(|| word.to_string())
}

/// Name when the masked, trimmed line declares a shell function.
fn shell_decl(line: &str) -> Option<(String, &'static str)> {
    let keyword = line.strip_prefix("function").filter(|rest| rest.starts_with(char::is_whitespace));
    let rest = keyword.map_or(line, str::trim_start);
    let end = rest.find(|c: char| !(c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | ':' | '@' | '+'))).unwrap_or(rest.len());
    let name = &rest[..end];
    if !name.starts_with(|c: char| c.is_alphabetic() || c == '_') || is_denied(name) {
        return None;
    }
    let tail = rest[end..].trim_start();
    let parens = tail.strip_prefix('(').is_some_and(|inner| inner.trim_start().starts_with(')'));
    let ok = if keyword.is_some() { tail.is_empty() || tail.starts_with('{') || parens } else { parens };
    ok.then(|| (name.to_string(), "fn"))
}

/// Protobuf `message`, `enum`, `service`, the `rpc` inside a service, and message fields and enum values, nested as `Parent::child`.
fn proto(text: &str) -> Vec<Decl> {
    let masked = mask(text, &PROTO);
    let mut walk = Walk::new(true);
    for (number, shown, original) in numbered(&masked, text) {
        let line = shown.trim();
        if line.is_empty() {
            continue;
        }
        let open = walk.begin(line);
        let found = if open.is_none() && walk.in_scope() {
            keyword_decl(line, &["message", "enum", "service"], &["rpc"]).or_else(|| members::proto_member(line, walk.container_word()))
        } else {
            None
        };
        walk.inherit = ident_at(line) == Some("oneof");
        walk.finish(number, shown, original, open, found);
    }
    walk.decls
}

/// GraphQL `type`, `input`, `enum`, `interface`, `union` and `scalar` at file level, and the fields (enum values) inside them.
fn graphql(text: &str) -> Vec<Decl> {
    let masked = mask(text, &GRAPHQL);
    let mut walk = Walk::new(true);
    let mut args = 0i32;
    for (number, shown, original) in numbered(&masked, text) {
        let line = shown.trim();
        if line.is_empty() {
            continue;
        }
        let open = walk.begin(line);
        let found = if open.is_some() {
            None
        } else if walk.stack.is_empty() {
            keyword_decl(line, &["type", "input", "enum", "interface", "union", "scalar"], &[])
        } else if args == 0 {
            members::graphql_member(line, walk.container_word())
        } else {
            None
        };
        walk.finish(number, shown, original, open, found);
        args = (args + members::bracket_delta(shown)).max(0);
    }
    walk.decls
}

/// `<keyword> Name` where the keyword is one of `types` (wire kind `type`) or `fns` (wire kind `fn`).
fn keyword_decl(line: &str, types: &[&str], fns: &[&str]) -> Option<(String, &'static str)> {
    let word = ident_at(line)?;
    let kind = if types.contains(&word) {
        "type"
    } else if fns.contains(&word) {
        "fn"
    } else {
        return None;
    };
    let rest = line[word.len()..].strip_prefix(char::is_whitespace)?;
    let name = ident_at(rest.trim_start()).filter(|name| !is_denied(name))?;
    Some((name.to_string(), kind))
}

/// Dockerfile named stages: `FROM image AS name`, and the `ARG` and `ENV` names (under their stage when it is named).
/// A stage runs until the next `FROM`; unnamed stages are not symbols.
fn docker(text: &str) -> Vec<Decl> {
    let masked = mask(text, &DOCKER);
    let mut decls: Vec<Decl> = Vec::new();
    let mut current: Option<usize> = None;
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut carry: Option<members::Carry> = None;
    let mut last = 0u32;
    for (number, shown, original) in numbered(&masked, text) {
        let mut words = shown.split_whitespace();
        let Some(first) = words.next() else {
            continue;
        };
        let (names, next) = members::docker_names(shown, carry.take());
        carry = next;
        for name in names {
            let name = match current {
                Some(at) => format!("{}::{name}", decls[at].name),
                None => name,
            };
            if seen.insert(name.clone()) {
                decls.push(Decl { name, kind: "var", lines: [number, number], signature: signature_of(original) });
            }
        }
        if first.eq_ignore_ascii_case("from") {
            if let Some(at) = current.take() {
                decls[at].lines[1] = last;
            }
            let name = words.skip_while(|word| !word.eq_ignore_ascii_case("as")).nth(1);
            if let Some(name) = name.filter(|name| !is_denied(name) && name.chars().all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.'))) {
                decls.push(Decl { name: name.to_string(), kind: "type", lines: [number, number], signature: signature_of(original) });
                current = Some(decls.len() - 1);
            }
        }
        last = number;
    }
    if let Some(at) = current {
        decls[at].lines[1] = last;
    }
    decls
}
