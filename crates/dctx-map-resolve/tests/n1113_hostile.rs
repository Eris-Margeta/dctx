// SPDX-License-Identifier: Apache-2.0
//! Deterministic hostile inputs at the public resolver boundary.
//! Text APIs take `str`; invalid UTF-8 is rejected before those APIs are called.
use dctx_map_model::{ContentId, EdgeKind, Node, NodeKind, StableId, Summary, SummarySource};
use dctx_map_resolve::*;
use std::collections::{BTreeMap, BTreeSet};

fn key(bytes: &[u8]) -> FactKey {
    FactKey::new(
        ContentId::of_bytes(bytes).unwrap(),
        contract("p1", "plan").unwrap(),
    )
    .unwrap()
}

fn symbol(name: &str) -> SharedSymbol {
    SharedSymbol {
        local_name: name.into(),
        kind: NodeKind::Symbol,
        symbol_kind: None,
        display_name: name.into(),
        signature: String::new(),
        fingerprint: "body".into(),
        public: true,
        lines: [1, 1],
    }
}

fn fact() -> SharedFact {
    SharedFact::new(
        key(b"ab"),
        vec![symbol("a"), symbol("b")],
        vec![SharedReference {
            source_local: "a".into(),
            scope: String::new(),
            kind: EdgeKind::Calls,
            target: Target::Name("b".into()),
            binding: None,
        }],
    )
    .unwrap()
}

fn view(key: FactKey) -> View {
    View {
        root: "root".into(),
        trust_namespace: "local".into(),
        resolver_revision: "r1".into(),
        ranking_revision: "k1".into(),
        frozen_clock: 0,
        placements: vec![Placement {
            path: "a.rs".into(),
            package: "app".into(),
            module: String::new(),
            key,
        }],
    }
}

fn node(path: &str) -> Node {
    Node {
        id: StableId::new(NodeKind::Symbol, path, Some("a"), None).unwrap(),
        kind: NodeKind::Symbol,
        symbol_kind: None,
        path: path.into(),
        lines: [1, 1],
        name: "a".into(),
        signature: String::new(),
        fingerprint: "body".into(),
        public: true,
        summary: Summary {
            text: String::new(),
            source: SummarySource::Signature,
        },
    }
}

#[test]
fn empty_inputs_cover_all_pure_snapshot_operations() {
    let empty = resolve(&[]).unwrap();
    let table = SymbolTable::new(&[]).unwrap();
    assert!(table.is_empty());
    assert_eq!(table.len(), 0);
    assert!(table.node(&node("a.rs").id).is_none());
    assert!(alias_collisions(&[]).is_empty());
    assert!(name_collisions(&[]).is_empty());
    assert!(disambiguate_name(&[], "", "").is_empty());
    assert!(detect_moves(&[], &[], "").is_empty());
    assert!(body_collisions(&[], &[]).is_empty());
    assert_eq!(expand_alias(&[], ""), AliasExpansion::Unknown);
    assert_eq!(expansion_from_hits(vec![]), AliasExpansion::Unknown);
    assert_eq!(reverse_index(&[], &empty), ReverseIndex::default());
    assert!(materialize(&BTreeMap::new(), &[]).unwrap().is_empty());
    assert!(SharedFact::new(key(b""), vec![], vec![]).is_ok());
}

#[test]
fn content_keys_cover_bytes_prefixes_and_invalid_utf8() {
    let sample = b"fn a() { b(); }\xff".to_vec();
    assert!(std::str::from_utf8(&sample).is_err());
    for end in 0..=sample.len() {
        let k = key(&sample[..end]);
        k.check().unwrap();
        assert_eq!(k, key(&sample[..end]));
        assert_eq!(
            k.to_string().as_bytes(),
            key(&sample[..end]).to_string().as_bytes()
        );
        assert_eq!(k.content().byte_length(), end as u64);
        assert_eq!(k.contract(), &contract("p1", "plan").unwrap());
        assert_eq!(k.cmp(&k), std::cmp::Ordering::Equal);
    }
}

