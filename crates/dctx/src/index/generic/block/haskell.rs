//! Haskell declarations: column-0 `name ::` signatures, `data`, `newtype`, `type`, `class` and `instance`. A class and an instance own the
//! indented lines that start at the column of their first member: signatures for a class, equations for an instance. Record fields of a
//! `data` or `newtype` are read from its braces. A declaration runs to the last line indented under it; bodies are never entered.
//! Names inside a type read `Type::member`; an instance is `Class.Type`, so its methods stay apart from the class's own signatures.

use super::super::{is_denied, mask, signature_of, Decl, Syntax};
use super::layout::{ident_run, indent_of, is_ident_char};

/// Haskell: `--` comments, nested `{- -}` comments and double-quoted strings.
const SYNTAX: Syntax = Syntax { line: &["--"], block: Some(("{-", "-}")), nested: true, quotes: b"\"", triple: false };

/// Words that start a line of a `where` body without naming a function.
const KEYWORDS: &[&str] = &[
    "where", "let", "in", "if", "then", "else", "case", "of", "do", "deriving", "module", "import", "type", "data", "newtype", "class",
    "instance", "infix", "infixl", "infixr", "default", "foreign",
];

/// The `where` body of a class or an instance being read: its declaration, the column of its members and the last member.
struct Body {
    /// Index of the class or instance declaration.
    owner: usize,
    /// Column of the first member, once one was seen.
    child: Option<usize>,
    /// Index of the latest member, which deeper lines continue.
    last: Option<usize>,
    /// True for an instance, whose members are equations rather than signatures.
    instance: bool,
}

/// The braces of a `data` or `newtype` being read for record fields, carried from line to line.
struct Record {
    /// Index of the type declaration.
    owner: usize,
    /// Open `{` depth.
    braces: usize,
    /// Open `(` and `[` inside the outermost braces.
    parens: usize,
    /// The text of the field group read so far, up to the next top-level comma.
    segment: String,
    /// Names from a group such as `x, y :: Int` whose type has not appeared yet.
    pending: Vec<String>,
    /// Fields already found, so a name shared by two constructors counts once.
    seen: Vec<String>,
}

impl Record {
    /// A reader for the type declared at `owner`.
    fn new(owner: usize) -> Self {
        Self { owner, braces: 0, parens: 0, segment: String::new(), pending: Vec::new(), seen: Vec::new() }
    }

    /// Field names that the masked, trimmed `line` completes.
    fn feed(&mut self, line: &str) -> Vec<String> {
        let mut fields = Vec::new();
        for c in line.chars().chain(std::iter::once(' ')) {
            match c {
                '{' => {
                    self.braces += 1;
                    if self.braces > 1 {
                        self.segment.push(c);
                    }
                }
                '}' if self.braces > 0 => {
                    if self.braces == 1 {
                        self.flush(&mut fields);
                    } else {
                        self.segment.push(c);
                    }
                    self.braces -= 1;
                }
                '(' | '[' if self.braces > 0 => {
                    self.parens += 1;
                    self.segment.push(c);
                }
                ')' | ']' if self.braces > 0 => {
                    self.parens = self.parens.saturating_sub(1);
                    self.segment.push(c);
                }
                ',' if self.braces == 1 && self.parens == 0 => self.flush(&mut fields),
                _ if self.braces > 0 => self.segment.push(c),
                _ => {}
            }
        }
        fields
    }

    /// Reads the finished field group: `name :: Type` names a field, a bare name waits for the type of the group (`x, y :: Int`).
    fn flush(&mut self, fields: &mut Vec<String>) {
        let segment = std::mem::take(&mut self.segment);
        let (names, typed) = match segment.find("::") {
            Some(at) => (&segment[..at], true),
            None => (segment.as_str(), false),
        };
        let name = names.trim();
        let valid = ident_run(name) == name && name.starts_with(|c: char| c.is_lowercase() || c == '_') && !is_denied(name);
        if !valid {
            if typed {
                self.pending.clear();
            }
            return;
        }
        if !typed {
            self.pending.push(name.to_string());
            return;
        }
        for field in self.pending.drain(..).chain(std::iter::once(name.to_string())) {
            if !self.seen.contains(&field) {
                self.seen.push(field.clone());
                fields.push(field);
            }
        }
    }
}

