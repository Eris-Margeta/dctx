//! Pinned facts for the legacy_parity corpus (DX-D3-01, D3-LEGACY-REMOVAL.md §3 and §5).
//! `fixtures/` records the tree the legacy walker and content reader store when every
//! timestamp is the injected value below. This test never calls a legacy module.
//! Depends on dctx-pack, dctx-core's selection policy and dctx-ports' MemorySource.
//! Run by `cargo test -p dctx-pack --test compatibility`.

use dctx_core::budget::PackLimits;
use dctx_core::datatypes::{DirContextTreeNode, NodeType};
use dctx_core::selection::{RuleTexts, SelectionPolicy};
use dctx_pack::{scan, Scan, ScanOptions};
use dctx_ports::memory::{CancelFlag, MemorySource};

const NORMAL: &str = include_str!("fixtures/legacy-normal.txt");
const DEBUG_FACTS: &str = include_str!("fixtures/legacy-debug.txt");
const PNG_HEX: &str = include_str!("fixtures/legacy-binary.hex");

struct Oracle {
    entries: u64,
    content_bytes: u64,
    diagnostics: usize,
    reads: Vec<String>,
    not_read: Vec<String>,
    untouched: Vec<String>,
    nodes: Vec<String>,
}

struct Fact {
    depth: usize,
    path: String,
    kind: String,
    decision: String,
    ignored: bool,
    size: u64,
    mtime: u64,
    binary: bool,
    binary_type: Option<String>,
    content: Option<Vec<u8>>,
}

fn decode_hex(text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for line in text.lines() {
        let code = line.split('#').next().unwrap_or("");
        for token in code.split_whitespace() {
            let byte = u8::from_str_radix(token, 16).unwrap_or_else(|_| panic!("bad hex {token}"));
            out.push(byte);
        }
    }
    out
}

fn corpus(png: &[u8]) -> MemorySource {
    let mut source = MemorySource::new();
    source.insert_file("src/main.rs", b"fn main() {}\n", 1_700_000_000_400).unwrap();
    source.insert_file("src/nested/deep/util.rs", b"pub fn util() {}\n", 1_700_000_000_500).unwrap();
    source.insert_file("README.md", b"# corpus\n", 1_700_000_000_600).unwrap();
    source.insert_file("Cargo.lock", b"# lock\n", 1_700_000_000_700).unwrap();
    source.insert_file("assets/logo.png", png, 1_700_000_000_900).unwrap();
    source.insert_file("assets/icon.svg", b"<svg/>", 1_700_000_001_000).unwrap();
    source.insert_file(".env", b"SECRET=1\n", 1_700_000_001_100).unwrap();
    source.insert_file("node_modules/pkg/index.js", b"module.exports = 1;\n", 1_700_000_001_400).unwrap();
    source.insert_file("private/keep/notes.txt", b"private\n", 1_700_000_001_700).unwrap();
    source.insert_file("empty.txt", b"", 1_700_000_001_800).unwrap();
    let dirs: &[(&str, u64)] = &[
        ("", 1_700_000_000_000),
        ("src", 1_700_000_000_100),
        ("src/nested", 1_700_000_000_200),
        ("src/nested/deep", 1_700_000_000_300),
        ("assets", 1_700_000_000_800),
        ("node_modules", 1_700_000_001_200),
        ("node_modules/pkg", 1_700_000_001_300),
        ("private", 1_700_000_001_500),
        ("private/keep", 1_700_000_001_600),
    ];
    for (path, modified) in dirs {
        source.insert_dir(path, *modified).expect(path);
    }
    source
}

fn policy() -> SelectionPolicy {
    SelectionPolicy::from_texts(&RuleTexts {
        local_ignore: Some(".env\nprivate/\n!private/keep/\n"),
        ..RuleTexts::default()
    })
}

fn run(debug: bool) -> (MemorySource, Scan) {
    let png = decode_hex(PNG_HEX);
    assert_eq!(png, [0x89, b'P', b'N', b'G', 0, 0, 1], "legacy-binary.hex");
    let source = corpus(&png);
    let rules = policy();
    let options = ScanOptions { policy: &rules, debug_ignored: debug, limits: PackLimits::unbounded() };
    let scanned = scan(&source, &CancelFlag::new(), options).expect("scan");
    (source, scanned)
}

fn load(text: &str) -> Oracle {
    let mut oracle = Oracle {
        entries: 0,
        content_bytes: 0,
        diagnostics: 0,
        reads: Vec::new(),
        not_read: Vec::new(),
        untouched: Vec::new(),
        nodes: Vec::new(),
    };
    let mut section = "";
    for line in text.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("ENTRIES ") {
            oracle.entries = rest.parse().expect("entries");
            continue;
        }
        if let Some(rest) = line.strip_prefix("CONTENT_BYTES ") {
            oracle.content_bytes = rest.parse().expect("bytes");
            continue;
        }
        if let Some(rest) = line.strip_prefix("DIAGNOSTICS ") {
            oracle.diagnostics = rest.parse().expect("diagnostics");
            continue;
        }
        if matches!(line, "READS" | "NOT_READ" | "UNTOUCHED" | "NODES") {
            section = line;
            continue;
        }
        match section {
            "READS" => oracle.reads.push(line.to_string()),
            "NOT_READ" => oracle.not_read.push(line.to_string()),
            "UNTOUCHED" => oracle.untouched.push(line.to_string()),
            "NODES" => oracle.nodes.push(line.to_string()),
            _ => panic!("line outside a section: {line}"),
        }
    }
    oracle.reads.sort();
    oracle
}

