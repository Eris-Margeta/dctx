// SPDX-License-Identifier: Apache-2.0
//! Optional Rust syntax extractor. This file is compiled only with the `syntax-rust` feature.
//! Implements the enhanced parser profile on top of `dctx-lex-rust` (syn).
//! Depends on that crate's [`extract`](dctx_lex_rust::extract). The walk calls it for Rust files.
//! Other languages stay on the structural extractor. Coverage is `syntax` only when syn succeeded.

use std::collections::HashMap;

use dctx_lex_rust::{extract as lex_extract, Completion, Confidence, Coverage, DeclarationKind, Limits, Refusal, UnresolvedReason};
use dctx_map_model::{EdgeKind, NodeKind};

use super::extract::{signature_text, Symbol};
use super::refs::RawRef;
use super::write_stable_id;
use crate::domain::{DctxError, Resource};

/// Syn facts and raw call and import references for the Rust file at `path`. The bool is true when coverage is syntax.
pub(crate) fn extract(path: &str, bytes: &[u8], max_file: u64) -> Result<(Vec<Symbol>, Vec<RawRef>, bool), DctxError> {
    let max_bytes = usize::try_from(max_file).unwrap_or(usize::MAX);
    let limits = Limits::narrowed(max_bytes, 256, 10_000);
    let extracted = match lex_extract(bytes, &limits) {
        Ok(extracted) => extracted,
        Err(Refusal::TooLarge { limit, .. } | Refusal::TooManyRecords { limit }) => {
            return Err(DctxError::ResourceLimit { resource: Resource::FileBytes, limit: u64::try_from(limit).unwrap_or(u64::MAX) });
        }
        Err(Refusal::TooDeep { limit, .. }) => {
            return Err(DctxError::ResourceLimit { resource: Resource::Depth, limit: u64::from(limit) });
        }
        Err(Refusal::Worker) => return Err(DctxError::ResourceLimit { resource: Resource::Work, limit: 1 }),
        // A Rust file that is not UTF-8 has no symbols, like every other language in the structural extractor.
        // It stays a module in the graph; one such file never fails the whole walk.
        Err(Refusal::InvalidUtf8 { .. } | Refusal::Lex { .. } | Refusal::Syntax { .. }) => {
            return Ok((Vec::new(), Vec::new(), false));
        }
    };
    let syntax = extracted.coverage == Coverage::Syntax && matches!(extracted.completion, Completion::Complete);
    let mut symbols = Vec::new();
    // Stable id text of each kept declaration, in declaration order. Repeated names take `~2`, `~3` as the graph does.
    let mut owners: Vec<Option<String>> = Vec::with_capacity(extracted.declarations.len());
    let mut seen: HashMap<&str, u32> = HashMap::new();
    for declaration in &extracted.declarations {
        let Some(kind) = kind_of(declaration.kind) else {
            owners.push(None);
            continue;
        };
        let count = seen.entry(declaration.name.as_str()).or_insert(0);
        *count += 1;
        let mut id = String::new();
        write_stable_id(&mut id, b's', path, &declaration.name, if *count > 1 { *count } else { 0 });
        owners.push(Some(id));
        let start = declaration.span.start;
        let end = declaration.span.end;
        let signature = std::str::from_utf8(bytes)
            .ok()
            .filter(|text| start <= end && end <= text.len() && text.is_char_boundary(start) && text.is_char_boundary(end))
            .map(|text| signature_text(&text[start..end]))
            .unwrap_or_default();
        let (summary, summary_source) = match &declaration.doc {
            Some(doc) if !doc.is_empty() => (doc.clone(), "doc"),
            _ => (signature.clone(), "sig"),
        };
        symbols.push(Symbol {
            name: declaration.name.clone(),
            kind: kind.to_string(),
            graph: NodeKind::Symbol,
            lines: declaration.span.lines,
            span: [u64::try_from(start).unwrap_or(u64::MAX), u64::try_from(end).unwrap_or(u64::MAX)],
            summary,
            summary_source: summary_source.to_string(),
            signature,
        });
    }
    let mut module = String::new();
    write_stable_id(&mut module, b'm', path, "", 0);
    let mut refs = Vec::new();
    for import in &extracted.imports {
        refs.extend(RawRef::new(&module, EdgeKind::Imports, &import.path, &import.scope, import.alias.as_deref()));
    }
    for call in &extracted.references {
        if call.confidence == Confidence::Unresolved(UnresolvedReason::QualifiedSelf) {
            continue;
        }
        // A call inside a skipped declaration (a module) belongs to the nearest kept one, else to the file.
        let mut owner = call.from;
        let src = loop {
            let Some(index) = owner else { break &module };
            if let Some(Some(id)) = owners.get(index) {
                break id;
            }
            owner = extracted.declarations.get(index).and_then(|declaration| declaration.parent);
        };
        refs.extend(RawRef::new(src, EdgeKind::Calls, &call.target, &call.scope, None));
    }
    Ok((symbols, refs, syntax))
}

