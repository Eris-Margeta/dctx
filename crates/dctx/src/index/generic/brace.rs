// SPDX-License-Identifier: Apache-2.0
//! Brace-family declarations: Scala, Dart, Zig, Solidity, Gleam, Groovy, Perl and PowerShell.
//! A small keyword table per language picks `type` and `fn` declarations from lines the masked source shows at file level
//! or directly inside a type body; [`decl`] reads one line, [`data`] adds constants, variables, fields and enum values.
//! Nesting follows brace depth, so a function in a class is `Class::name`, an enum value `Enum::value`, and (Scala only) a
//! def inside a def `outer::inner`. Perl is read by its own module, [`perl`]: a `package` is a type and a `sub` a
//! function, with POD masked like a comment. Heuristic by design: one declaration per line, other bodies are never entered.

mod data;
mod decl;
mod perl;
#[cfg(test)]
mod tests;

use super::{ident_at, mask, signature_of, to_symbols, Decl, Syntax};
use crate::index::extract::Symbol;
use decl::{declarations, shape, Inside, Scope};

/// How one language declares things.
struct Lang {
    /// The language tag this table belongs to.
    tag: &'static str,
    /// Comment and string syntax.
    syntax: Syntax,
    /// Words that may precede a declaration keyword.
    modifiers: &'static [&'static str],
    /// Keywords that open a type: the next word is its name.
    types: &'static [&'static str],
    /// Keywords that open a function: the next word is its name.
    fns: &'static [&'static str],
    /// True when a function name must be followed by `(` (so `def x = 1` is not a function).
    paren: bool,
    /// True for `<type> name(` members (Dart).
    typed: bool,
    /// True for `const Name = struct|enum|union|opaque` types (Zig).
    const_type: bool,
}

const SLASHES: Syntax = Syntax { line: &["//"], block: Some(("/*", "*/")), nested: false, quotes: b"\"'", triple: false };

const SCALA: Lang = Lang {
    tag: "scala",
    syntax: Syntax { nested: true, triple: true, ..SLASHES },
    modifiers: &["private", "protected", "override", "final", "abstract", "sealed", "implicit", "lazy", "case", "open", "inline", "transparent", "opaque", "infix"],
    types: &["class", "object", "trait", "enum", "type"],
    fns: &["def"],
    paren: false,
    typed: false,
    const_type: false,
};

const DART: Lang = Lang {
    tag: "dart",
    syntax: Syntax { nested: true, triple: true, ..SLASHES },
    modifiers: &["abstract", "base", "final", "sealed", "interface", "static", "external", "const", "late", "factory"],
    types: &["class", "mixin", "enum", "extension"],
    fns: &[],
    paren: true,
    typed: true,
    const_type: false,
};

const ZIG: Lang = Lang {
    tag: "zig",
    syntax: Syntax { line: &["//", "\\\\"], block: None, quotes: b"\"'", ..SLASHES },
    modifiers: &["pub", "export", "inline", "noinline", "extern", "comptime", "threadlocal"],
    types: &[],
    fns: &["fn"],
    paren: false,
    typed: false,
    const_type: true,
};

const SOLIDITY: Lang = Lang {
    tag: "solidity",
    syntax: SLASHES,
    modifiers: &["abstract"],
    types: &["contract", "interface", "library", "struct", "enum", "type"],
    fns: &["function", "event", "modifier", "error"],
    paren: false,
    typed: false,
    const_type: false,
};

const GLEAM: Lang = Lang {
    tag: "gleam",
    syntax: Syntax { line: &["//"], block: None, quotes: b"\"", ..SLASHES },
    modifiers: &["pub", "opaque"],
    types: &["type"],
    fns: &["fn"],
    paren: false,
    typed: false,
    const_type: false,
};

const GROOVY: Lang = Lang {
    tag: "groovy",
    syntax: Syntax { line: &["//", "#!"], triple: true, ..SLASHES },
    modifiers: &["public", "private", "protected", "static", "final", "abstract", "synchronized", "transient", "volatile"],
    types: &["class", "interface", "trait", "enum"],
    fns: &["def"],
    paren: true,
    typed: false,
    const_type: false,
};

