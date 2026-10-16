//! Unit tests for aliases, glob imports and qualified reexport traversal.
//! Verifies SPEC-MAP-MODE section 6.3 against hand-built parser facts.
//! Uses the public resolver and model types; performs no IO.
use super::{Target, UnresolvedReason, fixtures::*, resolve};
use dctx_map_model::{Confidence, EdgeKind, NodeKind};

#[test]
fn module_alias_and_reexport_chain_resolve_to_original_identity() {
    let mut fixture = three_modules();
    let root = fixture.files[0].definitions[0].clone();
    let entry = fixture.files[0].definitions[1].clone();
    let api = fixture.files[1].definitions[0].clone();
    let work = fixture.files[2].definitions[1].clone();
    let mut export = reference(&api, "api", EdgeKind::Reexports, "crate::worker::work");
    export.binding = Some("execute".into());
    fixture.files[1].references.push(export);
    let mut import = reference(&root, "", EdgeKind::Imports, "crate::api");
    import.binding = Some("facade".into());
    fixture.files[0].references.extend([
        import,
        reference(&entry, "", EdgeKind::Calls, "facade::execute"),
    ]);
    let result = resolve(&fixture.files).unwrap();
    assert!(result.edges.iter().any(|edge| edge.src == entry.node.id
        && edge.dst == work.node.id
        && edge.kind == EdgeKind::Calls
        && edge.conf == Confidence::Name));
    assert_eq!(result.unresolved.len(), 1);
}

#[test]
fn glob_imports_follow_reexports_but_not_private_import_aliases() {
    let mut fixture = three_modules();
    let root = fixture.files[0].definitions[0].clone();
    let entry = fixture.files[0].definitions[1].clone();
    let api = fixture.files[1].definitions[0].clone();
    let work = fixture.files[2].definitions[1].clone();
    let mut alias = reference(&api, "api", EdgeKind::Imports, "crate::worker::work");
    alias.binding = Some("private_alias".into());
    fixture.files[1].references.push(alias);
    fixture.files[1].references.push(reference(
        &api,
        "api",
        EdgeKind::Reexports,
        "crate::worker::*",
    ));
    fixture.files[0].references.extend([
        reference(&root, "", EdgeKind::Imports, "crate::api::*"),
        reference(&entry, "", EdgeKind::Calls, "work"),
        reference(&entry, "", EdgeKind::Calls, "api::private_alias"),
    ]);
    let result = resolve(&fixture.files).unwrap();
    assert!(result.edges.iter().any(|edge| edge.src == entry.node.id
        && edge.dst == work.node.id
        && edge.kind == EdgeKind::Calls
        && edge.conf == Confidence::Name));
    assert!(
        result
            .unresolved
            .iter()
            .any(|item| item.reference.target == Target::Name("api::private_alias".into()))
    );
}

#[test]
fn conflicting_aliases_remain_ambiguous_and_do_not_depend_on_import_order() {
    let mut fixture = three_modules();
    let root = fixture.files[0].definitions[0].clone();
    let entry = fixture.files[0].definitions[1].clone();
    for name in ["crate::worker::work", "crate::api::run"] {
        let mut import = reference(&root, "", EdgeKind::Imports, name);
        import.binding = Some("execute".into());
        fixture.files[0].references.push(import);
    }
    fixture.files[0]
        .references
        .push(reference(&entry, "", EdgeKind::Calls, "execute"));
    let result = resolve(&fixture.files).unwrap();
    assert!(result.unresolved.iter().any(|item| item.reference.target
        == Target::Name("execute".into())
        && matches!(&item.reason, UnresolvedReason::Ambiguous(ids) if ids.len() == 2)));
    fixture.files[0].references.reverse();
    assert_eq!(resolve(&fixture.files).unwrap(), result);
}

#[test]
fn name_only_import_and_implements_matches_keep_name_confidence() {
    let mut fixture = three_modules();
    let root = fixture.files[0].definitions[0].clone();
    let entry = fixture.files[0].definitions[1].clone();
    for kind in [EdgeKind::Imports, EdgeKind::Reexports] {
        let mut import = reference(&root, "", kind, "Job");
        import.binding = Some("Task".into());
        fixture.files[0].references.push(import);
    }
    fixture.files[0]
        .references
        .push(reference(&entry, "", EdgeKind::Implements, "Job"));
    let result = resolve(&fixture.files).unwrap();
    for kind in [EdgeKind::Imports, EdgeKind::Reexports, EdgeKind::Implements] {
        assert!(
            result
                .edges
                .iter()
                .any(|edge| edge.owner_path == "src/lib.rs"
                    && edge.kind == kind
                    && edge.dst.as_str().ends_with("#Job")
                    && edge.conf == Confidence::Name)
        );
    }
}

#[test]
fn local_scope_wins_over_outer_scope_and_package_wide_bare_matches() {
    let outer = definition("lib.rs", "run", "run", NodeKind::Symbol);
    let local = definition("a.rs", "run", "a::run", NodeKind::Symbol);
    let decoy = definition("b.rs", "run", "b::run", NodeKind::Symbol);
    let mut a = file("a.rs", vec![local.clone()]);
    a.references
        .push(reference(&local, "a::nested", EdgeKind::Calls, "run"));
    let result = resolve(&[file("lib.rs", vec![outer]), a, file("b.rs", vec![decoy])]).unwrap();
    assert_eq!(result.edges.len(), 1);
    assert_eq!(result.edges[0].dst, local.node.id);
    assert_eq!(result.edges[0].conf, Confidence::Name);
}

#[test]
fn explicit_bindings_and_definitions_shadow_glob_imports() {
    let mut fixture = three_modules();
    let api = fixture.files[1].definitions[0].clone();
    let run = fixture.files[1].definitions[1].clone();
    let work = fixture.files[2].definitions[1].clone();
    let duplicate = definition("src/worker.rs", "run", "worker::run", NodeKind::Symbol);
    fixture.files[2].definitions.push(duplicate);
    fixture.files[1].references.push(reference(
        &api,
        "api",
        EdgeKind::Imports,
        "crate::worker::*",
    ));
    let mut alias = reference(&api, "api", EdgeKind::Imports, "crate::worker::work");
    alias.binding = Some("helper".into());
    fixture.files[1].references.extend([
        alias,
        reference(&run, "api", EdgeKind::Calls, "run"),
        reference(&run, "api", EdgeKind::Calls, "helper"),
    ]);
    let result = resolve(&fixture.files).unwrap();
    assert!(result.edges.iter().any(|edge| edge.src == run.node.id
        && edge.dst == run.node.id
        && edge.kind == EdgeKind::Calls));
    assert!(result.edges.iter().any(|edge| edge.src == run.node.id
        && edge.dst == work.node.id
        && edge.kind == EdgeKind::Calls));
    assert!(!result.unresolved.iter().any(|item| {
        matches!(&item.reference.target, Target::Name(name) if name == "run" || name == "helper")
    }));
}
