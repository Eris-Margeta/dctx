//! Property tests for deterministic views and content-keyed sharing.
//! Implements PRODUCT-CONTRACTS P6 `p6_permutation_replay` and P7 `p7_non_vcs_reuse`.
//! Depends on the public view resolver. No clock, filesystem or git object id is read.
use super::*;
use dctx_map_model::{ContentId, EdgeKind, NodeKind, StableId, SymbolKind};
use proptest::prelude::*;
use std::collections::BTreeSet;

const LIB: &[u8] = b"fn entry(){ helper::work() }";
const HELPER: &[u8] = b"fn work(){ 1 }";

fn symbol(local: &str) -> SharedSymbol {
    SharedSymbol {
        local_name: local.into(),
        kind: NodeKind::Symbol,
        symbol_kind: Some(SymbolKind::Function),
        display_name: local.into(),
        signature: format!("fn {local}()"),
        fingerprint: format!("body-{local}"),
        public: true,
        lines: [1, 2],
    }
}

fn call(target: &str, binding: Option<&str>, kind: EdgeKind) -> SharedReference {
    SharedReference {
        source_local: "entry".into(),
        scope: String::new(),
        kind,
        target: Target::Name(target.into()),
        binding: binding.map(str::to_owned),
    }
}

fn fact(bytes: &[u8], revision: &str, plan: &str, symbols: Vec<SharedSymbol>, references: Vec<SharedReference>) -> SharedFact {
    SharedFact::new(
        FactKey::new(ContentId::of_bytes(bytes).unwrap(), contract(revision, plan).unwrap()).unwrap(),
        symbols,
        references,
    )
    .unwrap()
}

fn placement(path: &str, package: &str, module: &str, key: &FactKey) -> Placement {
    Placement {
        path: path.into(),
        package: package.into(),
        module: module.into(),
        key: key.clone(),
    }
}

fn view(root: &str, placements: Vec<Placement>) -> View {
    View {
        root: root.into(),
        trust_namespace: "local".into(),
        resolver_revision: "resolve-1".into(),
        ranking_revision: "rank-1".into(),
        frozen_clock: 17,
        placements,
    }
}

fn sample() -> (FactStore, View) {
    let helper = fact(HELPER, "p1", "plan-1", vec![symbol("other"), symbol("work")], vec![]);
    let lib = fact(
        LIB,
        "p1",
        "plan-1",
        vec![symbol("entry")],
        vec![
            call("helper::work", Some("execute"), EdgeKind::Imports),
            call("helper::other", Some("execute"), EdgeKind::Imports),
            call("execute", None, EdgeKind::Calls),
        ],
    );
    let mut store = FactStore::new();
    assert_eq!(store.admit(helper.clone()).unwrap(), Admission::Parsed);
    assert_eq!(store.admit(lib.clone()).unwrap(), Admission::Parsed);
    let mut reversed = helper.symbols.clone();
    reversed.reverse();
    assert_eq!(
        store
            .admit(SharedFact::new(helper.key.clone(), reversed, vec![]).unwrap())
            .unwrap(),
        Admission::Reused
    );
    let layout = view(
        "root-app",
        vec![
            placement("src/helper.rs", "app", "helper", &helper.key),
            placement("src/lib.rs", "app", "", &lib.key),
        ],
    );
    (store, layout)
}

fn merge(parts: &[Resolution]) -> Resolution {
    let mut edges = Vec::new();
    let mut unresolved = Vec::new();
    for part in parts {
        edges.extend(part.edges.iter().cloned());
        unresolved.extend(part.unresolved.iter().cloned());
    }
    edges.sort_by(|left, right| {
        (&left.owner_path, &left.src, left.kind, &left.dst, left.conf).cmp(&(
            &right.owner_path,
            &right.src,
            right.kind,
            &right.dst,
            right.conf,
        ))
    });
    edges.dedup();
    unresolved.sort();
    unresolved.dedup();
    Resolution { edges, unresolved }
}

fn insertion_winner(files: &[Facts]) -> StableId {
    files
        .iter()
        .flat_map(|file| &file.definitions)
        .find(|definition| definition.node.name == "work")
        .unwrap()
        .node
        .id
        .clone()
}