fn kind_of(kind: DeclarationKind) -> Option<&'static str> {
    Some(match kind {
        DeclarationKind::Function => "fn",
        DeclarationKind::Method => "method",
        DeclarationKind::Struct => "struct",
        DeclarationKind::Enum => "enum",
        DeclarationKind::Trait | DeclarationKind::TraitAlias => "trait",
        DeclarationKind::Impl => "impl",
        DeclarationKind::Const => "const",
        DeclarationKind::Static => "static",
        DeclarationKind::TypeAlias => "type",
        DeclarationKind::Macro => "macro",
        DeclarationKind::Module | DeclarationKind::Union => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const DECOYS: &[u8] = b"// fn Phantom() {}\n/* fn Blocked() {} */\nfn Real() {\n    let _text = \"fn Ghost() {}\";\n    let _raw = r#\"fn RawDecoy() {}\"#;\n}\n";

    #[test]
    fn bounded_input_refuses_before_a_parse() {
        let error = extract("a.rs", b"fn Real() {}\n", 4).expect_err("bound");
        match error {
            DctxError::ResourceLimit { resource: Resource::FileBytes, limit } => assert_eq!(limit, 4),
            other => panic!("file bound, got {other:?}"),
        }
    }

    #[test]
    fn rust_that_is_not_utf8_has_no_symbols_and_no_error() {
        let (symbols, refs, syntax) = extract("latin1.rs", b"pub fn caf\xe9() { tool(); }\n", 8 * 1024).expect("undecodable rust is an empty file, not an error");
        assert!(symbols.is_empty(), "{symbols:?}");
        assert!(refs.is_empty(), "{refs:?}");
        assert!(!syntax);
    }

    #[test]
    fn comment_and_string_decoys_are_not_declarations() {
        let (symbols, _, syntax) = extract("a.rs", DECOYS, 8 * 1024).expect("syn");
        assert!(syntax);
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "Real");
        assert_eq!(symbols[0].kind, "fn");
    }

    #[test]
    fn calls_and_imports_keep_their_owner_and_scope() {
        let source = b"use crate::util::tool;\npub fn eta() -> u32 { theta() }\nfn theta() -> u32 { 4 }\nmod inner { pub fn run() { super::theta(); } }\n";
        let (_, refs, syntax) = extract("r.rs", source, 8 * 1024).expect("syn");
        assert!(syntax);
        let import = refs.iter().find(|item| item.kind == EdgeKind::Imports).expect("import");
        assert_eq!((import.src.as_str(), import.name.as_str()), ("m:r.rs", "crate::util::tool"));
        let call = refs.iter().find(|item| item.name == "theta").expect("call");
        assert_eq!((call.src.as_str(), call.kind), ("s:r.rs#eta", EdgeKind::Calls));
        assert!(refs.iter().any(|item| item.src == "s:r.rs#inner::run" && item.name.ends_with("theta")), "{refs:?}");
    }
}