#[test]
fn contracts_reject_ambiguous_serialization_and_git_prefixes() {
    for bad in ["", "a|b", "git:opaque", "a/b", "a\\b", "\0", "a b"] {
        assert!(contract(bad, "plan").is_err(), "{bad:?}");
        assert!(contract("p1", bad).is_err(), "{bad:?}");
        let mut c = contract("p1", "plan").unwrap();
        c.dialect = bad.into();
        assert!(c.check().is_err(), "{bad:?}");
        c = contract("p1", "plan").unwrap();
        c.parser_id = bad.into();
        assert!(FactKey::new(ContentId::of_bytes(b"").unwrap(), c).is_err());
    }
    for field in [0, 1, u16::MAX] {
        let mut c = contract("p1", "plan").unwrap();
        c.language = field;
        c.fact_schema = field;
        assert_eq!(c.check().is_ok(), field != 0);
    }
}

#[test]
fn contract_and_name_prefixes_have_no_panics() {
    let sample = "module::caf\u{e9}";
    for end in 0..=sample.len() {
        let Ok(text) = std::str::from_utf8(&sample.as_bytes()[..end]) else {
            continue;
        };
        let _ = contract(text, "plan");
        let mut s = symbol("a");
        s.local_name = text.into();
        let _ = SharedFact::new(key(b"a"), vec![s], vec![]);
        let _ = expand_alias(&[node("a.rs").id], text);
        let mut v = view(key(b"a"));
        v.root = text.into();
        let _ = v.validate();
    }
    let large = "a".repeat(4097); // Names have no fixed byte ceiling in this API.
    assert!(SharedFact::new(key(b"a"), vec![symbol(&large)], vec![]).is_ok());
}

#[test]
fn shared_spans_cover_zero_maximum_and_reversed_ranges() {
    for (lines, valid) in [
        ([0, 0], false),
        ([1, 1], true),
        ([1, u32::MAX], true),
        ([u32::MAX, u32::MAX], true),
        ([2, 1], false),
    ] {
        let mut s = symbol("a");
        s.lines = lines;
        assert_eq!(SharedFact::new(key(b"a"), vec![s], vec![]).is_ok(), valid);
    }
}

#[test]
fn shared_payload_reordering_and_duplicate_references_reuse_the_key() {
    let original = fact();
    let mut store = FactStore::new();
    assert_eq!(store.admit(original.clone()).unwrap(), Admission::Parsed);
    let mut reordered = original.clone();
    reordered.symbols.reverse();
    reordered.references.extend(original.references.clone());
    assert_eq!(store.admit(reordered).unwrap(), Admission::Reused);
    assert_eq!(store.parses(), 1);
    assert_eq!(store.retained_count(), 1);
    assert!(store.contains(&original.key));
    let mut conflict = original.clone();
    conflict.symbols[0].fingerprint.push('x');
    assert_eq!(store.admit(conflict), Err(ResolveError::ConflictingFact));
    assert_eq!(store.parses(), 1);
    assert!(store.reclaim(&original.key));
    assert!(!store.reclaim(&original.key));
    assert!(!store.contains(&original.key));
}

#[test]
fn materialize_validates_publicly_mutable_payloads_and_map_keys() {
    let original = fact();
    let v = view(original.key.clone());
    let mut bad = original.clone();
    bad.symbols.push(bad.symbols[0].clone());
    assert!(
        materialize(
            &BTreeMap::from([(original.key.clone(), bad)]),
            &v.placements
        )
        .is_err()
    );
    let mut bad = original.clone();
    bad.symbols[0].lines = [0, 0];
    assert!(
        materialize(
            &BTreeMap::from([(original.key.clone(), bad)]),
            &v.placements
        )
        .is_err()
    );
    let other = key(b"other");
    assert!(
        materialize(
            &BTreeMap::from([(other.clone(), original)]),
            &view(other).placements
        )
        .is_err()
    );
}

