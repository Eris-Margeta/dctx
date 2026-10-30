//! Brace-family declarations: Scala, Dart, Zig, Solidity, Gleam and Groovy.
//! A small keyword table per language picks `type` and `fn` declarations from lines the masked source shows at file level
//! or directly inside a type body. Nesting follows brace depth, so a function in a class is `Class::name`.
//! Heuristic by design: one declaration per line, bodies are never entered.

use super::{ident_at, is_denied, mask, signature_of, skip_modifiers, to_symbols, Decl, Syntax};
use crate::index::extract::Symbol;

/// How one language declares things.
struct Lang {
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
    syntax: Syntax { nested: true, triple: true, ..SLASHES },
    modifiers: &["private", "protected", "override", "final", "abstract", "sealed", "implicit", "lazy", "case", "open", "inline", "transparent", "opaque", "infix"],
    types: &["class", "object", "trait", "enum"],
    fns: &["def"],
    paren: false,
    typed: false,
    const_type: false,
};

const DART: Lang = Lang {
    syntax: Syntax { nested: true, triple: true, ..SLASHES },
    modifiers: &["abstract", "base", "final", "sealed", "interface", "static", "external", "const", "late"],
    types: &["class", "mixin", "enum"],
    fns: &[],
    paren: true,
    typed: true,
    const_type: false,
};

const ZIG: Lang = Lang {
    syntax: Syntax { line: &["//", "\\\\"], block: None, quotes: b"\"'", ..SLASHES },
    modifiers: &["pub", "export", "inline", "noinline", "extern", "comptime", "threadlocal"],
    types: &[],
    fns: &["fn"],
    paren: false,
    typed: false,
    const_type: true,
};

const SOLIDITY: Lang = Lang {
    syntax: SLASHES,
    modifiers: &["abstract"],
    types: &["contract", "interface", "library"],
    fns: &["function", "event", "modifier"],
    paren: false,
    typed: false,
    const_type: false,
};

const GLEAM: Lang = Lang {
    syntax: Syntax { line: &["//"], block: None, quotes: b"\"", ..SLASHES },
    modifiers: &["pub", "opaque"],
    types: &["type"],
    fns: &["fn"],
    paren: false,
    typed: false,
    const_type: false,
};

const GROOVY: Lang = Lang {
    syntax: Syntax { line: &["//", "#!"], triple: true, ..SLASHES },
    modifiers: &["public", "private", "protected", "static", "final", "abstract", "synchronized"],
    types: &["class", "interface", "trait", "enum"],
    fns: &["def"],
    paren: true,
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
        _ => None,
    }
}

/// Comment and string syntax of a tag, for the masking tests and the call scan.
pub(super) fn syntax_of(tag: &str) -> Option<Syntax> {
    lang(tag).map(|lang| lang.syntax)
}

/// True for the six brace-family tags.
pub(super) fn handles(language: &str) -> bool {
    lang(language).is_some()
}