proptest! {
    #[test]
    fn p6_permutation_replay(swaps in prop::collection::vec(any::<usize>(), 0..36)) {
        let left = super::fixtures::definition("src/a.rs", "work", "a::work", NodeKind::Symbol);
        let right = super::fixtures::definition("src/b.rs", "work", "b::work", NodeKind::Symbol);
        let entry = super::fixtures::definition("src/lib.rs", "entry", "entry", NodeKind::Symbol);
        let mut caller = super::fixtures::file("src/lib.rs", vec![entry.clone()]);
        caller
            .references
            .push(super::fixtures::reference(&entry, "", EdgeKind::Calls, "work"));
        let forward = vec![
            super::fixtures::file("src/a.rs", vec![left.clone()]),
            super::fixtures::file("src/b.rs", vec![right.clone()]),
            caller.clone(),
        ];
        let backward = vec![
            super::fixtures::file("src/b.rs", vec![right]),
            super::fixtures::file("src/a.rs", vec![left]),
            caller,
        ];
        prop_assert_ne!(insertion_winner(&forward), insertion_winner(&backward));
        let resolved = resolve(&forward).unwrap();
        prop_assert_eq!(&resolve(&backward).unwrap(), &resolved);
        prop_assert!(matches!(
            &resolved.unresolved[0].reason,
            UnresolvedReason::Ambiguous(ids) if ids.len() == 2
        ));
        prop_assert!(resolved.edges.is_empty());

        let (store, layout) = sample();
        let base = store.resolve_view(&layout).unwrap();
        let bytes = base.canonical_bytes();
        prop_assert!(bytes.contains("ranking\trank-1\n"));
        prop_assert!(bytes.contains("clock\t17\n"));
        prop_assert!(bytes.contains("alias\t"));
        prop_assert!(!base.alias_collisions.is_empty());
        let mut clock = layout.clone();
        clock.frozen_clock = 18;
        let later = store.resolve_view(&clock).unwrap();
        prop_assert_ne!(later.canonical_bytes(), bytes.clone());
        prop_assert_eq!(&later.resolution, &base.resolution);
        let mut rank = layout.clone();
        rank.ranking_revision = "rank-2".into();
        prop_assert_ne!(store.resolve_view(&rank).unwrap().canonical_bytes(), bytes.clone());
        prop_assert_eq!(&store.resolve_view(&rank).unwrap().resolution, &base.resolution);

        let mut placements = layout.placements.clone();
        for (index, swap) in swaps.iter().enumerate() {
            let count = placements.len();
            placements.swap(index % count, swap % count);
        }
        let mut shuffled = layout.clone();
        shuffled.placements = placements;
        prop_assert_eq!(store.resolve_view(&shuffled).unwrap().canonical_bytes(), bytes);

        let mut files = store.placed_facts(&layout).unwrap();
        prop_assert_eq!(&resolve(&files).unwrap(), &base.resolution);
        for (index, swap) in swaps.iter().enumerate() {
            let count = files.len();
            files.swap(index % count, swap % count);
            for file in &mut files {
                if !file.definitions.is_empty() {
                    let count = file.definitions.len();
                    file.definitions.swap(index % count, swap % count);
                }
                if !file.references.is_empty() {
                    let count = file.references.len();
                    file.references.swap(index % count, swap % count);
                }
            }
        }
        prop_assert_eq!(&resolve(&files).unwrap(), &base.resolution);
        let mut extra_entry = super::fixtures::definition("src/extra.rs", "extra", "extra", NodeKind::Symbol);
        extra_entry.node.fingerprint = "body-extra".into();
        let mut extra = super::fixtures::file("src/extra.rs", vec![extra_entry.clone()]);
        extra.package = "other-crate".into();
        extra
            .references
            .push(super::fixtures::reference(&extra_entry, "", EdgeKind::Calls, "extra"));
        let mut together = files.clone();
        together.push(extra.clone());
        let combined = resolve(&together).unwrap();
        prop_assert_eq!(&merge(&[resolve(&files).unwrap(), resolve(std::slice::from_ref(&extra)).unwrap()]), &combined);
        prop_assert_eq!(&merge(&[resolve(std::slice::from_ref(&extra)).unwrap(), resolve(&files).unwrap()]), &combined);
        prop_assert_eq!(base.resolution.edges.len(), 2);
        prop_assert!(base.resolution.edges.iter().all(|edge| edge.kind.permits(edge.conf)));
    }
}

