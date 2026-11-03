// SPDX-License-Identifier: Apache-2.0
//! Call references for the infra outline family: Terraform/HCL, SQL and shell.
//! Every reference is a guess made on masked text: a Terraform address (`var.x`, `module.x`, `TYPE.NAME`), a SQL
//! `name(` inside a view, function, procedure or trigger, a shell command word that names a function of the same file.
//! The caller's id is spelled as the index writes it (`s:<path>#<name>`, `~N` for the Nth copy of a name); refs.rs resolves.
//! Consumes authorized UTF-8 from extract; never reads source, never panics on malformed input.

use std::collections::{HashMap, HashSet};

use dctx_map_model::EdgeKind;

use super::{mask, Syntax};
use crate::index::extract::Symbol;
use crate::index::refs::RawRef;

/// HCL: comments only. Strings stay readable because `"${var.x}"` interpolations are real references.
const HCL: Syntax = Syntax { line: &["#", "//"], block: Some(("/*", "*/")), nested: false, quotes: b"", triple: false };
/// Shell: `#` comments, single and double quotes.
const SHELL: Syntax = Syntax { line: &["#"], block: None, nested: false, quotes: b"\"'", triple: false };
/// SQL: `--` and `/* */` comments, single-quoted strings.
const SQL: Syntax = Syntax { line: &["--"], block: Some(("/*", "*/")), nested: false, quotes: b"'", triple: false };

/// SQL words (lowercase) that may sit before a `(` without being a call: keywords, clauses, builtin types and common builtins.
const SQL_WORDS: &[&str] = &[
    "select", "from", "where", "values", "in", "exists", "count", "sum", "max", "min", "avg", "coalesce", "cast", "nullif", "int",
    "integer", "bigint", "smallint", "tinyint", "varchar", "nvarchar", "char", "nchar", "text", "numeric", "decimal", "number", "float",
    "double", "real", "timestamp", "timestamptz", "date", "time", "datetime", "boolean", "bool", "varbinary", "binary", "bit", "serial",
    "primary", "foreign", "key", "references", "unique", "check", "default", "not", "null", "and", "or", "as", "on", "using", "join",
    "left", "right", "inner", "outer", "cross", "group", "order", "by", "having", "limit", "offset", "union", "all", "distinct", "case",
    "when", "then", "else", "end", "over", "partition", "insert", "into", "update", "set", "delete", "create", "table", "view",
    "function", "procedure", "trigger", "returns", "return", "begin", "declare", "if", "elsif", "elseif", "loop", "while", "for", "each",
    "row", "execute", "index", "with", "recursive", "any", "some", "between", "like", "ilike", "is", "array", "returning", "conflict",
    "constraint", "generated", "identity", "lateral", "filter", "window", "interval", "extract", "lower", "upper", "length", "substring",
    "trim", "concat", "round", "abs", "ceil", "floor", "now", "raise", "perform", "language", "after", "before", "instead", "of", "to",
    "unnest", "row_number", "rank", "dense_rank", "lag", "lead", "first_value", "last_value", "greatest", "least", "replace", "exception",
    "when_others", "using_index", "tablespace", "include", "storage", "engine", "temporary", "temp", "type",
];