/// Perl is read by [`perl`] on its own (its `package`s are types and its `sub`s are functions); the table keeps the
/// keywords and the comment-and-string syntax so `syntax_of` and `handles` answer for it.
const PERL: Lang = Lang {
    tag: "perl",
    syntax: Syntax { line: &["#"], block: None, nested: false, quotes: b"\"'", triple: false },
    modifiers: &[],
    types: &["package"],
    fns: &["sub"],
    paren: false,
    typed: false,
    const_type: false,
};

const POWERSHELL: Lang = Lang {
    tag: "powershell",
    syntax: Syntax { line: &["#"], block: Some(("<#", "#>")), nested: false, quotes: b"\"'", triple: false },
    modifiers: &[],
    types: &["class", "enum"],
    fns: &["function", "filter"],
    paren: false,
    typed: false,
    const_type: false,
};

/// The table for a language tag.
fn lang(tag: &str) -> Option<&'static Lang> {
    match tag {
        "scala" => Some(&SCALA),
        "dart" => Some(&DART),
        "zig" => Some(&ZIG),
        "solidity" => Some(&SOLIDITY),
        "gleam" => Some(&GLEAM),
        "groovy" => Some(&GROOVY),
        "perl" => Some(&PERL),
        "powershell" => Some(&POWERSHELL),
        _ => None,
    }
}

/// Comment and string syntax of a tag, for the masking tests and the call scan.
pub(super) fn syntax_of(tag: &str) -> Option<Syntax> {
    lang(tag).map(|lang| lang.syntax)
}

/// Called names in one Perl function body; the scan and its `&name` and bareword rules live in [`perl::call_names`].
pub(super) fn perl_call_names<'a>(body: &'a str, own: &str, defined: &std::collections::HashSet<&str>) -> Vec<&'a str> {
    perl::call_names(body, own, defined)
}

/// True for the eight brace-family tags.
pub(super) fn handles(language: &str) -> bool {
    lang(language).is_some()
}

/// One open brace: the type it belongs to (so members are visible) and the declaration it closes.
struct Frame {
    /// Qualified name of the type whose body this is.
    container: Option<String>,
    /// Index of the declaration whose end line this brace sets.
    decl: Option<usize>,
    /// What kind of body this is, for the declarations directly inside it.
    inside: Inside,
}

/// A declaration header that runs over several lines because a parenthesis is still open (every language but Scala).
#[derive(Default)]
struct Parens {
    /// Parentheses and brackets opened and not yet closed.
    depth: usize,
    /// The declaration whose header this is, if the line that opened it declared one.
    decl: Option<usize>,
    /// Lines the header has lasted; a stray `(` must not hide the rest of the file.
    lines: u32,
}

/// A Scala declaration whose parameter list or expression body runs on past its first line (Scala only).
struct Cont {
    /// Index of the declaration.
    decl: usize,
    /// Indentation of the declaration line.
    indent: usize,
    /// Parentheses opened and not yet closed.
    depth: usize,
    /// Number of open braces at the declaration; braces opened by the body stand above it.
    base: usize,
    /// True once the `=` of a function was seen: what follows is its expression body, not a `{` that opens a block.
    body: bool,
}

