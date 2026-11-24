// SPDX-License-Identifier: Apache-2.0
//! Per-file facts in the dctx-map-parse shape, each carrying a precision label.
//! Implements PARSERS-v1 (completion, diagnostics) and SPEC-MAP-MODE 5.3 and 6.2.
//! Uses dctx-map-model nodes and edges; the walker builds these, callers consume them.
use dctx_map_model::{Edge, EdgeKind, Node, StableId};

/// Source dialect selected by the final path extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    /// `.js`, `.mjs`, `.cjs` and `.jsx`, with JSX.
    JavaScript,
    /// `.ts`, `.mts` and `.cts`, without JSX.
    TypeScript,
    /// `.tsx`: TypeScript with JSX.
    Tsx,
}

impl Dialect {
    /// Selects a dialect case-insensitively; other extensions are unsupported.
    /// This lexical helper does not validate path identities or enforce a size limit.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn for_path(path: &str) -> Option<Self> {
        let extension = path.rsplit_once('.')?.1.to_ascii_lowercase();
        match extension.as_str() {
            "js" | "mjs" | "cjs" | "jsx" => Some(Self::JavaScript),
            "ts" | "mts" | "cts" => Some(Self::TypeScript),
            "tsx" => Some(Self::Tsx),
            _ => None,
        }
    }

    /// Whether type annotations and declarations are recognised.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub const fn typed(self) -> bool {
        matches!(self, Self::TypeScript | Self::Tsx)
    }

    /// Whether `<` in expression position may open a JSX element.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub const fn jsx(self) -> bool {
        matches!(self, Self::JavaScript | Self::Tsx)
    }
}

/// How much syntactic evidence stands behind one fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Precision {
    /// A complete token shape inside balanced, terminated syntax.
    Structural,
    /// Recognised by convention: test blocks, `require` bindings, test edges.
    Convention,
    /// The fact reaches unterminated or unbalanced input; treat as a hint.
    Partial,
}

/// A fact together with its precision label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Labeled<T> {
    /// The extracted fact.
    pub fact: T,
    /// Evidence behind the fact.
    pub precision: Precision,
}

/// A syntactic reference whose destination has not been resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawReference {
    /// Source node, including its duplicate ordinal when needed.
    pub src: StableId,
    /// Relation to resolve; never `contains`.
    pub kind: EdgeKind,
    /// Referenced spelling (module path, receiver.method or type name).
    pub name: String,
    /// Lexical scope qualified with `::`; empty means the file root.
    pub scope: String,
    /// Explicit local binding (`import { a as b }`, `const b = require(..)`).
    pub alias: Option<String>,
    /// One-based inclusive lines of the reference site.
    pub lines: [u32; 2],
    /// File whose replacement invalidates this reference.
    pub owner_path: String,
}

/// A reference site keeps its binding space independently of relation and precision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceFact {
    /// Reference in the common map fact vocabulary.
    pub fact: RawReference,
    /// Syntactic or convention evidence for the site.
    pub precision: Precision,
    /// A type-space import contributes a dependency without a runtime binding.
    pub type_only: bool,
    /// A TypeScript import-equals declaration binds the module's assigned value.
    pub import_equals: bool,
}

impl FromIterator<Labeled<RawReference>> for Vec<ReferenceFact> {
    fn from_iter<I: IntoIterator<Item = Labeled<RawReference>>>(iter: I) -> Self {
        iter.into_iter()
            .map(|mut labeled| {
                let type_only = labeled.fact.kind == EdgeKind::Imports
                    && labeled.fact.name.starts_with("import type ");
                let import_equals = labeled.fact.kind == EdgeKind::Imports
                    && labeled.fact.name.starts_with("import = ");
                if type_only {
                    labeled.fact.name.drain(.."import type ".len());
                }
                if import_equals {
                    labeled.fact.name.drain(.."import = ".len());
                }
                ReferenceFact {
                    fact: labeled.fact,
                    precision: labeled.precision,
                    type_only,
                    import_equals,
                }
            })
            .collect()
    }
}

/// Why a span could not be extracted with full structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticReason {
    /// A block comment has no closing `*/`.
    UnterminatedComment,
    /// A quoted string reaches a line end or the end of input.
    UnterminatedString,
    /// A template literal has no closing backtick.
    UnterminatedTemplate,
    /// A JSX element is not closed before the end of input.
    UnterminatedJsx,
    /// A bracket has no partner, or closes a different bracket.
    UnbalancedBracket,
    /// A declaration name cannot form a stable id; the declaration is abstained.
    UnsupportedName,
}

/// One bounded diagnostic with its location.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Diagnostic {
    /// One-based line of the problem.
    pub line: u32,
    /// Byte offset of the problem.
    pub byte: u32,
    /// Problem category.
    pub reason: DiagnosticReason,
}

/// Completion of an accepted file; refusals are [`crate::LexError`] instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Completion {
    /// Every token was understood.
    Complete,
    /// Some spans were unsupported; see the diagnostics and fact labels.
    Partial,
}

