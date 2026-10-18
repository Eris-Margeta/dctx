//! Public map verbs over temporary repositories covering the facade's new lexers.
//! Implements SPEC-MAP-MODE 4–6.2 and the selection rule that excluded bytes stay unread.
//! Uses FsSource and an explicit Session policy, following fs_fixture's construction.
//! All sources are inline; existing facade golden fixtures remain unchanged.

#![forbid(unsafe_code)]

#[path = "common/mod.rs"]
mod common;

use std::path::Path;
use std::sync::Arc;

use dctx::memory::CancelFlag;
use dctx::{
    FsSource, HostGrants, IdentityOutcome, MapRequest, RuleTexts, SelectionPolicy, Session,
    SnapshotRequest, StoragePolicy, VecSink, Verb,
};

struct Fixture {
    extensions: &'static [&'static str],
    source: &'static str,
    symbol: &'static str,
}

const FIXTURES: &[Fixture] = &[
    Fixture {
        extensions: &["py", "pyi"],
        source: "def PythonMain():\n    return 1\n",
        symbol: "PythonMain",
    },
    Fixture {
        extensions: &["go"],
        source: "package sample\nfunc GoMain() {}\n",
        symbol: "GoMain",
    },
    Fixture {
        extensions: &["java"],
        source: "public class JavaMain {}\n",
        symbol: "JavaMain",
    },
    Fixture {
        extensions: &["c", "h", "cc", "cpp", "cxx", "hpp", "hh"],
        source: "int CMain(void) { return 1; }\n",
        symbol: "CMain",
    },
    Fixture {
        extensions: &["json"],
        source: "{\"JsonMain\": 1}\n",
        symbol: "JsonMain",
    },
    Fixture {
        extensions: &["jsonc"],
        source: "// Configuration\n{\"JsoncMain\": 1,}\n",
        symbol: "JsoncMain",
    },
    Fixture {
        extensions: &["toml"],
        source: "TomlMain = 1\n",
        symbol: "TomlMain",
    },
    Fixture {
        extensions: &["yaml", "yml"],
        source: "YamlMain: 1\n",
        symbol: "YamlMain",
    },
    Fixture {
        extensions: &["cs"],
        source: "public class CSharpMain {}\n",
        symbol: "CSharpMain",
    },
    Fixture {
        extensions: &["kt", "kts"],
        source: "fun KotlinMain() {}\n",
        symbol: "KotlinMain",
    },
    Fixture {
        extensions: &["rb", "rake", "gemspec"],
        source: "def RubyMain\n  1\nend\n",
        symbol: "RubyMain",
    },
    Fixture {
        extensions: &["php", "phtml", "inc"],
        source: "<?php\nfunction PhpMain() {}\n",
        symbol: "PhpMain",
    },
    Fixture {
        extensions: &["swift"],
        source: "func SwiftMain() {}\n",
        symbol: "SwiftMain",
    },
];

fn write_sources(root: &Path) -> Vec<(String, &'static str)> {
    let mut files = Vec::new();
    for fixture in FIXTURES {
        for extension in fixture.extensions {
            let path = format!("sample.{extension}");
            std::fs::write(root.join(&path), fixture.source).expect("source");
            files.push((path, fixture.symbol));
        }
    }
    files
}

fn open(root: &Path, selection: SelectionPolicy) -> Session {
    common::session_parts(
        FsSource::open(root).expect("open"),
        Arc::new(CancelFlag::new()),
        HostGrants::PermitAll,
        StoragePolicy::Ephemeral { max_bytes: 8 * 1024 * 1024 },
        selection,
    )
}

#[test]
fn find_and_outline_cover_every_new_language_extension() {
    let root = tempfile::tempdir().expect("temp");
    let files = write_sources(root.path());
    std::fs::write(root.path().join("sample.unknown"), "def HiddenOpaque():\n    pass\n")
        .expect("opaque source");
    let mut session = open(root.path(), SelectionPolicy::from_texts(&RuleTexts::default()));
    let handle = session.snapshot(&SnapshotRequest::immutable()).expect("snapshot");
    let reads = session.content_reads();
    assert_eq!(reads, files.len() as u64 + 1);

    for (path, symbol) in &files {
        let id = format!("s:{path}#{symbol}");
        let mut request = MapRequest::new(Verb::Find);
        request.query = symbol.to_string();
        request.within = Some(path.clone());
        let found = session.query(&handle, &request, &mut VecSink::new()).expect("find");
        assert!(found.records.iter().any(|record| record.id == id && record.path == *path), "find {id}");
        let outline = session.map(&handle, &format!("m:{path}"), &mut VecSink::new()).expect("outline");
        assert!(outline.records.iter().any(|record| record.id == id), "outline {id}");
    }

    let opaque = session.map(&handle, "m:sample.unknown", &mut VecSink::new()).expect("opaque outline");
    assert!(opaque.outcome.is_none(), "the opaque file still has a module");
    assert!(opaque.records.is_empty());
    let hidden = session.find(&handle, "HiddenOpaque", &mut VecSink::new()).expect("opaque find");
    assert!(hidden.records.is_empty());
    assert_eq!(session.content_reads(), reads, "find and outline never reread content");
}

#[test]
fn selection_excludes_or_omits_new_languages_before_content_reads() {
    let root = tempfile::tempdir().expect("temp");
    let files = write_sources(root.path());
    std::fs::write(root.path().join("visible.py"), "def VisibleControl():\n    pass\n").expect("control");

    for rules in [
        RuleTexts { local_ignore: Some("sample.*\n"), ..RuleTexts::default() },
        RuleTexts { local_omit: Some("sample.*\n"), ..RuleTexts::default() },
    ] {
        let mut session = open(root.path(), SelectionPolicy::from_texts(&rules));
        let handle = session.snapshot(&SnapshotRequest::immutable()).expect("snapshot");
        assert_eq!(session.content_reads(), 1, "only the visible control may be read");
        let control = session.find(&handle, "VisibleControl", &mut VecSink::new()).expect("control");
        assert_eq!(control.records.len(), 1);
        assert_eq!(control.records[0].id, "s:visible.py#VisibleControl");
        for (path, symbol) in &files {
            let found = session.find(&handle, symbol, &mut VecSink::new()).expect("excluded find");
            assert!(found.records.is_empty(), "excluded {path}");
            let outline = session.map(&handle, &format!("m:{path}"), &mut VecSink::new()).expect("excluded outline");
            assert!(outline.records.is_empty(), "excluded outline {path}");
            assert!(matches!(outline.outcome, Some(IdentityOutcome::Gone { .. })));
        }
        assert_eq!(session.content_reads(), 1);
    }
}