fn unescape(text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            let mut buf = [0u8; 4];
            out.extend(ch.encode_utf8(&mut buf).as_bytes());
            continue;
        }
        match chars.next() {
            Some('n') => out.push(b'\n'),
            Some('\\') => out.push(b'\\'),
            Some('"') => out.push(b'"'),
            Some(other) => panic!("bad escape {other}"),
            None => panic!("truncated escape"),
        }
    }
    out
}

fn parse_content(field: &str) -> Option<Vec<u8>> {
    if field == "-" {
        return None;
    }
    assert!(field.len() >= 2 && field.starts_with('"') && field.ends_with('"'), "{field}");
    Some(unescape(&field[1..field.len() - 1]))
}

fn parse_fact(line: &str) -> Fact {
    let field: Vec<&str> = line.split('\t').collect();
    assert_eq!(field.len(), 10, "fact columns: {line}");
    Fact {
        depth: field[0].parse().expect("depth"),
        path: field[1].to_string(),
        kind: field[2].to_string(),
        decision: field[3].to_string(),
        ignored: field[4] == "true",
        size: field[5].parse().expect("size"),
        mtime: field[6].parse().expect("mtime"),
        binary: field[7] == "true",
        binary_type: (field[8] != "-").then(|| field[8].to_string()),
        content: parse_content(field[9]),
    }
}

fn check_node(node: &DirContextTreeNode, lines: &[String], index: &mut usize, depth: usize) {
    assert!(*index < lines.len(), "tree longer than the fixture at depth {depth}");
    let fact = parse_fact(&lines[*index]);
    *index += 1;
    let path = if node.relative_path.is_empty() { "." } else { node.relative_path.as_str() };
    assert_eq!(fact.depth, depth, "{path}");
    assert_eq!(fact.path, path);
    let kind = match node.node_type {
        NodeType::Directory => "dir",
        NodeType::File => "file",
    };
    assert_eq!(fact.kind, kind, "{path}");
    assert_eq!(fact.decision, format!("{:?}", node.handling_decision), "{path}");
    assert_eq!(fact.ignored, node.is_ignored, "{path}");
    assert_eq!(fact.size, node.content_size, "{path}");
    assert_eq!(fact.mtime, node.last_modified_timestamp, "{path}");
    assert_eq!(fact.binary, node.is_binary_file, "{path}");
    assert_eq!(fact.binary_type, node.binary_file_type, "{path}");
    assert_eq!(fact.content, node.file_content, "{path}");
    for child in &node.children {
        check_node(child, lines, index, depth + 1);
    }
}

fn assert_oracle(debug: bool, text: &str) {
    let (source, scanned) = run(debug);
    let oracle = load(text);
    assert_eq!(scanned.entries, oracle.entries);
    assert_eq!(scanned.content_bytes, oracle.content_bytes);
    assert_eq!(scanned.diagnostics.len(), oracle.diagnostics);
    let mut got = source.reads();
    got.sort();
    assert_eq!(got, oracle.reads, "files whose bytes were read");
    for path in &oracle.not_read {
        assert!(!got.iter().any(|read| read == path), "{path} was read");
    }
    for path in &oracle.untouched {
        assert!(!source.touched(path), "{path} was touched");
    }
    let mut index = 0;
    check_node(&scanned.tree, &oracle.nodes, &mut index, 0);
    assert_eq!(index, oracle.nodes.len(), "unread fixture lines");
}

#[test]
fn normal_scan_matches_pinned_legacy_facts() {
    assert_oracle(false, NORMAL);
    let (_, scanned) = run(false);
    assert!(scanned.tree.children.iter().all(|child| child.relative_path != ".env"));
    let readme = scanned.tree.children.iter().find(|child| child.relative_path == "README.md").unwrap();
    assert_eq!(readme.file_content.as_deref(), Some(&b"# corpus\n"[..]));
    let assets = scanned.tree.children.iter().find(|child| child.relative_path == "assets").unwrap();
    assert_eq!(assets.children[0].relative_path, "assets/icon.svg");
    assert_eq!(assets.children[1].relative_path, "assets/logo.png");
    assert!(assets.children[1].is_binary_file);
    assert!(assets.children[1].file_content.is_none());
}

#[test]
fn debug_scan_matches_pinned_legacy_facts() {
    assert_oracle(true, DEBUG_FACTS);
    let (_, scanned) = run(true);
    let private = scanned.tree.children.iter().find(|child| child.relative_path == "private").unwrap();
    assert!(private.is_ignored);
    assert_eq!(private.handling_decision, dctx_core::domain::FileHandlingDecision::DebugVisible);
    assert_eq!(private.children[0].relative_path, "private/keep");
    assert!(private.children[0].is_ignored);
    assert!(private.children[0].children[0].file_content.is_none());
    assert_eq!(private.children[0].children[0].relative_path, "private/keep/notes.txt");
}