/// Symbols of `text`, or none for an unknown tag.
pub(super) fn symbols(tag: &str, text: &str) -> Vec<Symbol> {
    let Some(lang) = lang(tag) else {
        return Vec::new();
    };
    if tag == PERL.tag {
        return perl::symbols(tag, text);
    }
    let scala = tag == "scala";
    let zig = tag == "zig";
    let solidity = tag == "solidity";
    let masked = mask(text, &lang.syntax);
    let mut decls: Vec<Decl> = Vec::new();
    let mut stack: Vec<Frame> = Vec::new();
    // A declaration whose `{` has not appeared yet: it may open on a following line of its own.
    let mut pending: Option<usize> = None;
    // Scala: a declaration still reading its multi-line parameter list or its brace-less expression body.
    let mut cont: Option<Cont> = None;
    // Other languages: a parameter list that runs over several lines.
    let mut parens = Parens::default();
    // Solidity: lines of a function header (modifiers, `returns`) read so far while waiting for its `{` or `;`.
    let mut header_lines = 0u32;
    for (index, (shown, original)) in masked.split('\n').zip(text.split('\n')).enumerate() {
        let number = u32::try_from(index + 1).unwrap_or(u32::MAX);
        let line = shown.trim();
        if line.is_empty() {
            continue;
        }
        let indent = shown.len() - shown.trim_start().len();
        // A brace-less body ends at the first line that is not indented past its `def` (and not inside open parens or braces).
        if cont.as_ref().is_some_and(|c| c.body && c.depth == 0 && stack.len() <= c.base && indent <= c.indent) {
            cont = None;
        }
        let in_parens = parens.depth > 0;
        let in_header = in_parens || (solidity && header_lines < 12 && !line.starts_with('{') && pending.is_some_and(|at| decls[at].kind == "fn"));
        header_lines = if in_header && !in_parens { header_lines + 1 } else { 0 };
        if cont.is_none() && !in_header && !(line.starts_with('{') || (scala && continues_header(line))) {
            pending = None;
        }
        let mut open = pending.take();
        if in_parens {
            open = parens.decl;
        }
        if let Some(c) = cont.as_ref().filter(|c| !c.body) {
            open = Some(c.decl);
        }
        if in_header && let Some(at) = open {
            decls[at].lines[1] = number;
        }
        let top = stack.last();
        let in_scope = top.is_none_or(|frame| frame.container.is_some());
        // Scala: a class header goes on over this line (an open parameter list, or a further `(..)` list after the name).
        let header_depth = match cont.as_ref() {
            Some(c) if !c.body => Some(c.depth),
            _ => (scala && open.is_some() && line.starts_with('(')).then_some(0),
        };
        let mut fresh = false;
        if open.is_none() && cont.is_none() && !in_header && in_scope {
            let scope = Scope { inside: top.map_or(Inside::File, |frame| frame.inside), owner: top.and_then(|frame| frame.container.as_deref()) };
            for (name, kind) in declarations(lang, line, original, &scope) {
                let name = match scope.owner {
                    Some(owner) => format!("{owner}::{name}"),
                    None => name,
                };
                decls.push(Decl { name, kind, lines: [number, number], signature: signature_of(original) });
                open = Some(decls.len() - 1);
                fresh = true;
            }
            // A type whose body is on its own line (`enum E { A, B }`) lists its members right here.
            // A Zig field typed by an anonymous `enum { a, b }` lists its values the same way.
            let inline = open.and_then(|at| match decls[at].kind {
                "type" => Some(shape(lang, line)),
                "field" if zig => decl::field_shape(line).filter(|inside| *inside == Inside::Enum),
                _ => None,
            });
            if let (Some(at), Some(inside)) = (open, inline) {
                let parent = decls[at].name.clone();
                for (member, kind) in data::inline_members(lang, line, inside) {
                    decls.push(Decl { name: format!("{parent}::{member}"), kind, lines: [number, number], signature: signature_of(original) });
                }
            }
        }
        // Scala: `val` and `var` parameters of a class header are members of the class.
        let params_depth = if fresh { Some(0) } else { header_depth };
        if scala
            && let Some(at) = open.filter(|&at| decls[at].kind == "type")
            && let Some(depth) = params_depth
        {
            let parent = decls[at].name.clone();
            for (member, kind) in decl::class_params(line, depth) {
                decls.push(Decl { name: format!("{parent}::{member}"), kind, lines: [number, number], signature: signature_of(original) });
            }
        }
        for byte in shown.bytes() {
            match byte {
                b'(' | b'[' if !scala => parens.depth += 1,
                b')' | b']' if !scala => parens.depth = parens.depth.saturating_sub(1),
                b'{' => {
                    let decl = if parens.depth == 0 { open.take() } else { None };
                    let (container, inside) = match decl {
                        Some(at) if decls[at].kind == "type" => (Some(decls[at].name.clone()), shape(lang, &decls[at].signature)),
                        // Scala defs and vals and Zig functions and tests: nested declarations are named under them.
                        Some(at) if (scala || zig) && decls[at].kind == "fn" => (Some(decls[at].name.clone()), Inside::Body),
                        Some(at) if scala && matches!(decls[at].kind, "const" | "var") => (Some(decls[at].name.clone()), Inside::Body),
                        Some(at) if zig && decls[at].kind == "field" => match decl::field_shape(&decls[at].signature) {
                            Some(inside) => (Some(decls[at].name.clone()), inside),
                            None => (None, Inside::File),
                        },
                        Some(_) => (None, Inside::File),
                        // A block inside a body stays in that body; a Zig `return struct {` gives the function its members.
                        None => {
                            let body = stack.last().filter(|frame| (scala || zig) && frame.inside == Inside::Body);
                            let returned = if zig { decl::returned_shape(line) } else { None };
                            match body {
                                Some(frame) => (frame.container.clone(), returned.unwrap_or(Inside::Body)),
                                None => (None, Inside::File),
                            }
                        }
                    };
                    stack.push(Frame { container, decl, inside });
                }
                b'}' => {
                    if let Some(frame) = stack.pop()
                        && let Some(at) = frame.decl
                    {
                        decls[at].lines[1] = number;
                    }
                }
                _ => {}
            }
        }
        if !scala {
            if parens.depth > 0 {
                parens.lines += 1;
                if parens.lines > 120 {
                    parens.depth = 0;
                }
                parens.decl = open;
            } else {
                parens.lines = 0;
                parens.decl = None;
            }
        }
        if scala {
            let (depth, assigned) = scan_parens(shown, cont.as_ref().map_or(0, |c| c.depth));
            if let Some(c) = cont.as_mut().filter(|c| c.body) {
                decls[c.decl].lines[1] = number;
                c.depth = depth;
            } else if let Some(at) = open {
                let body = assigned && decls[at].kind == "fn";
                decls[at].lines[1] = number;
                match cont.as_mut() {
                    Some(c) => {
                        c.depth = depth;
                        c.body |= body;
                    }
                    None => cont = Some(Cont { decl: at, indent, depth, base: stack.len(), body }),
                }
                // Header done and no expression body: back to the one-line rules below.
                if depth == 0 && !body {
                    cont = None;
                }
            } else {
                cont = None;
            }
            if cont.is_some() {
                continue;
            }
        }
        // No brace yet and the line is not an expression or a statement end: the body may start on the next line.
        pending = open.filter(|_| !line.ends_with(';') && !line.contains('='));
    }
    to_symbols(text, decls)
}

/// True for a Scala line that goes on with the header above it (`extends A`, `with B`, a further `(implicit ..)` parameter
/// list), so a `{` may still follow.
fn continues_header(line: &str) -> bool {
    line.starts_with('(') || ident_at(line).is_some_and(|word| matches!(word, "extends" | "with"))
}

/// Parenthesis depth at the end of the masked `line` when it starts at `depth`, and whether a plain `=` (not `==`, `=>`,
/// `<=`, `>=`, `!=`) stands outside every parenthesis: the assignment of a `def name(..) = body`.
fn scan_parens(line: &str, mut depth: usize) -> (usize, bool) {
    let bytes = line.as_bytes();
    let mut assigned = false;
    for (at, &byte) in bytes.iter().enumerate() {
        match byte {
            b'(' => depth += 1,
            b')' => depth = depth.saturating_sub(1),
            b'=' if depth == 0 && !assigned => {
                let before = at.checked_sub(1).map(|prior| bytes[prior]);
                assigned = !matches!(before, Some(b'=' | b'!' | b'<' | b'>')) && !matches!(bytes.get(at + 1), Some(b'=' | b'>' | b':'));
            }
            _ => {}
        }
    }
    (depth, assigned)
}