/// Raw call references of `text` for `hcl`, `sql` and `shell`; every other tag yields none.
pub(super) fn references(language: &str, path: &str, text: &str, symbols: &[Symbol]) -> Vec<RawRef> {
    let syntax = match language {
        "hcl" => &HCL,
        "sql" => &SQL,
        "shell" => &SHELL,
        _ => return Vec::new(),
    };
    let masked = mask(text, syntax);
    let functions: HashSet<&str> = symbols.iter().filter(|symbol| symbol.kind == "fn").map(|symbol| symbol.name.as_str()).collect();
    // SQL is case-insensitive: a call spelled `Help(` reaches the declared `help`, by full name or by last segment.
    let mut declared: HashMap<String, &str> = HashMap::new();
    if language == "sql" {
        for symbol in symbols {
            declared.entry(symbol.name.to_ascii_lowercase()).or_insert(&symbol.name);
            declared.entry(tail(&symbol.name).to_ascii_lowercase()).or_insert(&symbol.name);
        }
    }
    let mut seen: HashMap<&str, u32> = HashMap::new();
    let mut refs = Vec::new();
    for symbol in symbols {
        let copy = seen.entry(symbol.name.as_str()).or_insert(0);
        *copy += 1;
        let Some(body) = body_of(&masked, symbol, language) else { continue };
        let callees = match language {
            "hcl" => hcl_calls(body),
            "sql" => sql_calls(body),
            _ => shell_calls(body, &functions),
        };
        let src = id_text(path, &symbol.name, *copy);
        let mut done: HashSet<String> = HashSet::new();
        for callee in callees {
            let callee = declared.get(&callee.to_ascii_lowercase()).or_else(|| declared.get(&tail(&callee).to_ascii_lowercase())).map_or(callee, |name| (*name).to_string());
            let own = callee == symbol.name || (language == "sql" && tail(&callee).eq_ignore_ascii_case(tail(&symbol.name)));
            if own || !done.insert(callee.clone()) {
                continue;
            }
            refs.extend(RawRef::new(&src, EdgeKind::Calls, &callee, &symbol.name, None));
        }
    }
    refs
}

/// Stable id text of the `copy`-th symbol called `name` in `path`: `s:<path>#<name>` and `~N` from the second copy on.
fn id_text(path: &str, name: &str, copy: u32) -> String {
    let mut id = format!("s:{path}#{name}");
    if copy > 1 {
        id.push('~');
        id.push_str(&copy.to_string());
    }
    id
}

/// Last dotted segment of a SQL name.
fn tail(name: &str) -> &str {
    name.rsplit('.').next().unwrap_or(name)
}

/// The masked text a symbol scans. HCL and shell skip the declaration up to its first `{` (or its line), so a definition never
/// calls itself; SQL scans the whole statement when it is a view, function, procedure or trigger.
fn body_of<'a>(masked: &'a str, symbol: &Symbol, language: &str) -> Option<&'a str> {
    let start = usize::try_from(symbol.span[0]).ok()?;
    let end = usize::try_from(symbol.span[1]).ok()?;
    let span = masked.get(start..end)?;
    let first_end = span.find('\n').unwrap_or(span.len());
    if language == "sql" {
        return sql_has_body(&span[..first_end]).then_some(span);
    }
    let skip = span[..first_end].find('{').map_or(first_end, |brace| brace + 1);
    span.get(skip..)
}

/// True when a masked `CREATE` line declares a view, function, procedure or trigger.
fn sql_has_body(line: &str) -> bool {
    let mut words = line.split_whitespace();
    if !words.next().is_some_and(|word| word.eq_ignore_ascii_case("create")) {
        return false;
    }
    for word in words {
        let upper = word.split('(').next().unwrap_or(word).to_ascii_uppercase();
        match upper.as_str() {
            "VIEW" | "FUNCTION" | "PROCEDURE" | "TRIGGER" => return true,
            "TABLE" | "TYPE" | "INDEX" => return false,
            _ => {}
        }
    }
    false
}

/// True for a byte that continues a Terraform name part.
fn is_hcl_word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// Dotted parts (at most three) of the Terraform traversal that starts at `at`, and where it ends.
fn hcl_chain(body: &str, at: usize) -> (Vec<&str>, usize) {
    let bytes = body.as_bytes();
    let mut parts = Vec::new();
    let mut end = at;
    while parts.len() < 3 {
        let mut stop = end;
        while stop < bytes.len() && (is_hcl_word(bytes[stop]) || (bytes[stop] == b'-' && stop > end && bytes.get(stop + 1).is_some_and(|b| is_hcl_word(*b)))) {
            stop += 1;
        }
        if stop == end || !(bytes[end].is_ascii_alphabetic() || bytes[end] == b'_') {
            break;
        }
        parts.push(&body[end..stop]);
        end = stop;
        if bytes.get(end) == Some(&b'.') && bytes.get(end + 1).is_some_and(|b| b.is_ascii_alphabetic() || *b == b'_') {
            end += 1;
        } else {
            break;
        }
    }
    (parts, end)
}

