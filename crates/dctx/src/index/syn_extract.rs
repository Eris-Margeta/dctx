//! Optional Rust syntax extractor. This file is compiled only with the `syntax-rust` feature.
//! Implements the enhanced parser profile on top of `dctx-lex-rust` (syn).
//! Depends on that crate's [`extract`](dctx_lex_rust::extract). The walk calls it for Rust files.
//! Other languages stay on the structural extractor. Coverage is `syntax` only when syn succeeded.

use dctx_lex_rust::{extract as lex_extract, Completion, Coverage, DeclarationKind, Limits, Refusal};
use dctx_map_model::NodeKind;

use super::extract::{signature_text, Symbol};
use crate::domain::{DctxError, Resource};

/// Syn facts for one Rust file. The bool is true when coverage is syntax.
pub(crate) fn extract(bytes: &[u8], max_file: u64) -> Result<(Vec<Symbol>, bool), DctxError> {
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
        Err(Refusal::InvalidUtf8 { .. } | Refusal::Lex { .. } | Refusal::Syntax { .. }) => {
            return Err(DctxError::invalid("rust source is not valid UTF-8"));
        }
    };
    let syntax = extracted.coverage == Coverage::Syntax && matches!(extracted.completion, Completion::Complete);
    let mut symbols = Vec::new();
    for declaration in &extracted.declarations {
        let Some(kind) = kind_of(declaration.kind) else {
            continue;
        };
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
    Ok((symbols, syntax))
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
