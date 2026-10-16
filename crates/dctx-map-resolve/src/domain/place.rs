//! Turns retained fact payloads into package-layout facts.
//! Implements DX-B5: the same bytes can resolve differently per placement.
//! Depends on shared symbols and the pure resolver. Views call this before resolve.
use super::{
    Definition, FactKey, Facts, Placement, Reference, ResolveError, SharedFact, shared::join,
};
use dctx_map_model::{Node, NodeKind, StableId, Summary, SummarySource};
use std::collections::{BTreeMap, BTreeSet};

/// Builds resolver facts for one layout. The same payload can be placed twice.
pub fn materialize(
    retained: &BTreeMap<FactKey, SharedFact>,
    placements: &[Placement],
) -> Result<Vec<Facts>, ResolveError> {
    let mut seen = BTreeSet::new();
    for placement in placements {
        placement.check()?;
        if !seen.insert(placement.path.as_str()) {
            return Err(ResolveError::DuplicateFile(placement.path.clone()));
        }
    }
    let mut ordered: Vec<&Placement> = placements.iter().collect();
    ordered.sort_by(|left, right| left.path.cmp(&right.path));
    let mut files = Vec::with_capacity(ordered.len());
    for placement in ordered {
        let shared = retained
            .get(&placement.key)
            .ok_or_else(|| ResolveError::MissingFact(placement.path.clone()))?;
        files.push(place(shared, placement)?);
    }
    Ok(files)
}

fn place(shared: &SharedFact, placement: &Placement) -> Result<Facts, ResolveError> {
    let ordinals = ordinals(&shared.symbols);
    let mut ids = BTreeMap::new();
    let mut definitions = Vec::with_capacity(shared.symbols.len());
    for (index, symbol) in shared.symbols.iter().enumerate() {
        let id = symbol_id(&placement.path, symbol, ordinals[index])?;
        ids.insert(symbol.local_name.clone(), id.clone());
        definitions.push(Definition {
            qualified_name: join(&placement.module, &symbol.local_name),
            node: Node {
                id,
                kind: symbol.kind,
                symbol_kind: symbol.symbol_kind,
                path: placement.path.clone(),
                lines: symbol.lines,
                name: symbol.display_name.clone(),
                signature: symbol.signature.clone(),
                fingerprint: symbol.fingerprint.clone(),
                public: symbol.public,
                summary: Summary {
                    text: symbol.display_name.clone(),
                    source: SummarySource::Signature,
                },
            },
        });
    }
    definitions.sort_by(|left, right| {
        left.qualified_name
            .cmp(&right.qualified_name)
            .then(left.node.id.cmp(&right.node.id))
    });
    let mut references = Vec::with_capacity(shared.references.len());
    for reference in &shared.references {
        let source = ids
            .get(&reference.source_local)
            .ok_or_else(|| ResolveError::MissingSymbol(reference.source_local.clone()))?
            .clone();
        references.push(Reference {
            source,
            scope: join(&placement.module, &reference.scope),
            kind: reference.kind,
            target: reference.target.clone(),
            binding: reference.binding.clone(),
        });
    }
    references.sort();
    Ok(Facts {
        path: placement.path.clone(),
        package: placement.package.clone(),
        definitions,
        references,
    })
}

fn ordinals(symbols: &[super::SharedSymbol]) -> Vec<Option<u32>> {
    let mut groups: BTreeMap<(NodeKind, &str), Vec<usize>> = BTreeMap::new();
    for (index, symbol) in symbols.iter().enumerate() {
        if symbol.kind != NodeKind::Module {
            groups
                .entry((symbol.kind, symbol.display_name.as_str()))
                .or_default()
                .push(index);
        }
    }
    let mut assigned = vec![None; symbols.len()];
    for indexes in groups.values_mut() {
        indexes.sort_by(|&left, &right| {
            symbols[left]
                .local_name
                .cmp(&symbols[right].local_name)
                .then(symbols[left].fingerprint.cmp(&symbols[right].fingerprint))
        });
        if indexes.len() > 1 {
            for (number, index) in indexes.iter().enumerate() {
                assigned[*index] = Some(number as u32 + 1);
            }
        }
    }
    assigned
}

fn symbol_id(
    path: &str,
    symbol: &super::SharedSymbol,
    ordinal: Option<u32>,
) -> Result<StableId, ResolveError> {
    let name = match symbol.kind {
        NodeKind::Module => None,
        _ => Some(symbol.display_name.as_str()),
    };
    StableId::new(symbol.kind, path, name, ordinal).map_err(|_| ResolveError::InvalidPath(path.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FactKey, Placement, SharedFact, SharedReference, SharedSymbol, Target};
    use crate::domain::ExtractionContract;
    use crate::resolve;
    use dctx_map_model::{ContentId, EdgeKind, NodeKind, SymbolKind};
    use std::collections::BTreeMap;

    fn key(bytes: &[u8]) -> FactKey {
        FactKey::new(
            ContentId::of_bytes(bytes).unwrap(),
            ExtractionContract {
                language: 1,
                dialect: "rust-2024".into(),
                parser_id: "dctx-map-parse".into(),
                parser_revision: "p1".into(),
                extraction_digest: "plan-1".into(),
                fact_schema: 1,
            },
        )
        .unwrap()
    }

    fn symbol(local: &str) -> SharedSymbol {
        SharedSymbol {
            local_name: local.into(),
            kind: NodeKind::Symbol,
            symbol_kind: Some(SymbolKind::Function),
            display_name: local.into(),
            signature: format!("fn {local}()"),
            fingerprint: format!("body-{local}"),
            public: true,
            lines: [1, 1],
        }
    }

    #[test]
    fn the_same_payload_resolves_differently_in_two_layouts() {
        let helper = SharedFact::new(key(b"fn work() {}"), vec![symbol("work")], vec![]).unwrap();
        let lib = SharedFact::new(
            key(b"fn entry() { helper::work(); }"),
            vec![symbol("entry")],
            vec![SharedReference {
                source_local: "entry".into(),
                scope: String::new(),
                kind: EdgeKind::Calls,
                target: Target::Name("helper::work".into()),
                binding: None,
            }],
        )
        .unwrap();
        let helper_key = helper.key.clone();
        let lib_key = lib.key.clone();
        let mut retained = BTreeMap::new();
        retained.insert(helper_key.clone(), helper);
        retained.insert(lib_key.clone(), lib);
        let place_at = |module: &str, path: &str, key: &FactKey| Placement {
            path: path.into(),
            package: "app".into(),
            module: module.into(),
            key: key.clone(),
        };
        let nested = materialize(
            &retained,
            &[
                place_at("", "src/lib.rs", &lib_key),
                place_at("helper", "src/helper.rs", &helper_key),
            ],
        )
        .unwrap();
        let moved = materialize(
            &retained,
            &[
                place_at("", "src/lib.rs", &lib_key),
                place_at("util::helper", "src/util/helper.rs", &helper_key),
            ],
        )
        .unwrap();
        let hit = resolve(&nested).unwrap();
        let miss = resolve(&moved).unwrap();
        assert_eq!(hit.edges.len(), 1);
        assert!(hit.unresolved.is_empty());
        assert!(miss.edges.is_empty());
        assert_eq!(miss.unresolved.len(), 1);
    }
}
