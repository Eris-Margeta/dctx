// SPDX-License-Identifier: Apache-2.0

use dctx_map_model::{EdgeKind, Node, NodeKind, StableId, Summary, SummarySource};
use dctx_map_resolve::{
    Definition, Facts, Reference, Resolution, Target, UnresolvedReason, resolve,
};
use std::{
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

const DEADLINE: Duration = Duration::from_secs(10);

fn checked<T: Send + 'static>(run: impl FnOnce() -> T + Send + 'static) -> T {
    let (done, finished) = mpsc::sync_channel(0);
    let start = Instant::now();
    let worker = thread::spawn(move || {
        let result = run();
        let _ = done.send(());
        result
    });
    assert_ne!(
        finished.recv_timeout(DEADLINE),
        Err(mpsc::RecvTimeoutError::Timeout),
        "hostile case exceeded {DEADLINE:?}"
    );
    let result = match worker.join() {
        Ok(result) => result,
        Err(panic) => std::panic::resume_unwind(panic),
    };
    assert!(
        start.elapsed() < DEADLINE,
        "hostile case exceeded {DEADLINE:?}"
    );

    #[cfg(target_os = "linux")]
    {
        let status = std::fs::read_to_string("/proc/self/status").unwrap();
        let peak_kib: usize = status
            .lines()
            .find_map(|line| line.strip_prefix("VmHWM:"))
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap()
            .parse()
            .unwrap();
        assert!(
            peak_kib < 512 * 1024,
            "test process peaked at {peak_kib} KiB"
        );
    }
    result
}

fn definition(path: &str, name: &str, qualified: String, kind: NodeKind) -> Definition {
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
        qualified_name: qualified,
    }
}

fn module(name: &str) -> Facts {
    let path = format!("{name}.rs");
    Facts {
        definitions: vec![definition(&path, name, name.into(), NodeKind::Module)],
        path,
        package: "fixture".into(),
        references: Vec::new(),
    }
}

fn reference(file: &mut Facts, kind: EdgeKind, name: String) {
    file.references.push(Reference {
        source: file.definitions[0].node.id.clone(),
        scope: file.definitions[0].qualified_name.clone(),
        kind,
        target: Target::Name(name),
        binding: None,
    });
}

fn repeated(files: Vec<Facts>) -> Resolution {
    let run = |files: Vec<Facts>| {
        checked(move || {
            let result = resolve(&files).unwrap();
            (files, result)
        })
    };
    let (mut files, expected) = run(files);
    files.reverse();
    for file in &mut files {
        file.definitions.reverse();
        file.references.reverse();
    }
    let (mut files, reversed) = run(files);
    assert!(reversed == expected, "resolution changed after reversal");
    drop(reversed);
    let shift = usize::from(files.len() > 1);
    files.rotate_left(shift);
    let (_, rotated) = run(files);
    assert!(rotated == expected, "resolution changed after rotation");
    expected
}

fn ring(size: usize) {
    let mut files: Vec<_> = (0..size).map(|i| module(&format!("m{i:04}"))).collect();
    for (i, file) in files.iter_mut().enumerate() {
        reference(
            file,
            EdgeKind::Reexports,
            format!("crate::m{:04}::*", (i + 1) % size),
        );
    }
    reference(
        &mut files[0],
        EdgeKind::Calls,
        "crate::m0000::missing".into(),
    );
    let result = repeated(files);
    assert_eq!(result.edges.len(), size);
    for (i, edge) in result.edges.iter().enumerate() {
        assert_eq!(edge.kind, EdgeKind::Reexports);
        assert_eq!(edge.src.as_str(), format!("m:m{i:04}.rs"));
        assert_eq!(edge.dst.as_str(), format!("m:m{:04}.rs", (i + 1) % size));
    }
    assert_eq!(result.unresolved.len(), 1);
    assert_eq!(result.unresolved[0].reason, UnresolvedReason::Missing);
}

#[test]
fn self_import() {
    ring(1);
}

#[test]
fn mutual_imports() {
    ring(2);
}

#[test]
fn thousand_module_import_ring() {
    ring(1_000);
}

#[test]
fn hundred_thousand_edge_import_star() {
    const LEAVES: usize = 100_000;
    let mut hub = module("hub");
    hub.definitions[0].qualified_name.clear();
    let mut leaves = module("leaves");
    for i in 0..LEAVES {
        let name = format!("leaf{i:05}");
        let qualified = format!("leaves::{name}");
        reference(&mut hub, EdgeKind::Imports, format!("crate::{qualified}"));
        leaves
            .definitions
            .push(definition(&leaves.path, &name, qualified, NodeKind::Symbol));
    }
    let result = repeated(vec![hub, leaves]);
    assert!(result.unresolved.is_empty());
    assert_eq!(result.edges.len(), LEAVES);
    for (i, edge) in result.edges.iter().enumerate() {
        assert_eq!(edge.kind, EdgeKind::Imports);
        assert_eq!(edge.src.as_str(), "m:hub.rs");
        assert_eq!(edge.dst.as_str(), format!("s:leaves.rs#leaf{i:05}"));
    }
}

#[test]
fn same_symbol_in_ten_thousand_files() {
    const FILES: usize = 10_000;
    let mut files: Vec<_> = (0..FILES).map(|i| module(&format!("m{i:04}"))).collect();
    let candidates: Vec<_> = files
        .iter_mut()
        .map(|file| {
            let qualified = format!("{}::needle", file.definitions[0].qualified_name);
            let symbol = definition(&file.path, "needle", qualified, NodeKind::Symbol);
            let id = symbol.node.id.clone();
            file.definitions.push(symbol);
            id
        })
        .collect();
    let mut caller = module("caller");
    reference(&mut caller, EdgeKind::Calls, "needle".into());
    files.push(caller);
    let result = repeated(files);
    assert!(result.edges.is_empty());
    assert_eq!(result.unresolved.len(), 1);
    assert_eq!(
        result.unresolved[0].reason,
        UnresolvedReason::Ambiguous(candidates)
    );
}
