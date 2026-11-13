// SPDX-License-Identifier: Apache-2.0
//! Hand-labelled declaration accuracy; no compiler or extractor generates labels.
#![forbid(unsafe_code)]

use dctx_lex_rust::{Completion, DeclarationKind, Limits, extract};
use proc_macro2::{TokenStream, TokenTree};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::io::Write as _;
use std::path::Path;
use syn::parse::{Parse, ParseStream};
use syn::{Ident, LitInt, LitStr, Token, braced, bracketed};

const KINDS: &[DeclarationKind] = &[
    DeclarationKind::Module,
    DeclarationKind::Struct,
    DeclarationKind::Union,
    DeclarationKind::Enum,
    DeclarationKind::Trait,
    DeclarationKind::TraitAlias,
    DeclarationKind::Impl,
    DeclarationKind::Function,
    DeclarationKind::Method,
    DeclarationKind::Const,
    DeclarationKind::Static,
    DeclarationKind::TypeAlias,
    DeclarationKind::Macro,
];

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Label {
    name: String,
    kind: String,
    line: u32,
    parent: Option<String>,
}

struct Labels(Vec<Label>);

// A strict reader for the fixture JSON schema, using the crate's existing parser
// dependency. Keys are ordered, strings use the common JSON/Rust escape subset.
// Rejects unknown keys, trailing tokens and non-null/non-string parents.
impl Parse for Labels {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let array;
        bracketed!(array in input);
        let mut labels = Vec::new();
        while !array.is_empty() {
            let object;
            braced!(object in array);
            let key = |expected: &str| -> syn::Result<()> {
                let actual: LitStr = object.parse()?;
                if actual.value() != expected {
                    return Err(syn::Error::new(actual.span(), "unexpected JSON key"));
                }
                object.parse::<Token![:]>()?;
                Ok(())
            };
            key("name")?;
            let name = object.parse::<LitStr>()?.value();
            object.parse::<Token![,]>()?;
            key("kind")?;
            let kind = object.parse::<LitStr>()?.value();
            object.parse::<Token![,]>()?;
            key("line")?;
            let line = object.parse::<LitInt>()?.base10_parse()?;
            object.parse::<Token![,]>()?;
            key("parent")?;
            let parent = if object.peek(LitStr) {
                Some(object.parse::<LitStr>()?.value())
            } else {
                let null: Ident = object.parse()?;
                if null != "null" {
                    return Err(syn::Error::new(null.span(), "expected null"));
                }
                None
            };
            if !object.is_empty() {
                return Err(object.error("unexpected JSON field"));
            }
            labels.push(Label {
                name,
                kind,
                line,
                parent,
            });
            if !array.is_empty() {
                array.parse::<Token![,]>()?;
            }
        }
        Ok(Self(labels))
    }
}

#[derive(Clone, Copy, Default)]
struct Counts {
    expected: u32,
    emitted: u32,
    correct: u32,
}

impl Counts {
    fn recall(self) -> f64 {
        ratio(self.correct, self.expected)
    }

    fn precision(self) -> f64 {
        ratio(self.correct, self.emitted)
    }
}

fn ratio(numerator: u32, denominator: u32) -> f64 {
    if denominator == 0 {
        1.0
    } else {
        f64::from(numerator) / f64::from(denominator)
    }
}

fn table(title: &str, counts: &BTreeMap<String, Counts>) -> String {
    let mut output =
        format!("\n{title}\nkind          expected emitted correct recall   precision\n");
    let mut total = Counts::default();
    for (kind, count) in counts {
        writeln!(
            output,
            "{kind:13} {:8} {:7} {:7} {:8.4} {:9.4}",
            count.expected,
            count.emitted,
            count.correct,
            count.recall(),
            count.precision()
        )
        .unwrap();
        total.expected += count.expected;
        total.emitted += count.emitted;
        total.correct += count.correct;
    }
    writeln!(
        output,
        "TOTAL         {:8} {:7} {:7} {:8.4} {:9.4}",
        total.expected,
        total.emitted,
        total.correct,
        total.recall(),
        total.precision()
    )
    .unwrap();
    output
}

fn baseline() -> BTreeMap<String, Counts> {
    include_str!("accuracy/before.tsv")
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .map(|line| {
            let fields: Vec<_> = line.split_whitespace().collect();
            assert_eq!(fields.len(), 4, "baseline schema");
            (
                fields[0].to_owned(),
                Counts {
                    expected: fields[1].parse().unwrap(),
                    emitted: fields[2].parse().unwrap(),
                    correct: fields[3].parse().unwrap(),
                },
            )
        })
        .collect()
}