#[test]
fn p7_non_vcs_reuse() {
    let helper = fact(HELPER, "p1", "plan-1", vec![symbol("work")], vec![]);
    let lib = fact(
        LIB,
        "p1",
        "plan-1",
        vec![symbol("entry")],
        vec![call("helper::work", None, EdgeKind::Calls)],
    );
    let helper_key = helper.key.clone();
    let lib_key = lib.key.clone();
    let mut store = FactStore::new();
    assert_eq!(store.admit(helper.clone()).unwrap(), Admission::Parsed);
    assert_eq!(store.admit(lib.clone()).unwrap(), Admission::Parsed);
    assert_eq!(store.admit(helper.clone()).unwrap(), Admission::Reused);
    assert_eq!(store.admit(lib.clone()).unwrap(), Admission::Reused);
    assert_eq!(store.admit(lib.clone()).unwrap(), Admission::Reused);
    assert_eq!(store.parses(), 2);
    assert_eq!(store.retained_count(), 2);
    let plugin_key = |plugin: &str| {
        let _ = plugin;
        FactKey::new(ContentId::of_bytes(LIB).unwrap(), contract("p1", "plan-1").unwrap()).unwrap()
    };
    assert_eq!(plugin_key("dctx-plugin-git"), plugin_key("none"));
    assert_eq!(plugin_key("dctx-plugin-git"), lib_key);
    for rejected in [
        "da39a3ee5e6b4b0d3255bfef95601890afd80709",
        "git:da39a3ee5e6b4b0d3255bfef95601890afd80709",
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    ] {
        assert!(ContentId::parse(rejected).is_err(), "{rejected}");
    }
    assert!(lib_key.to_string().starts_with("dctx:object:v1:sha256:"));

    let root1 = view(
        "root-app",
        vec![
            placement("src/lib.rs", "app", "", &lib_key),
            placement("src/helper.rs", "app", "helper", &helper_key),
        ],
    );
    let root2 = view(
        "root-moved",
        vec![
            placement("src/lib.rs", "app", "", &lib_key),
            placement("src/util/helper.rs", "app", "util::helper", &helper_key),
        ],
    );
    let root3 = view(
        "root-tools",
        vec![
            placement("other/src/lib.rs", "tools", "", &lib_key),
            placement("other/src/helper.rs", "tools", "helper", &helper_key),
        ],
    );
    let hit = store.resolve_view(&root1).unwrap();
    let miss = store.resolve_view(&root2).unwrap();
    let moved = store.resolve_view(&root3).unwrap();
    assert_eq!(store.parses(), 2, "three roots share the retained keys");
    assert_eq!(hit.resolution.edges.len(), 1);
    assert!(hit.resolution.edges[0].dst.as_str().contains("src/helper.rs#work"));
    assert_eq!(miss.resolution.edges, [] as [dctx_map_model::Edge; 0]);
    assert!(matches!(miss.resolution.unresolved[0].reason, UnresolvedReason::Missing));
    assert_eq!(moved.resolution.edges.len(), 1);
    assert!(moved.resolution.edges[0].dst.as_str().contains("other/src/helper.rs#work"));
    assert_ne!(hit.resolution, miss.resolution);
    assert_ne!(hit.resolution, moved.resolution);

    let mut allowed = BTreeSet::new();
    allowed.insert("src/lib.rs".into());
    allowed.insert("src/helper.rs".into());
    assert_eq!(store.resolve_view(&root1.authorize(&allowed)).unwrap().resolution, hit.resolution);
    allowed.remove("src/helper.rs");
    let denied = store.resolve_view(&root1.authorize(&allowed)).unwrap();
    assert_eq!(denied.resolution.edges, [] as [dctx_map_model::Edge; 0]);
    assert!(matches!(denied.resolution.unresolved[0].reason, UnresolvedReason::Missing));
    assert!(denied.resolution.edges.iter().all(|edge| !edge.dst.as_str().contains("helper.rs")));

    let mut changed = helper.clone();
    changed.key = FactKey::new(ContentId::of_bytes(HELPER).unwrap(), contract("p2", "plan-1").unwrap()).unwrap();
    assert_eq!(store.admit(changed).unwrap(), Admission::Parsed);
    let mut extracted = helper.clone();
    extracted.key = FactKey::new(ContentId::of_bytes(HELPER).unwrap(), contract("p1", "plan-2").unwrap()).unwrap();
    assert_eq!(store.admit(extracted).unwrap(), Admission::Parsed);
    assert_eq!(store.parses(), 4);
    let mut conflict = helper.clone();
    conflict.symbols[0].display_name = "renamed".into();
    assert!(matches!(store.admit(conflict), Err(ResolveError::ConflictingFact)));
    assert_eq!(store.parses(), 4);
    assert!(store.reclaim(&helper_key));
    assert!(!store.contains(&helper_key));
    assert_eq!(store.admit(helper).unwrap(), Admission::Parsed);
    assert_eq!(store.parses(), 5);
    assert_eq!(store.resolve_view(&root1).unwrap().resolution, hit.resolution);
}
