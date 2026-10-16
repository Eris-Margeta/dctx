//! Parity: dctx-pack's port-based scan builds the same tree as the legacy walker + ContentReader.
//! Uses a tiny test-only filesystem `Source` (this file is test code, not an adapter) over a temp
//! corpus with ignored, omitted, binary, SVG, nested and debug-visible items.
//! Depends on dctx-core's legacy modules, dctx-pack and dctx-ports. Run by `cargo test -p dctx-pack`.

use dctx_core::budget::PackLimits;
use dctx_core::content_reader::ContentReader;
use dctx_core::datatypes::{DirContextTreeNode, NodeType};
use dctx_core::error::DctxError;
use dctx_core::facts::{EntryMeta, ReadGrant, SourceEntry};
use dctx_core::ids::{RelativePath, Timestamp};
use dctx_core::ignore::IgnoreEngine;
use dctx_core::selection::{RuleTexts, SelectionPolicy};
use dctx_core::walker::walk_directory_structure;
use dctx_pack::{scan, ScanOptions};
use dctx_ports::memory::CancelFlag;
use dctx_ports::Source;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

struct FsSource {
    root: PathBuf,
}

impl FsSource {
    fn at(&self, path: &RelativePath) -> PathBuf {
        if path.is_root() {
            self.root.clone()
        } else {
            self.root.join(path.as_str())
        }
    }
}

impl Source for FsSource {
    fn root_kind(&self) -> Result<NodeType, DctxError> {
        let meta = fs::metadata(&self.root).map_err(|_| DctxError::SourceUnavailable)?;
        Ok(if meta.is_dir() { NodeType::Directory } else { NodeType::File })
    }

    fn entries(&self, dir: &RelativePath) -> Result<Vec<SourceEntry>, DctxError> {
        let mut out = Vec::new();
        for entry in fs::read_dir(self.at(dir)).map_err(DctxError::io)?.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            out.push(if is_dir { SourceEntry::directory(&name) } else { SourceEntry::file(&name) });
        }
        Ok(out)
    }

    fn metadata(&self, path: &RelativePath) -> Result<EntryMeta, DctxError> {
        let meta = fs::metadata(self.at(path)).map_err(DctxError::io)?;
        let modified = meta
            .modified()
            .map(|t| t.duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64)
            .unwrap_or(0);
        Ok(EntryMeta { size: meta.len(), modified: Timestamp(modified) })
    }

    fn read(&self, grant: &ReadGrant, _max_bytes: u64) -> Result<Vec<u8>, DctxError> {
        fs::read(self.at(grant.path())).map_err(DctxError::io)
    }
}

fn write(root: &Path, path: &str, bytes: &[u8]) {
    let full = root.join(path);
    fs::create_dir_all(full.parent().unwrap()).unwrap();
    fs::write(full, bytes).unwrap();
}

fn corpus() -> tempfile::TempDir {
    let dir = tempfile::TempDir::new().unwrap();
    let root = dir.path();
    write(root, "src/main.rs", b"fn main() {}\n");
    write(root, "src/nested/deep/util.rs", b"pub fn util() {}\n");
    write(root, "README.md", b"# corpus\n");
    write(root, "Cargo.lock", b"# lock\n");
    write(root, "assets/logo.png", &[0x89, b'P', b'N', b'G', 0, 0, 1]);
    write(root, "assets/icon.svg", b"<svg/>");
    write(root, ".env", b"SECRET=1\n");
    write(root, "node_modules/pkg/index.js", b"module.exports = 1;\n");
    write(root, "private/keep/notes.txt", b"private\n");
    write(root, "empty.txt", b"");
    dir
}

fn policy() -> SelectionPolicy {
    SelectionPolicy::from_texts(&RuleTexts {
        local_ignore: Some(".env\nprivate/\n!private/keep/\n"),
        ..RuleTexts::default()
    })
}

fn legacy(root: &Path, debug: bool) -> DirContextTreeNode {
    let engine = IgnoreEngine::from_policy(policy(), false);
    let mut tree = walk_directory_structure(root, &engine, debug).unwrap();
    ContentReader::populate(&mut tree);
    tree
}

fn ported(root: &Path, debug: bool) -> DirContextTreeNode {
    let source = FsSource { root: root.to_path_buf() };
    let policy = policy();
    let options = ScanOptions { policy: &policy, debug_ignored: debug, limits: PackLimits::unbounded() };
    scan(&source, &CancelFlag::new(), options).unwrap().tree
}

fn assert_same(a: &DirContextTreeNode, b: &DirContextTreeNode) {
    assert_eq!(a.relative_path, b.relative_path);
    assert_eq!(a.node_type, b.node_type, "{}", a.relative_path);
    assert_eq!(a.handling_decision, b.handling_decision, "{}", a.relative_path);
    assert_eq!(a.is_ignored, b.is_ignored, "{}", a.relative_path);
    assert_eq!(a.content_size, b.content_size, "{}", a.relative_path);
    assert_eq!(a.last_modified_timestamp, b.last_modified_timestamp, "{}", a.relative_path);
    assert_eq!(a.file_content, b.file_content, "{}", a.relative_path);
    assert_eq!(a.is_binary_file, b.is_binary_file, "{}", a.relative_path);
    assert_eq!(a.binary_file_type, b.binary_file_type, "{}", a.relative_path);
    let names = |n: &DirContextTreeNode| n.children.iter().map(|c| c.relative_path.clone()).collect::<Vec<_>>();
    assert_eq!(names(a), names(b), "children of {:?}", a.relative_path);
    for (x, y) in a.children.iter().zip(&b.children) {
        assert_same(x, y);
    }
}

#[test]
fn ported_scan_matches_legacy_walk_and_content() {
    let dir = corpus();
    let legacy_tree = legacy(dir.path(), false);
    assert_same(&legacy_tree, &ported(dir.path(), false));
    assert!(legacy_tree.children.iter().all(|c| c.relative_path != ".env"));
}

#[test]
fn ported_debug_scan_matches_legacy_debug_walk() {
    let dir = corpus();
    let legacy_tree = legacy(dir.path(), true);
    assert_same(&legacy_tree, &ported(dir.path(), true));
    let private = legacy_tree.children.iter().find(|c| c.relative_path == "private").unwrap();
    assert!(private.is_ignored);
    assert!(private.children[0].children[0].file_content.is_none());
}