/// Declarations of a Haskell module.
pub(super) fn haskell(text: &str) -> Vec<Decl> {
    let masked = mask(text, &SYNTAX);
    let mut decls: Vec<Decl> = Vec::new();
    // The column-0 declarations the indented lines below belong to, the class or instance body, and the record being read.
    let mut group: Vec<usize> = Vec::new();
    let mut body: Option<Body> = None;
    let mut record: Option<Record> = None;
    for (index, (shown, original)) in masked.split('\n').zip(text.split('\n')).enumerate() {
        let number = u32::try_from(index + 1).unwrap_or(u32::MAX);
        let line = shown.trim();
        if line.is_empty() {
            continue;
        }
        let indent = indent_of(shown);
        if indent == 0 {
            body = None;
            record = None;
            let found = haskell_top(line);
            if found.is_empty() {
                // Further equations of the function named by the signature extend it.
                let word = ident_run(line);
                if !group.iter().any(|&at| decls[at].kind == "fn" && decls[at].name == word) {
                    group.clear();
                }
            } else {
                group.clear();
                for (name, kind) in found {
                    decls.push(Decl { name, kind, lines: [number, number], signature: signature_of(original) });
                    group.push(decls.len() - 1);
                }
                let owner = decls.len() - 1;
                match ident_run(line) {
                    "class" => body = Some(Body { owner, child: None, last: None, instance: false }),
                    "instance" => body = Some(Body { owner, child: None, last: None, instance: true }),
                    "data" | "newtype" => record = Some(Record::new(owner)),
                    _ => {}
                }
            }
        } else if let Some(open) = body.as_mut() {
            match open.child {
                Some(column) if indent > column => {
                    if let Some(at) = open.last {
                        decls[at].lines[1] = number;
                    }
                }
                Some(column) if indent < column => {}
                _ => {
                    let members: Vec<(String, &'static str)> = if open.instance { equation_name(line).map(|name| (name, "fn")).into_iter().collect() } else { signature_names(line) };
                    if !members.is_empty() {
                        open.child = Some(indent);
                    }
                    for (name, kind) in members {
                        let name = format!("{}::{name}", decls[open.owner].name);
                        // Consecutive equations of one method are one symbol.
                        if open.instance && decls.last().is_some_and(|last| last.name == name) {
                            let last = decls.len() - 1;
                            decls[last].lines[1] = number;
                        } else {
                            decls.push(Decl { name, kind, lines: [number, number], signature: signature_of(original) });
                        }
                        open.last = Some(decls.len() - 1);
                    }
                }
            }
        }
        if let Some(reader) = record.as_mut() {
            let owner = reader.owner;
            for field in reader.feed(line) {
                let name = format!("{}::{field}", decls[owner].name);
                decls.push(Decl { name, kind: "field", lines: [number, number], signature: signature_of(original) });
            }
        }
        for &at in &group {
            decls[at].lines[1] = number;
        }
    }
    decls
}

/// Declarations that the column-0 `line` of a Haskell module starts.
fn haskell_top(line: &str) -> Vec<(String, &'static str)> {
    let word = ident_run(line);
    let rest = &line[word.len()..];
    match word {
        "data" | "newtype" | "class" | "type" if rest.starts_with(char::is_whitespace) => {
            let mut rest = rest.trim_start();
            if let Some(after) = rest.strip_prefix("family").filter(|after| after.starts_with(char::is_whitespace)) {
                rest = after.trim_start();
            }
            if rest.starts_with("instance") {
                return Vec::new();
            }
            // A context `Eq a =>` comes before the name. After a `data` constructor's `=` it is the constructor's own.
            let head = rest.split(" where").next().unwrap_or(rest);
            let head = match head.find("=>") {
                Some(arrow) if word == "class" || head.find('=') == Some(arrow) => head[arrow + 2..].trim_start(),
                _ => head,
            };
            let name = ident_run(head);
            if name.starts_with(char::is_uppercase) { vec![(name.to_string(), "type")] } else { Vec::new() }
        }
        "instance" if rest.starts_with(char::is_whitespace) => instance_name(rest).map(|name| vec![(name, "type")]).unwrap_or_default(),
        _ => signature_names(line),
    }
}

/// `Class.Type` for the rest of an `instance Context => Class (Type a) where` line, or `Class` when no type name follows it.
fn instance_name(rest: &str) -> Option<String> {
    let head = rest.split(" where").next().unwrap_or(rest);
    let head = head.rfind("=>").map_or(head, |arrow| &head[arrow + 2..]);
    let mut words = head.split(|c: char| !(is_ident_char(c) || c == '.')).filter(|word| !word.is_empty());
    let class = words.next().filter(|class| class.starts_with(char::is_uppercase))?;
    Some(match words.find(|word| word.starts_with(char::is_uppercase)) {
        Some(target) => format!("{class}.{target}"),
        None => class.to_string(),
    })
}

/// The function an equation line of an instance body defines: its first word, unless an operator or a keyword takes that place.
fn equation_name(line: &str) -> Option<String> {
    let name = ident_run(line);
    if !name.starts_with(|c: char| c.is_lowercase() || c == '_') || is_denied(name) || KEYWORDS.contains(&name) {
        return None;
    }
    let tail = line[name.len()..].trim_start();
    let symbolic = tail.starts_with(|c: char| c.is_ascii_punctuation() && !matches!(c, '(' | '[' | '{' | '_' | '\'' | '"' | '|' | '=' | '~' | '@' | ':'));
    let operator = symbolic || tail.starts_with("==") || (tail.starts_with(':') && !tail.starts_with("::"));
    (!operator).then(|| name.to_string())
}

/// Names of a type signature line `a, b :: Type`, or none when the line is anything else.
fn signature_names(line: &str) -> Vec<(String, &'static str)> {
    let Some(split) = line.find("::") else {
        return Vec::new();
    };
    if line[split..].starts_with(":::") {
        return Vec::new();
    }
    let mut names = Vec::new();
    for name in line[..split].split(',').map(str::trim) {
        let valid = ident_run(name) == name && name.starts_with(|c: char| c.is_lowercase() || c == '_') && !is_denied(name);
        if !valid {
            return Vec::new();
        }
        names.push((name.to_string(), "fn"));
    }
    names
}
