//! R declarations: `name <- function(` and `name = function(` outside any brace or parenthesis (fn), and the object-system calls
//! `setClass("Name")`, `setRefClass("Name")` and `R6Class("Name")` (type) and `setGeneric("name")` (fn), with or without `x <-` in front.

use super::super::{ident_at, is_denied, mask, signature_of, Decl, Syntax};

/// Comment and string syntax of R.
const SYNTAX: Syntax = Syntax { line: &["#"], block: None, nested: false, quotes: b"\"'", triple: false };

/// R declarations of `text`.
pub(super) fn decls(text: &str) -> Vec<Decl> {
    let masked = mask(text, &SYNTAX);
    let mut decls: Vec<Decl> = Vec::new();
    let (mut depth, mut parens) = (0usize, 0usize);
    // The declaration whose `{` has not appeared yet, the one whose braces are open, and the class or generic call whose parentheses are open.
    let (mut waiting, mut body): (Option<usize>, Option<usize>) = (None, None);
    let mut call: Option<usize> = None;
    let mut recheck = false;
    for (index, (shown, original)) in masked.split('\n').zip(text.split('\n')).enumerate() {
        let number = u32::try_from(index + 1).unwrap_or(u32::MAX);
        let line = shown.trim();
        if line.is_empty() {
            continue;
        }
        // A body that does not start on the line after the header, with the parameters closed, belongs to no one.
        if recheck && !line.starts_with('{') {
            waiting = None;
        }
        if depth == 0 && parens == 0 && waiting.is_none() {
            if let Some(name) = function_name(line) {
                decls.push(Decl { name, kind: "fn", lines: [number, number], signature: signature_of(original) });
                waiting = Some(decls.len() - 1);
            } else if let Some((name, kind)) = call_name(shown, original) {
                decls.push(Decl { name, kind, lines: [number, number], signature: signature_of(original) });
                call = Some(decls.len() - 1);
            }
        }
        for byte in shown.bytes() {
            match byte {
                b'{' => {
                    depth += 1;
                    if depth == 1 {
                        body = waiting.take();
                    }
                }
                b'}' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0
                        && let Some(at) = body.take()
                    {
                        decls[at].lines[1] = number;
                    }
                }
                b'(' => parens += 1,
                b')' => parens = parens.saturating_sub(1),
                _ => {}
            }
        }
        if let Some(at) = call {
            decls[at].lines[1] = number;
            if parens == 0 && depth == 0 {
                call = None;
            }
        }
        recheck = waiting.is_some() && parens == 0;
    }
    decls
}

/// The name when `line` starts `name <- function(`, `name <<- function(` or `name = function(`.
fn function_name(line: &str) -> Option<String> {
    let (name, value) = assignment(line)?;
    let is_function = ident_at(value) == Some("function") && value["function".len()..].trim_start().starts_with('(');
    is_function.then(|| name.to_string())
}

/// The name and the value of `line` when it starts `name <- value`, `name <<- value` or `name = value`.
fn assignment(line: &str) -> Option<(&str, &str)> {
    let end = line.find(|c: char| !(c.is_alphanumeric() || c == '.' || c == '_')).unwrap_or(line.len());
    let name = &line[..end];
    if !name.starts_with(|c: char| c.is_alphabetic() || c == '.') || is_denied(name) {
        return None;
    }
    let rest = line[end..].trim_start();
    let value = rest.strip_prefix("<<-").or_else(|| rest.strip_prefix("<-")).or_else(|| rest.strip_prefix('=').filter(|after| !after.starts_with('=')))?;
    Some((name, value.trim_start()))
}

/// Name and wire kind of a `setClass("Person", ...)`, `setGeneric("greet", ...)`, `setRefClass("Account", ...)` or `R6::R6Class("Account", ...)` call,
/// optionally behind `x <-`. The name is the first argument; a class call without one is named after what it is assigned to.
/// `shown` is the masked line (strings blank), `original` the same line unmasked, so offsets agree.
fn call_name(shown: &str, original: &str) -> Option<(String, &'static str)> {
    let lead = shown.len() - shown.trim_start().len();
    let line = &shown[lead..];
    let source = original.get(lead..)?;
    let (assigned, head) = match assignment(line) {
        Some((name, value)) => (Some(name), value),
        None => (None, line),
    };
    let end = head.find(|c: char| !(c.is_alphanumeric() || matches!(c, '.' | '_' | ':'))).unwrap_or(head.len());
    let kind = match head[..end].rsplit("::").next()? {
        "setClass" | "setRefClass" | "R6Class" => "type",
        "setGeneric" => "fn",
        _ => return None,
    };
    let arguments = head[end..].trim_start().strip_prefix('(')?;
    let literal = quoted_name(source.get(line.len() - arguments.len()..)?);
    let name = literal.or_else(|| assigned.filter(|_| kind == "type").map(str::to_string))?;
    Some((name, kind))
}

/// The string that opens an argument list, with or without a `Class =` label in front of it, when it can name something.
fn quoted_name(arguments: &str) -> Option<String> {
    let mut arguments = arguments.trim_start();
    if let Some(label) = ident_at(arguments)
        && let Some(value) = arguments[label.len()..].trim_start().strip_prefix('=').filter(|after| !after.starts_with('='))
    {
        arguments = value.trim_start();
    }
    let quote = arguments.chars().next().filter(|c| matches!(c, '"' | '\''))?;
    let body = &arguments[1..];
    let name = &body[..body.find(quote)?];
    let valid = name.starts_with(|c: char| c.is_alphabetic() || c == '.') && name.chars().all(|c| c.is_alphanumeric() || matches!(c, '.' | '_'));
    (valid && !is_denied(name)).then(|| name.to_string())
}