// An independent token lexer identifies actual code-token starts. Literal bodies
// and comments cannot supply one, even on the recovery path. The corpus uses '@'
// syntax errors so every sample remains lexable. BOM is replaced at equal length.
fn code_starts(source: &str) -> BTreeSet<usize> {
    fn visit(tokens: TokenStream, starts: &mut BTreeSet<usize>) {
        for token in tokens {
            match token {
                TokenTree::Group(group) => visit(group.stream(), starts),
                TokenTree::Ident(ident) => {
                    starts.insert(ident.span().byte_range().start);
                }
                TokenTree::Punct(_) | TokenTree::Literal(_) => {}
            }
        }
    }
    let source = source
        .strip_prefix('\u{feff}')
        .map_or_else(|| source.to_owned(), |rest| format!("   {rest}"));
    let mut starts = BTreeSet::new();
    visit(
        source.parse().expect("corpus must remain lexable"),
        &mut starts,
    );
    starts
}

fn evaluate(path: &Path, counts: &mut BTreeMap<String, Counts>, differences: &mut String) {
    let source = std::fs::read_to_string(path).unwrap();
    let Labels(expected) =
        syn::parse_str(&std::fs::read_to_string(path.with_extension("expected.json")).unwrap())
            .expect("hand labels must match JSON schema");
    let facts = extract(source.as_bytes(), &Limits::default()).unwrap();
    let recovery = path
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .contains("_recovery_");
    assert_eq!(
        facts.completion == Completion::Complete,
        !recovery,
        "{}: {:?}",
        path.display(),
        facts.completion
    );
    let starts = code_starts(&source);
    let mut remaining: BTreeMap<Label, u32> = BTreeMap::new();
    for label in expected {
        assert!(label.line > 0 && label.line as usize <= source.lines().count());
        counts
            .get_mut(&label.kind)
            .expect("known declaration kind")
            .expected += 1;
        *remaining.entry(label).or_default() += 1;
    }
    for declaration in &facts.declarations {
        if !starts.contains(&declaration.span.start)
            || declaration.name.to_ascii_lowercase().contains("phantom")
        {
            writeln!(
                differences,
                "FORBIDDEN {}: symbol inside string/comment/attribute/macro: {:?}",
                path.file_name().unwrap().display(),
                declaration
            )
            .unwrap();
        }
        let label = Label {
            name: declaration.name.clone(),
            kind: format!("{:?}", declaration.kind),
            line: declaration.span.lines[0],
            parent: declaration
                .parent
                .map(|index| facts.declarations[index].name.clone()),
        };
        let count = counts.get_mut(&label.kind).unwrap();
        count.emitted += 1;
        if let Some(left) = remaining.get_mut(&label).filter(|left| **left > 0) {
            *left -= 1;
            count.correct += 1;
        } else {
            writeln!(
                differences,
                "{} extra {label:?}",
                path.file_name().unwrap().display()
            )
            .unwrap();
        }
    }
    for (label, left) in remaining {
        if left > 0 {
            writeln!(
                differences,
                "{} missing x{left} {label:?}",
                path.file_name().unwrap().display()
            )
            .unwrap();
        }
    }
}

#[test]
fn labelled_corpus_accuracy() {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/accuracy");
    let mut files: Vec<_> = std::fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "rs"))
        .collect();
    files.sort();
    assert!(files.len() >= 30);
    let mut counts: BTreeMap<_, _> = KINDS
        .iter()
        .map(|kind| (format!("{kind:?}"), Counts::default()))
        .collect();
    let mut differences = String::new();
    for path in &files {
        evaluate(path, &mut counts, &mut differences);
    }
    let before = baseline();
    let report = format!(
        "{}{}\nCorpus: {} source files\n{differences}",
        table(
            "BEFORE: frozen unmodified extractor measurement (54d0deff)",
            &before
        ),
        table("AFTER: current extractor measurement", &counts),
        files.len()
    );
    // Bypass libtest capture: the required plain `cargo test` gate prints both tables.
    std::io::stdout()
        .lock()
        .write_all(report.as_bytes())
        .unwrap();
    let expected: u32 = counts.values().map(|count| count.expected).sum();
    let emitted: u32 = counts.values().map(|count| count.emitted).sum();
    let correct: u32 = counts.values().map(|count| count.correct).sum();
    assert_eq!(
        before.len(),
        counts.len(),
        "baseline must include every kind"
    );
    for (kind, count) in &counts {
        assert_eq!(
            before[kind].expected, count.expected,
            "baseline corpus changed for {kind}"
        );
    }
    assert!(
        !differences.contains("FORBIDDEN"),
        "symbol inside string/comment/attribute/macro"
    );
    assert!(ratio(correct, expected) >= 0.97, "recall below 0.97");
    assert!(ratio(correct, emitted) >= 0.99, "precision below 0.99");
}
