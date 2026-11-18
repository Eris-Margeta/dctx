// SPDX-License-Identifier: Apache-2.0
//! Public resolver regression tests for cyclic, deep and ambiguous import graphs.

use dctx_map_model::{EdgeKind, Node, NodeKind, StableId, Summary, SummarySource};
use dctx_map_resolve::{Definition, Facts, Reference, Target, UnresolvedReason, resolve};

fn definition(path: &str, name: &str, qualified_name: &str, kind: NodeKind) -> Definition {
    Definition {
        node: Node {
            id: StableId::new(kind, path, (kind == NodeKind::Symbol).then_some(name), None)
                .unwrap(),
            kind,
            symbol_kind: None,
            path: path.into(),
            lines: [1, 1],
            name: name.into(),
            signature: String::new(),
            fingerprint: String::new(),
            public: true,
            summary: Summary {
                text: String::new(),
                source: SummarySource::Signature,
            },
        },
        qualified_name: qualified_name.into(),
    }
}

fn module(name: &str) -> Facts {
    let path = format!("{name}.rs");
    Facts {
        definitions: vec![definition(&path, name, name, NodeKind::Module)],
        path,
        package: "fixture".into(),
        references: Vec::new(),
    }
}

fn add_reference(file: &mut Facts, kind: EdgeKind, target: impl Into<String>) {
    file.references.push(Reference {
        source: file.definitions[0].node.id.clone(),
        scope: file.definitions[0].qualified_name.clone(),
        kind,
        target: Target::Name(target.into()),
        binding: None,
    });
}

fn add_symbol(file: &mut Facts, name: &str) -> StableId {
    let qualified = format!("{}::{name}", file.definitions[0].qualified_name);
    let definition = definition(&file.path, name, &qualified, NodeKind::Symbol);
    let id = definition.node.id.clone();
    file.definitions.push(definition);
    id
}

#[test]
fn two_module_import_cycle_reports_missing_and_can_still_reach_an_exit() {
    let (mut a, mut b, mut c) = (module("a"), module("b"), module("c"));
    let leaf = add_symbol(&mut c, "leaf");
    add_reference(&mut a, EdgeKind::Reexports, "crate::b::*");
    add_reference(&mut b, EdgeKind::Reexports, "crate::a::*");
    add_reference(&mut b, EdgeKind::Reexports, "crate::c::*");
    add_reference(&mut a, EdgeKind::Calls, "crate::a::leaf");
    add_reference(&mut a, EdgeKind::Calls, "crate::a::missing");
    let missing = a.references.last().unwrap().clone();

    let result = resolve(&[a, b, c]).unwrap();
    assert_eq!(result.edges.len(), 4);
    let call = result
        .edges
        .iter()
        .find(|edge| edge.kind == EdgeKind::Calls)
        .unwrap();
    assert_eq!(call.src, missing.source);
    assert_eq!(call.dst, leaf);
    assert_eq!(result.unresolved.len(), 1);
    assert_eq!(result.unresolved[0].reference, missing);
    assert_eq!(result.unresolved[0].reason, UnresolvedReason::Missing);
}

#[test]
fn ten_thousand_import_hops_resolve_on_the_default_test_stack() {
    const HOPS: usize = 10_000;
    let mut files: Vec<_> = (0..=HOPS).map(|i| module(&format!("m{i}"))).collect();
    for (i, file) in files.iter_mut().take(HOPS).enumerate() {
        add_reference(file, EdgeKind::Reexports, format!("crate::m{}::*", i + 1));
    }
    let leaf = add_symbol(&mut files[HOPS], "leaf");
    add_reference(&mut files[0], EdgeKind::Calls, "crate::m0::leaf");

    let result = resolve(&files).unwrap();
    assert!(result.unresolved.is_empty());
    assert_eq!(result.edges.len(), HOPS + 1);
    let call = result
        .edges
        .iter()
        .find(|edge| edge.kind == EdgeKind::Calls)
        .unwrap();
    assert_eq!(call.src, files[0].definitions[0].node.id);
    assert_eq!(call.dst, leaf);
}

#[test]
fn competing_imports_report_both_candidates_without_adding_a_call_edge() {
    let (mut caller, mut a, mut b) = (module("caller"), module("a"), module("b"));
    let candidates = vec![add_symbol(&mut a, "work"), add_symbol(&mut b, "work")];
    add_reference(&mut caller, EdgeKind::Imports, "crate::b::*");
    add_reference(&mut caller, EdgeKind::Imports, "crate::a::*");
    add_reference(&mut caller, EdgeKind::Calls, "work");
    let call = caller.references.last().unwrap().clone();

    let result = resolve(&[caller, b, a]).unwrap();
    assert_eq!(result.edges.len(), 2);
    assert!(
        result
            .edges
            .iter()
            .all(|edge| edge.kind == EdgeKind::Imports)
    );
    assert_eq!(result.unresolved.len(), 1);
    assert_eq!(result.unresolved[0].reference, call);
    assert_eq!(
        result.unresolved[0].reason,
        UnresolvedReason::Ambiguous(candidates)
    );
}

#[test]
fn resolution_order_survives_twenty_input_permutations() {
    let (mut caller, mut a, mut b, mut c) =
        (module("caller"), module("a"), module("b"), module("c"));
    for file in [&mut a, &mut b, &mut c] {
        add_symbol(file, "work");
        add_symbol(file, "helper");
    }
    for target in ["crate::c::*", "crate::b::*", "crate::a::*"] {
        add_reference(&mut caller, EdgeKind::Imports, target);
    }
    for target in ["work", "crate::a::helper", "crate::c::helper", "absent"] {
        add_reference(&mut caller, EdgeKind::Calls, target);
    }
    let files = [caller, a, b, c];
    let expected = resolve(&files).unwrap();
    assert_eq!(expected.edges.len(), 5);
    assert_eq!(expected.unresolved.len(), 2);

    for run in 0..20 {
        let mut shuffled = files.clone();
        // Enumerate 20 distinct file permutations using factorial digits.
        let mut permutation = run;
        for index in 1..shuffled.len() {
            shuffled.swap(index, permutation % (index + 1));
            permutation /= index + 1;
        }
        if run % 2 == 0 {
            for file in &mut shuffled {
                file.definitions.reverse();
                file.references.reverse();
            }
        }
        assert_eq!(resolve(&shuffled).unwrap(), expected, "run {run}");
    }
}