/// One open brace: the type it belongs to (so members are visible) and the declaration it closes.
struct Frame {
    /// Qualified name of the type whose body this is.
    container: Option<String>,
    /// Index of the declaration whose end line this brace sets.
    decl: Option<usize>,
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
    let scala = tag == "scala";
    let masked = mask(text, &lang.syntax);
    let mut decls: Vec<Decl> = Vec::new();
    let mut stack: Vec<Frame> = Vec::new();
    // A declaration whose `{` has not appeared yet: it may open on a following line of its own.
    let mut pending: Option<usize> = None;
    // Scala: a declaration still reading its multi-line parameter list or its brace-less expression body.
    let mut cont: Option<Cont> = None;
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
        if cont.is_none() && !(line.starts_with('{') || (scala && continues_header(line))) {
            pending = None;
        }
        let mut open = pending.take();
        if let Some(c) = cont.as_ref().filter(|c| !c.body) {
            open = Some(c.decl);
        }
        let in_scope = stack.last().is_none_or(|frame| frame.container.is_some());
        if open.is_none()
            && cont.is_none()
            && in_scope
            && let Some((name, kind)) = declaration(lang, line)
        {
            let owner = stack.last().and_then(|frame| frame.container.as_deref());
            let name = match owner {
                Some(owner) => format!("{owner}::{name}"),
                None => name,
            };
            decls.push(Decl { name, kind, lines: [number, number], signature: signature_of(original) });
            open = Some(decls.len() - 1);
        }
        for byte in shown.bytes() {
            match byte {
                b'{' => {
                    let decl = open.take();
                    let container = decl.filter(|&at| decls[at].kind == "type").map(|at| decls[at].name.clone());
                    stack.push(Frame { container, decl });
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

/// True for a Scala line that goes on with the header above it (`extends A`, `with B`), so a `{` may still follow.
fn continues_header(line: &str) -> bool {
    ident_at(line).is_some_and(|word| matches!(word, "extends" | "with"))
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

/// Name and wire kind when `line` (masked, trimmed) declares something in `lang`.
fn declaration(lang: &Lang, line: &str) -> Option<(String, &'static str)> {
    let rest = skip_modifiers(line, lang.modifiers);
    if lang.typed
        && ident_at(rest).is_none_or(|word| !lang.types.contains(&word))
        && let Some(name) = typed_function(rest)
    {
        return Some((name, "fn"));
    }
    let word = ident_at(rest)?;
    let after = &rest[word.len()..];
    if lang.types.contains(&word) {
        return name_after(after, false).map(|name| (name, "type"));
    }
    if lang.fns.contains(&word) {
        return name_after(after, lang.paren).map(|name| (name, "fn"));
    }
    if lang.const_type && word == "const" {
        return const_type(after).map(|name| (name, "type"));
    }
    None
}

/// The name that follows a declaration keyword. `paren` requires `(` right after it.
fn name_after(after: &str, paren: bool) -> Option<String> {
    if !after.starts_with(char::is_whitespace) {
        return None;
    }
    let mut rest = after.trim_start();
    // `mixin class Name`: the second keyword is part of the declaration.
    if ident_at(rest) == Some("class") {
        rest = rest["class".len()..].trim_start();
    }
    let name = ident_at(rest).filter(|name| !is_denied(name))?;
    (!paren || rest[name.len()..].trim_start().starts_with('(')).then(|| name.to_string())
}

/// Zig `const Name = [packed|extern] struct|enum|union|opaque`.
fn const_type(after: &str) -> Option<String> {
    let rest = after.trim_start();
    let name = ident_at(rest).filter(|name| !is_denied(name))?;
    let mut value = rest[name.len()..].trim_start().strip_prefix('=')?.trim_start();
    while let Some(word) = ident_at(value).filter(|word| matches!(*word, "packed" | "extern")) {
        value = value[word.len()..].trim_start();
    }
    let kind = ident_at(value)?;
    matches!(kind, "struct" | "enum" | "union" | "opaque").then(|| name.to_string())
}

/// Dart `Type name(` or `Type<T> name<U>(`: a return type, a name and a parameter list.
fn typed_function(rest: &str) -> Option<String> {
    let first = ident_at(rest)?;
    if is_denied(first) {
        return None;
    }
    let mut after = &rest[first.len()..];
    after = after.strip_prefix('.').and_then(ident_at).map_or(after, |segment| &after[1 + segment.len()..]);
    if after.starts_with('<') {
        let close = balanced_angle(after)?;
        after = &after[close..];
    }
    after = after.strip_prefix('?').unwrap_or(after);
    if !after.starts_with(char::is_whitespace) {
        return None;
    }
    let rest = after.trim_start();
    let name = ident_at(rest).filter(|name| !is_denied(name))?;
    let tail = rest[name.len()..].trim_start();
    tail.starts_with(['(', '<']).then(|| name.to_string())
}

/// Offset just past the `>` that closes the `<` at the start of `text`, on one line.
fn balanced_angle(text: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (offset, c) in text.char_indices() {
        match c {
            '<' => depth += 1,
            '>' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(offset + 1);
                }
            }
            c if c.is_alphanumeric() || matches!(c, '_' | '$' | '?' | ',' | ' ' | '.') => {}
            _ => return None,
        }
    }
    None
}