#[test]
fn views_validate_paths_duplicates_authorization_and_missing_facts() {
    let f = fact();
    let mut store = FactStore::default();
    store.admit(f.clone()).unwrap();
    let good = view(f.key.clone());
    for bad in [
        "", ".", "..", "../a", "/a", "a//b", "a\\b", "a/.git/b", "a\nb",
    ] {
        let mut v = good.clone();
        v.placements[0].path = bad.into();
        assert!(store.placed_facts(&v).is_err(), "{bad:?}");
        assert!(store.resolve_view(&v).is_err());
    }
    let mut duplicate = good.clone();
    duplicate.placements.push(duplicate.placements[0].clone());
    assert!(matches!(
        store.resolve_view(&duplicate),
        Err(ResolveError::DuplicateFile(_))
    ));
    let none = good.authorize(&BTreeSet::new());
    assert!(
        store
            .resolve_view(&none)
            .unwrap()
            .resolution
            .edges
            .is_empty()
    );
    assert_eq!(good.authorize(&BTreeSet::from(["a.rs".into()])), good);
    let mut missing = good.clone();
    missing.placements[0].key = key(b"missing");
    assert!(matches!(
        store.resolve_view(&missing),
        Err(ResolveError::MissingFact(_))
    ));
    for field in ["", "git:opaque", "a\tb", "a/b"] {
        let mut v = good.clone();
        v.root = field.into();
        assert!(v.validate().is_err());
    }
}

#[test]
fn resolutions_are_byte_identical_across_repeats_reordering_and_threads() {
    let f = fact();
    let mut store = FactStore::new();
    store.admit(f.clone()).unwrap();
    let mut v = view(f.key);
    let mut second = v.placements[0].clone();
    second.path = "b.rs".into();
    second.module = "other".into();
    v.placements.push(second);
    let expected = store.resolve_view(&v).unwrap().canonical_bytes();
    v.placements.reverse();
    assert_eq!(
        store.resolve_view(&v).unwrap().canonical_bytes().as_bytes(),
        expected.as_bytes()
    );
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..4)
            .map(|_| scope.spawn(|| store.resolve_view(&v).unwrap().canonical_bytes()))
            .collect();
        for h in handles {
            assert_eq!(h.join().unwrap().as_bytes(), expected.as_bytes());
        }
    });
    let files = store.placed_facts(&v).unwrap();
    let mut reordered = files.clone();
    reordered.reverse();
    for f in &mut reordered {
        f.definitions.reverse();
        f.references.reverse();
    }
    assert_eq!(resolve(&files), resolve(&reordered));
    let r = resolve(&files).unwrap();
    let reverse = reverse_index(&files, &r);
    assert_eq!(
        reverse_index(&reordered, &resolve(&reordered).unwrap()),
        reverse
    );
    assert!(
        invalidated_dependents(&reverse, &files[0], &files[0])
            .unwrap()
            .is_empty()
    );
    assert!(invalidated_dependents(&reverse, &files[0], &files[1]).is_err());
    assert_eq!(alias_collisions(&files), alias_collisions(&reordered));
    assert_eq!(name_collisions(&files), name_collisions(&reordered));
    assert_eq!(
        disambiguate_name(&files, "app", "a"),
        disambiguate_name(&reordered, "app", "a")
    );
}

#[test]
fn resolver_deduplicates_references_and_rejects_duplicate_definitions() {
    let f = fact();
    let mut store = FactStore::new();
    store.admit(f.clone()).unwrap();
    let files = store.placed_facts(&view(f.key)).unwrap();
    let expected = resolve(&files).unwrap();
    let mut duplicated = files.clone();
    duplicated[0].references.extend(files[0].references.clone());
    assert_eq!(resolve(&duplicated).unwrap(), expected);
    assert!(matches!(
        resolve(&[files[0].clone(), files[0].clone()]),
        Err(ResolveError::DuplicateFile(_))
    ));
    duplicated[0]
        .definitions
        .push(files[0].definitions[0].clone());
    assert!(matches!(
        resolve(&duplicated),
        Err(ResolveError::DuplicateNode(_))
    ));
    assert!(
        SymbolTable::new(&files)
            .unwrap()
            .node(&files[0].definitions[0].node.id)
            .is_some()
    );
}

