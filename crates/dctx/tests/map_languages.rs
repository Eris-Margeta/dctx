//! Public map verbs over temporary repositories covering the facade's new lexers.
//! Implements SPEC-MAP-MODE 4–6.2 and the selection rule that excluded bytes stay unread.
//! Uses `FsSource` and an explicit Session policy, following `fs_fixture`'s construction.
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
    assert_eq!(opaque.records, [] as [dctx::AnswerRecord; 0]);
    let hidden = session.find(&handle, "HiddenOpaque", &mut VecSink::new()).expect("opaque find");
    assert_eq!(hidden.records, [] as [dctx::AnswerRecord; 0]);
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

/// Same extension list as `dctx-map-parse` `facts.rs` / `lexers_extensions.rs`
/// and `index/extract.rs`. Basenames are the Ruby lexer set.
const SHARED_EXTENSIONS: &[&str] = &[
    "rs", "js", "mjs", "cjs", "jsx", "ts", "mts", "cts", "tsx", "md", "markdown", "py", "pyi", "go", "java", "c", "h",
    "cc", "cpp", "cxx", "hpp", "hh", "json", "jsonc", "toml", "yaml", "yml", "cs", "kt", "kts", "rb", "rake", "gemspec",
    "php", "phtml", "inc", "swift",
];
const SHARED_RUBY_BASENAMES: &[&str] = &["Rakefile", "Gemfile"];

#[test]
fn extension_case_and_ruby_basenames_match_the_shared_literal() {
    fn at(session: &mut Session, handle: &dctx::SnapshotHandle, symbol: &str, path: &str) -> dctx::AnswerRecord {
        let mut request = MapRequest::new(Verb::Find);
        request.query = symbol.to_string();
        request.within = Some(path.to_string());
        let found = session.query(handle, &request, &mut VecSink::new()).unwrap_or_else(|error| panic!("{path}: {error}"));
        found.records.into_iter().find(|record| record.path == path).unwrap_or_else(|| panic!("{path} has no {symbol}"))
    }
    // Rust, JavaScript and Markdown have no row in `FIXTURES`. The facade unit
    // test covers those extensions. Every other shared extension must.
    const WITHOUT_SESSION_FIXTURE: &[&str] = &[
        "rs", "js", "mjs", "cjs", "jsx", "ts", "mts", "cts", "tsx", "md", "markdown",
    ];
    let root = tempfile::tempdir().expect("temp");
    std::fs::create_dir_all(root.path().join("lower")).expect("dir");
    std::fs::create_dir_all(root.path().join("upper")).expect("dir");
    std::fs::create_dir_all(root.path().join("nested")).expect("dir");
    let mut pairs = Vec::new();
    for extension in SHARED_EXTENSIONS {
        if WITHOUT_SESSION_FIXTURE.contains(extension) {
            continue;
        }
        let fixture = FIXTURES
            .iter()
            .find(|item| item.extensions.contains(extension))
            .unwrap_or_else(|| panic!("shared extension {extension} has no fixture"));
        let lower = format!("lower/sample.{extension}");
        let upper = format!("upper/sample.{}", extension.to_ascii_uppercase());
        std::fs::write(root.path().join(&lower), fixture.source).expect("lower");
        std::fs::write(root.path().join(&upper), fixture.source).expect("upper");
        pairs.push((lower, upper, fixture.symbol));
    }
    let ruby = FIXTURES.iter().find(|item| item.symbol == "RubyMain").expect("ruby fixture");
    let mut basenames = Vec::new();
    for basename in SHARED_RUBY_BASENAMES {
        let canonical = basename.to_string();
        let folded = format!("nested/{}", basename.to_ascii_lowercase());
        std::fs::write(root.path().join(&canonical), ruby.source).expect("basename");
        std::fs::write(root.path().join(&folded), ruby.source).expect("folded basename");
        basenames.push(canonical);
        basenames.push(folded);
    }
    assert!(pairs.iter().any(|(lower, _, symbol)| lower == "lower/sample.rb" && *symbol == "RubyMain"));
    // Not a defect: the Ruby lexer does not accept these basenames.
    std::fs::write(root.path().join("Guardfile"), "def GuardHidden\nend\n").expect("guard");
    std::fs::write(root.path().join("config.ru"), "def ConfigHidden\nend\n").expect("ru");

    let mut session = open(root.path(), SelectionPolicy::from_texts(&RuleTexts::default()));
    let handle = session.snapshot(&SnapshotRequest::immutable()).expect("snapshot");



    for (lower, upper, symbol) in &pairs {
        let control = at(&mut session, &handle, symbol, lower);
        let folded = at(&mut session, &handle, symbol, upper);
        assert_eq!(control.id, format!("s:{lower}#{symbol}"));
        assert_eq!(folded.id, format!("s:{upper}#{symbol}"));
        assert_eq!(folded.kind, control.kind);
        assert_eq!(folded.lines, control.lines);
        assert_eq!(folded.summary, control.summary);
        assert_eq!(folded.summary_source, control.summary_source);
        if *symbol == "RubyMain" && lower == "lower/sample.rb" {
            assert_eq!(control.kind, "fn");
            assert_eq!(control.lines, [1, 3]);
            assert_eq!(control.summary, "def RubyMain");
            assert_eq!(control.summary_source, "sig");
        }
        for path in [lower, upper] {
            let outline = session.map(&handle, &format!("m:{path}"), &mut VecSink::new()).expect("outline");
            assert!(outline.records.iter().any(|item| item.id == format!("s:{path}#{symbol}")));
        }
    }
    let control = at(&mut session, &handle, "RubyMain", "lower/sample.rb");
    for path in &basenames {
        let record = at(&mut session, &handle, "RubyMain", path);
        assert_eq!(record.id, format!("s:{path}#RubyMain"));
        assert_eq!(record.kind, control.kind);
        assert_eq!(record.lines, control.lines);
        assert_eq!(record.summary, control.summary);
        assert_eq!(record.summary_source, control.summary_source);
    }
    for hidden in ["GuardHidden", "ConfigHidden"] {
        let found = session.find(&handle, hidden, &mut VecSink::new()).expect("opaque find");
        assert!(found.records.is_empty(), "{hidden}");
    }
}
