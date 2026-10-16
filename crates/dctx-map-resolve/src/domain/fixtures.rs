//! Hand-built parser output shared by resolver unit tests.
//! Exercises SPEC-MAP-MODE sections 5.3 and 6.3 across three Rust modules.
//! Depends only on public model and resolver facts; never reads fixture files.
use super::{Definition, Facts, Reference, Target};
use dctx_map_model::{
    Confidence, Edge, EdgeKind, Node, NodeKind, StableId, Summary, SummarySource, SymbolKind,
};

pub(crate) fn definition(path: &str, name: &str, qualified: &str, kind: NodeKind) -> Definition {
    let named = matches!(kind, NodeKind::Symbol | NodeKind::Test | NodeKind::Doc);
    Definition {
        node: Node {
            id: StableId::new(kind, path, named.then_some(name), None).unwrap(),
            kind,
            symbol_kind: (kind == NodeKind::Symbol).then_some(SymbolKind::Function),
            path: path.into(),
            lines: [1, 3],
            name: name.into(),
            signature: format!("fn {name}()"),
            fingerprint: format!("fingerprint:{qualified}"),
            public: true,
            summary: Summary {
                text: name.into(),
                source: SummarySource::Signature,
            },
        },
        qualified_name: qualified.into(),
    }
}

pub(crate) fn file(path: &str, definitions: Vec<Definition>) -> Facts {
    Facts {
        path: path.into(),
        package: "fixture".into(),
        definitions,
        references: vec![],
    }
}

pub(crate) fn reference(source: &Definition, scope: &str, kind: EdgeKind, name: &str) -> Reference {
    Reference {
        source: source.node.id.clone(),
        scope: scope.into(),
        kind,
        target: Target::Name(name.into()),
        binding: None,
    }
}

pub(crate) struct Fixture {
    pub files: Vec<Facts>,
    pub expected: Vec<Edge>,
}

pub(crate) fn three_modules() -> Fixture {
    let root = definition("src/lib.rs", "fixture", "", NodeKind::Module);
    let api = definition("src/api.rs", "api", "api", NodeKind::Module);
    let worker = definition("src/worker.rs", "worker", "worker", NodeKind::Module);
    let entry = definition("src/lib.rs", "entry", "entry", NodeKind::Symbol);
    let run = definition("src/api.rs", "run", "api::run", NodeKind::Symbol);
    let test = definition(
        "src/api.rs",
        "checks_run",
        "api::checks_run",
        NodeKind::Test,
    );
    let work = definition("src/worker.rs", "work", "worker::work", NodeKind::Symbol);
    let helper = definition(
        "src/worker.rs",
        "helper",
        "worker::helper",
        NodeKind::Symbol,
    );
    let mut item = definition("src/worker.rs", "Item", "worker::Item", NodeKind::Symbol);
    item.node.symbol_kind = Some(SymbolKind::Struct);
    let mut trait_node = definition("src/worker.rs", "Job", "worker::Job", NodeKind::Symbol);
    trait_node.node.symbol_kind = Some(SymbolKind::Trait);
    let doc = definition("README.md", "usage", "usage", NodeKind::Doc);
    let mut files = vec![
        file("src/lib.rs", vec![root.clone(), entry.clone()]),
        file("src/api.rs", vec![api.clone(), run.clone(), test.clone()]),
        file(
            "src/worker.rs",
            vec![
                worker.clone(),
                work.clone(),
                helper.clone(),
                item.clone(),
                trait_node.clone(),
            ],
        ),
        file("README.md", vec![doc.clone()]),
    ];
    let mut expected = Vec::new();
    let mut add = |index: usize,
                   source: &Definition,
                   scope: &str,
                   kind,
                   target: &Definition,
                   name: &str,
                   conf| {
        let mut raw = reference(source, scope, kind, name);
        if kind == EdgeKind::Contains {
            raw.target = Target::Id(target.node.id.clone());
        }
        files[index].references.push(raw);
        expected.push(Edge {
            src: source.node.id.clone(),
            dst: target.node.id.clone(),
            kind,
            conf,
            owner_path: files[index].path.clone(),
        });
    };
    add(
        0,
        &root,
        "",
        EdgeKind::Contains,
        &api,
        "",
        Confidence::Exact,
    );
    add(
        0,
        &root,
        "",
        EdgeKind::Contains,
        &worker,
        "",
        Confidence::Exact,
    );
    add(
        0,
        &root,
        "",
        EdgeKind::Contains,
        &entry,
        "",
        Confidence::Exact,
    );
    add(
        1,
        &api,
        "api",
        EdgeKind::Contains,
        &run,
        "",
        Confidence::Exact,
    );
    add(
        2,
        &worker,
        "worker",
        EdgeKind::Contains,
        &work,
        "",
        Confidence::Exact,
    );
    add(
        1,
        &api,
        "api",
        EdgeKind::Imports,
        &work,
        "crate::worker::work",
        Confidence::Exact,
    );
    add(
        1,
        &api,
        "api",
        EdgeKind::Reexports,
        &item,
        "crate::worker::Item",
        Confidence::Exact,
    );
    add(
        0,
        &root,
        "",
        EdgeKind::Imports,
        &item,
        "crate::api::Item",
        Confidence::Exact,
    );
    add(
        1,
        &run,
        "api",
        EdgeKind::Calls,
        &work,
        "work",
        Confidence::Name,
    );
    add(
        0,
        &entry,
        "",
        EdgeKind::Calls,
        &helper,
        "helper",
        Confidence::Guess,
    );
    add(
        0,
        &entry,
        "",
        EdgeKind::Calls,
        &run,
        "api::run",
        Confidence::Name,
    );
    add(
        1,
        &run,
        "api",
        EdgeKind::UsesType,
        &item,
        "Item",
        Confidence::Name,
    );
    add(
        2,
        &item,
        "worker",
        EdgeKind::Implements,
        &trait_node,
        "self::Job",
        Confidence::Exact,
    );
    add(
        1,
        &test,
        "api",
        EdgeKind::Tests,
        &run,
        "run",
        Confidence::Name,
    );
    add(
        3,
        &doc,
        "",
        EdgeKind::Documents,
        &run,
        "api::run",
        Confidence::Name,
    );
    add(
        3,
        &doc,
        "",
        EdgeKind::Documents,
        &api,
        "src/api.rs",
        Confidence::Name,
    );
    files[0]
        .references
        .push(reference(&entry, "", EdgeKind::Calls, "external::missing"));
    expected.sort_by(|a, b| {
        (&a.owner_path, &a.src, a.kind, &a.dst, a.conf).cmp(&(
            &b.owner_path,
            &b.src,
            b.kind,
            &b.dst,
            b.conf,
        ))
    });
    Fixture { files, expected }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_has_three_modules_and_every_edge_kind() {
        let fixture = three_modules();
        assert_eq!(
            fixture
                .files
                .iter()
                .flat_map(|file| &file.definitions)
                .filter(|definition| definition.node.kind == NodeKind::Module)
                .count(),
            3
        );
        assert_eq!(
            fixture
                .expected
                .iter()
                .map(|edge| edge.kind)
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            8
        );
    }
}