/// Terraform references of a block body: `var.N`, `module.N`, `local.N` are `N`; `TYPE.NAME` and `data.TYPE.NAME` need an `_` in `TYPE`.
fn hcl_calls(body: &str) -> Vec<String> {
    let bytes = body.as_bytes();
    let mut out = Vec::new();
    let mut at = 0usize;
    while at < bytes.len() {
        let starts = (bytes[at].is_ascii_alphabetic() || bytes[at] == b'_') && (at == 0 || !(is_hcl_word(bytes[at - 1]) || matches!(bytes[at - 1], b'.' | b'-')));
        if !starts {
            at += 1;
            continue;
        }
        let (parts, end) = hcl_chain(body, at);
        at = end.max(at + 1);
        match parts.as_slice() {
            ["var" | "module" | "local", name, ..] => out.push((*name).to_string()),
            ["data", kind, name, ..] if kind.contains('_') => out.push(format!("data.{kind}.{name}")),
            [kind, name, ..] if kind.contains('_') => out.push(format!("{kind}.{name}")),
            _ => {}
        }
    }
    out
}

/// SQL calls of a masked statement: a dotted name directly followed by `(` that is not a keyword or builtin type.
fn sql_calls(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut from: Option<usize> = None;
    let flush = |lo: usize, hi: usize, out: &mut Vec<String>| {
        if !body[hi..].starts_with('(') {
            return;
        }
        let name: String = body[lo..hi].chars().filter(|c| !matches!(c, '"' | '`')).collect();
        let name = name.trim_matches('.');
        let word = tail(name).to_ascii_lowercase();
        if name.starts_with(|c: char| c.is_alphabetic() || c == '_') && !SQL_WORDS.contains(&word.as_str()) {
            out.push(name.to_string());
        }
    };
    for (index, c) in body.char_indices() {
        let word = c.is_alphanumeric() || matches!(c, '_' | '.' | '"' | '`');
        match (word, from) {
            (true, None) => from = Some(index),
            (false, Some(start)) => {
                flush(start, index, &mut out);
                from = None;
            }
            _ => {}
        }
    }
    if let Some(start) = from {
        flush(start, body.len(), &mut out);
    }
    out
}

/// True for a byte that can be part of a shell command word.
fn is_shell_word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':' | b'@' | b'+')
}