#[test]
fn body_collisions_count_identities_once_like_move_detection() {
    let old = node("old.rs");
    let new = node("new.rs");
    let old2 = node("old2.rs");
    assert!(body_collisions(&[old.clone(), old.clone()], &[new.clone(), new.clone()]).is_empty());
    let expected = body_collisions(&[old.clone(), old2.clone()], std::slice::from_ref(&new));
    assert_eq!(
        body_collisions(
            &[old2.clone(), old.clone(), old2],
            &[new.clone(), new.clone()]
        ),
        expected
    );
    let moves = detect_moves(
        &[old.clone(), old.clone()],
        &[new.clone(), new.clone()],
        "r",
    );
    assert_eq!(moves.len(), 1);
    assert_eq!(
        moves[0].identity(),
        dctx_map_model::IdentityStatus::Moved {
            old: old.id,
            new: new.id.clone()
        }
    );
    assert_eq!(
        expand_alias(&[new.id.clone(), new.id.clone()], new.id.as_str()),
        AliasExpansion::Unique(new.id.clone())
    );
    assert_eq!(
        expansion_from_hits(vec![new.id.clone(), new.id.clone()]),
        AliasExpansion::Unique(new.id)
    );
}

#[test]
fn public_resolver_errors_name_a_next_step() {
    let id = node("a.rs").id;
    let errors = [
        ResolveError::DuplicateFile("a.rs".into()),
        ResolveError::DuplicateNode(id.clone()),
        ResolveError::InvalidNode(id.clone()),
        ResolveError::UnknownSource(id.clone()),
        ResolveError::ForeignSource(id),
        ResolveError::InvalidScope("bad".into()),
        ResolveError::InvalidBinding("bad".into()),
        ResolveError::UnprovenContainment,
        ResolveError::DuplicateSymbol("a".into()),
        ResolveError::ConflictingFact,
        ResolveError::MissingFact("a.rs".into()),
        ResolveError::MissingSymbol("a".into()),
        ResolveError::InvalidRoot("bad".into()),
        ResolveError::InvalidPath("bad".into()),
        ResolveError::LayoutDependentTarget,
    ];
    for error in errors {
        let message = error.to_string();
        let (_, next) = message.split_once("; next: ").unwrap();
        assert!(!next.trim().is_empty(), "{message}");
        assert_eq!(error.to_string().as_bytes(), message.as_bytes());
    }
}

#[test]
fn reference_target_prefixes_and_invalid_contract_fields_refuse_or_resolve() {
    let f = fact();
    let mut store = FactStore::new();
    store.admit(f.clone()).unwrap();
    let files = store.placed_facts(&view(f.key)).unwrap();
    let sample = "crate::b";
    for cut in 0..=sample.len() {
        let mut snapshot = files.clone();
        snapshot[0].references[0].target = Target::Name(sample[..cut].into());
        let first = resolve(&snapshot).unwrap();
        assert_eq!(resolve(&snapshot).unwrap(), first);
        assert_eq!(first.edges.len() + first.unresolved.len(), 1);
    }
    for field in 0..5 {
        let mut bad = files.clone();
        match field {
            0 => bad[0].references[0].scope = "a::".into(),
            1 => bad[0].references[0].binding = Some("alias".into()),
            2 => bad[0].references[0].source = node("absent.rs").id,
            3 => bad[0].definitions[0].node.path = "foreign.rs".into(),
            _ => bad[0].references[0].kind = EdgeKind::Contains,
        }
        assert!(resolve(&bad).is_err());
        assert!(SymbolTable::new(&bad).is_err());
    }
}