/// Structural facts for one file, in source traversal order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexedFile {
    /// Validated, place-relative file path.
    pub path: String,
    /// Dialect used for extraction.
    pub dialect: Dialect,
    /// Whether all input was understood.
    pub completion: Completion,
    /// At most [`LexedFile::MAX_DIAGNOSTICS`] problems, in source order.
    pub diagnostics: Vec<Diagnostic>,
    /// Module, symbols and tests.
    pub nodes: Vec<Labeled<Node>>,
    /// Containment relations owned by this file.
    pub edges: Vec<Labeled<Edge>>,
    /// Unresolved references; repeated sites remain separate records.
    pub references: Vec<ReferenceFact>,
}

impl LexedFile {
    /// Bound on retained diagnostics; later problems still mark the file partial.
    pub const MAX_DIAGNOSTICS: usize = 64;
}

impl RawReference {
    /// Lossless module/member spelling for adapters that carry a single string.
    /// Ordinary paths keep their existing spelling. A byte count delimits an ambiguous operand.
    #[must_use]
    pub fn module_name(module: &str, member: Option<&str>) -> String {
        let explicit = module.starts_with("import type ")
            || module.starts_with("import = ")
            || module.starts_with('#')
            || module.contains("::")
            || module.ends_with(':')
            || member.is_some_and(|m| m.contains("::"));
        let mut name = if explicit {
            format!("#{}:{module}", module.len())
        } else {
            module.into()
        };
        if let Some(member) = member {
            name.push_str("::");
            name.push_str(member);
        }
        name
    }

    /// Recover a module operand and selected member without splitting literal punctuation.
    #[must_use]
    pub fn module_parts(name: &str) -> (&str, Option<&str>) {
        if let Some((length, tail)) = name.strip_prefix('#').and_then(|s| s.split_once(':'))
            && let Ok(len) = length.parse::<usize>()
            && let Some((module, member)) = tail.split_at_checked(len)
        {
            if member.is_empty() {
                return (module, None);
            }
            if let Some(member) = member.strip_prefix("::") {
                return (module, Some(member));
            }
        }
        name.rsplit_once("::")
            .map_or((name, None), |(path, member)| (path, Some(member)))
    }

    /// Cooks a quoted ECMAScript string operand. Malformed escapes supply no module evidence.
    #[must_use]
    pub fn string_literal(raw: &str) -> Option<String> {
        let quote = raw.chars().next()?;
        if !matches!(quote, '\'' | '"') {
            return None;
        }
        let body = raw.strip_prefix(quote)?.strip_suffix(quote)?;
        let mut chars = body.chars();
        let mut out = String::with_capacity(body.len());
        while let Some(c) = chars.next() {
            if c != '\\' {
                out.push(c);
                continue;
            }
            out.push(match chars.next()? {
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                'b' => '\x08',
                'f' => '\x0c',
                'v' => '\x0b',
                '0' if !chars.clone().next().is_some_and(|c| c.is_ascii_digit()) => '\0',
                '\n' | '\u{2028}' | '\u{2029}' => continue,
                '\r' => {
                    if chars.as_str().starts_with('\n') {
                        chars.next();
                    }
                    continue;
                }
                'x' => char::from_u32(hex(&mut chars, 2)?)?,
                'u' => {
                    let code = if chars.as_str().starts_with('{') {
                        chars.next();
                        let (digits, rest) = chars.as_str().split_once('}')?;
                        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
                            return None;
                        }
                        let code = u32::from_str_radix(digits, 16).ok()?;
                        chars = rest.chars();
                        code
                    } else {
                        let first = hex(&mut chars, 4)?;
                        if (0xd800..=0xdbff).contains(&first) {
                            chars = chars.as_str().strip_prefix("\\u")?.chars();
                            let low = hex(&mut chars, 4)?;
                            if !(0xdc00..=0xdfff).contains(&low) {
                                return None;
                            }
                            0x10000 + ((first - 0xd800) << 10) + low - 0xdc00
                        } else {
                            first
                        }
                    };
                    char::from_u32(code)?
                }
                c if c.is_ascii_digit() => return None,
                other => other,
            });
        }
        Some(out)
    }
}

fn hex(chars: &mut std::str::Chars<'_>, count: usize) -> Option<u32> {
    (0..count).try_fold(0, |value, _| Some(value * 16 + chars.next()?.to_digit(16)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialects_follow_the_final_extension() {
        assert_eq!(Dialect::for_path("a/b.MJS"), Some(Dialect::JavaScript));
        assert_eq!(Dialect::for_path("view.jsx"), Some(Dialect::JavaScript));
        assert_eq!(Dialect::for_path("types.d.ts"), Some(Dialect::TypeScript));
        assert_eq!(Dialect::for_path("view.tsx"), Some(Dialect::Tsx));
        assert_eq!(Dialect::for_path("notes.md"), None);
        assert_eq!(Dialect::for_path("Makefile"), None);
        assert!(Dialect::Tsx.typed() && Dialect::Tsx.jsx());
        assert!(!Dialect::TypeScript.jsx() && !Dialect::JavaScript.typed());
    }
}