/// Command words of a function body that name a function of the file: at line start, after `;`, `&&`, `||`, `|`, `$(`,
/// `(`, `{`, a case branch `)` or a keyword such as `then`.
fn shell_calls(body: &str, functions: &HashSet<&str>) -> Vec<String> {
    let bytes = body.as_bytes();
    let mut out = Vec::new();
    let mut command = true;
    let mut at = 0usize;
    while at < bytes.len() {
        let byte = bytes[at];
        if matches!(byte, b'\n' | b';' | b'|' | b'&' | b'(' | b')' | b'{' | b'`') {
            command = true;
            at += 1;
        } else if matches!(byte, b' ' | b'\t' | b'\r' | b'!') {
            at += 1;
        } else if is_shell_word(byte) {
            let mut end = at;
            while end < bytes.len() && is_shell_word(bytes[end]) {
                end += 1;
            }
            let word = &body[at..end];
            let declares = body[end..].trim_start().strip_prefix('(').is_some_and(|rest| rest.trim_start().starts_with(')'));
            if command && !declares && functions.contains(word) {
                out.push(word.to_string());
            }
            command = matches!(word, "then" | "do" | "else" | "elif" | "if" | "while" | "until" | "time");
            at = end;
        } else {
            command = false;
            at += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `(src, name, scope)` of every reference `text` yields for `language`.
    fn calls(language: &str, path: &str, text: &str) -> Vec<(String, String, String)> {
        let symbols = super::super::symbols(language, text);
        references(language, path, text, &symbols).into_iter().map(|r| (r.src, r.name, r.scope)).collect()
    }

    /// One expected `(src, name, scope)` row.
    fn row(src: &str, name: &str, scope: &str) -> (String, String, String) {
        (src.to_string(), name.to_string(), scope.to_string())
    }

    /// A Terraform resource calls the variable, module and resource addresses it mentions, but not itself.
    #[test]
    fn terraform_addresses_are_calls() {
        let text = "variable \"region\" {\n  default = \"eu\"\n}\nresource \"aws_s3_bucket\" \"logs\" {\n  bucket = \"${var.region}-x\"\n}\nresource \"aws_s3_bucket_policy\" \"p\" {\n  bucket = aws_s3_bucket.logs.id\n  src = module.net.id\n  x = data.aws_ami.ubuntu.id\n  y = local.tags\n  # var.hidden\n  z = each.value\n}\n";
        assert_eq!(
            calls("hcl", "main.tf", text),
            [
                row("s:main.tf#aws_s3_bucket.logs", "region", "aws_s3_bucket.logs"),
                row("s:main.tf#aws_s3_bucket_policy.p", "aws_s3_bucket.logs", "aws_s3_bucket_policy.p"),
                row("s:main.tf#aws_s3_bucket_policy.p", "net", "aws_s3_bucket_policy.p"),
                row("s:main.tf#aws_s3_bucket_policy.p", "data.aws_ami.ubuntu", "aws_s3_bucket_policy.p"),
                row("s:main.tf#aws_s3_bucket_policy.p", "tags", "aws_s3_bucket_policy.p"),
            ]
        );
    }

    /// A SQL view calls a function; keywords, builtin types and the statement's own name are not calls.
    #[test]
    fn sql_view_and_function_bodies_call_by_name() {
        let text = "CREATE FUNCTION help(x int) RETURNS int AS $$ select x $$ LANGUAGE sql;\nCREATE VIEW v AS SELECT Help(1), count(*), cast(a as varchar(10)) FROM t WHERE a IN (1, 2) AND exists (select 1);\nCREATE TABLE t (a varchar(5), b numeric(3,2), c int default nextval(1));\nCREATE FUNCTION loopy(n int) RETURNS int AS $$ select loopy(n - 1) $$ LANGUAGE sql;\n-- CREATE VIEW c AS SELECT hidden(1);\nCREATE VIEW s AS SELECT 'quoted(1)' FROM t;\n";
        assert_eq!(calls("sql", "s.sql", text), [row("s:s.sql#v", "help", "v")]);
    }

    /// A shell function calls another function of the file at command positions only.
    #[test]
    fn shell_command_words_naming_functions_are_calls() {
        let text = "helper() {\n  echo hi\n}\nother() { :; }\nrun() {\n  helper\n  echo helper\n  x=$(other) && helper\n  if true; then other; fi\n  echo \"helper\" # helper\n  helper() { :; }\n}\nrecurse() {\n  recurse\n}\n";
        assert_eq!(calls("shell", "b.sh", text), [row("s:b.sh#run", "helper", "run"), row("s:b.sh#run", "other", "run")]);
    }

    /// The second copy of a name carries the `~2` ordinal the index gives it, and other tags have no references.
    #[test]
    fn duplicate_names_get_ordinals_and_other_tags_none() {
        let text = "f() {\n  :\n}\nf() {\n  g\n}\ng() {\n  :\n}\n";
        assert_eq!(calls("shell", "d.sh", text), [row("s:d.sh#f~2", "g", "f")]);
        assert!(calls("make", "Makefile", "a: b\n\t./x\n").is_empty());
    }
}
