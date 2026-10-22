//! Filesystem rule-loading evidence for D3-LEGACY-REMOVAL §3/§5 D3-07.
//! Uses public `FsSource` with temp roots and an injected `SourceFs` error control.
//! Complements core `selection_legacy/selection_omit_legacy` without legacy imports.
//!
//! Legacy coverage: `engine_build_success/loads_hardcoded/loads_global/loads_local`
//! map to the loading tests below; `error_display` maps to typed Rule diagnostics.
//! Legacy omit loader's unreadable-file fallback is retired: adapter rule IO is
//! deliberately fail-closed (`SourceError::Rule`), tested with readable controls.
//! Global files are caller-supplied `ignore`/`omitcontent`; no home discovery.

#![forbid(unsafe_code)]

use dctx_adapter_fs::{
    DirEntry, EntryMeta, FileBody, FileHandlingDecision as Decision, FsSource, SourceError, SourceFs, SourceOptions,
    StdFs,
};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

fn options(global: &Path) -> SourceOptions {
    SourceOptions { global_dir: Some(global.to_path_buf()), ..SourceOptions::default() }
}

fn write(root: &Path, name: &str, text: &str) {
    fs::write(root.join(name), text).unwrap();
}

fn included<F: SourceFs>(source: &FsSource<F>, name: &str) {
    let files = source.inventory().unwrap();
    let file = files.iter().find(|file| file.relative == name).unwrap();
    assert_eq!(file.decision, Decision::Include);
    assert!(matches!(file.body, FileBody::Digested { .. }));
    assert_eq!(source.read(name).unwrap().bytes, b"public");
}

fn excluded<F: SourceFs>(source: &FsSource<F>, name: &str, denied_at: &str) {
    assert!(source.inventory().unwrap().iter().all(|file| file.relative != name));
    assert!(matches!(source.read(name), Err(SourceError::Denied { relative }) if relative == denied_at));
    included(source, "keep.txt");
}

fn omitted<F: SourceFs>(source: &FsSource<F>, name: &str) {
    let files = source.inventory().unwrap();
    let file = files.iter().find(|file| file.relative == name).unwrap();
    assert_eq!(file.decision, Decision::OmitContent);
    assert!(matches!(file.body, FileBody::Metadata { .. }));
    assert!(matches!(source.read(name), Err(SourceError::Denied { relative }) if relative == name));
    included(source, "keep.txt");
}

#[test]
fn missing_global_and_project_files_keep_builtins_and_normal_content() {
    let root = tempfile::tempdir().unwrap();
    let global = tempfile::tempdir().unwrap();
    write(root.path(), "keep.txt", "public");
    write(root.path(), "Cargo.lock", "lock");
    fs::create_dir(root.path().join("node_modules")).unwrap();
    write(&root.path().join("node_modules"), "secret.txt", "secret");
    let source = FsSource::open(root.path(), options(global.path())).unwrap();
    included(&source, "keep.txt");
    omitted(&source, "Cargo.lock");
    excluded(&source, "node_modules/secret.txt", "node_modules");
    for name in [".gitignore", ".dctxignore", ".legacyignore", ".dctxomitcontent"] {
        assert!(!root.path().join(name).exists());
    }
    assert!(!global.path().join("ignore").exists());
    assert!(!global.path().join("omitcontent").exists());
}

#[test]
fn global_ignore_file_loads_only_when_explicitly_selected() {
    let root = tempfile::tempdir().unwrap();
    let global = tempfile::tempdir().unwrap();
    write(root.path(), "keep.txt", "public");
    write(root.path(), "private.txt", "public");
    write(global.path(), "ignore", "# comment\n\nprivate.txt\n");
    let source = FsSource::open(root.path(), options(global.path())).unwrap();
    excluded(&source, "private.txt", "private.txt");
    let source = FsSource::open(root.path(), SourceOptions::default()).unwrap();
    included(&source, "private.txt");
}

#[test]
fn project_ignore_files_load_in_order() {
    let root = tempfile::tempdir().unwrap();
    let global = tempfile::tempdir().unwrap();
    for name in ["keep.txt", "global.txt", "git.txt", "local.txt", "legacy.txt"] {
        write(root.path(), name, "public");
    }
    write(global.path(), "ignore", "global.txt\nkeep.txt\n");
    write(root.path(), ".gitignore", "git.txt\n!global.txt\n");
    write(root.path(), ".dctxignore", "local.txt\n!git.txt\n");
    write(root.path(), ".legacyignore", "legacy.txt\n!local.txt\n!keep.txt\n");
    let source = FsSource::open(root.path(), options(global.path())).unwrap();
    excluded(&source, "legacy.txt", "legacy.txt");
    for name in ["global.txt", "git.txt", "local.txt", "keep.txt"] {
        included(&source, name);
    }
}

#[test]
fn project_ignore_patterns_and_negations_load_without_global_files() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "keep.txt", "public");
    write(root.path(), "test_private.txt", "secret");
    fs::create_dir(root.path().join("custom_pattern")).unwrap();
    write(&root.path().join("custom_pattern"), "secret.txt", "secret");
    write(root.path(), ".dctxignore", "custom_pattern/\ntest_*.txt\n!keep.txt\n");
    let source = FsSource::open(root.path(), SourceOptions::default()).unwrap();
    excluded(&source, "test_private.txt", "test_private.txt");
    excluded(&source, "custom_pattern/secret.txt", "custom_pattern");
}

#[test]
fn global_omit_file_loads_and_can_negate_a_builtin() {
    let root = tempfile::tempdir().unwrap();
    let global = tempfile::tempdir().unwrap();
    write(root.path(), "keep.txt", "public");
    write(root.path(), "private.data", "secret");
    write(root.path(), "Cargo.lock", "public");
    write(global.path(), "omitcontent", "*.data\n!Cargo.lock\n");
    let source = FsSource::open(root.path(), options(global.path())).unwrap();
    omitted(&source, "private.data");
    included(&source, "Cargo.lock");
    let source = FsSource::open(root.path(), SourceOptions::default()).unwrap();
    omitted(&source, "Cargo.lock");
}

#[test]
fn project_omit_file_overrides_global_and_ignore_still_wins() {
    let root = tempfile::tempdir().unwrap();
    let global = tempfile::tempdir().unwrap();
    for name in ["keep.txt", "keep.data", "private.data", "hidden.data", "Cargo.lock"] {
        write(root.path(), name, "public");
    }
    write(global.path(), "omitcontent", "*.data\n!Cargo.lock\n");
    write(root.path(), ".dctxomitcontent", "!keep.data\nCargo.lock\n");
    write(root.path(), ".dctxignore", "hidden.data\n");
    let source = FsSource::open(root.path(), options(global.path())).unwrap();
    omitted(&source, "private.data");
    omitted(&source, "Cargo.lock");
    included(&source, "keep.data");
    excluded(&source, "hidden.data", "hidden.data");
}

struct RuleErrorFs {
    path: PathBuf,
    kind: io::ErrorKind,
}

impl SourceFs for RuleErrorFs {
    fn metadata(&self, path: &Path) -> io::Result<EntryMeta> {
        StdFs.metadata(path)
    }
    fn read_dir(&self, path: &Path) -> io::Result<Vec<DirEntry>> {
        StdFs.read_dir(path)
    }
    fn read_link(&self, path: &Path) -> io::Result<PathBuf> {
        StdFs.read_link(path)
    }
    fn read_file(&self, path: &Path) -> io::Result<Vec<u8>> {
        StdFs.read_file(path)
    }
    fn read_policy(&self, path: &Path) -> io::Result<Option<String>> {
        if path == self.path {
            return Err(io::Error::new(self.kind, "injected rule IO"));
        }
        StdFs.read_policy(path)
    }
}

#[test]
fn unreadable_global_and_project_ignore_and_omit_files_refuse_with_path() {
    let root = tempfile::tempdir().unwrap();
    let global = tempfile::tempdir().unwrap();
    write(root.path(), "keep.txt", "public");
    let paths = [
        global.path().join("ignore"),
        global.path().join("omitcontent"),
        root.path().join(".gitignore"),
        root.path().join(".dctxignore"),
        root.path().join(".legacyignore"),
        root.path().join(".dctxomitcontent"),
    ];
    for path in paths {
        fs::write(&path, "# readable control\n").unwrap();
        let source = FsSource::open(root.path(), options(global.path())).unwrap();
        included(&source, "keep.txt");
        let result = FsSource::load(
            root.path().to_path_buf(),
            options(global.path()),
            RuleErrorFs { path: path.clone(), kind: io::ErrorKind::PermissionDenied },
        );
        let error = result.err().unwrap();
        assert!(error.to_string().contains("rule file unreadable"));
        assert!(error.to_string().contains(&path.display().to_string()));
        assert!(matches!(error, SourceError::Rule { path: failed, source }
            if failed == path && source.kind() == io::ErrorKind::PermissionDenied));
        let source = FsSource::open(root.path(), options(global.path())).unwrap();
        included(&source, "keep.txt");
    }
}

#[test]
fn not_found_from_the_port_is_absence_for_every_rule_file() {
    let root = tempfile::tempdir().unwrap();
    let global = tempfile::tempdir().unwrap();
    write(root.path(), "keep.txt", "public");
    for path in [
        global.path().join("ignore"),
        global.path().join("omitcontent"),
        root.path().join(".gitignore"),
        root.path().join(".dctxignore"),
        root.path().join(".legacyignore"),
        root.path().join(".dctxomitcontent"),
    ] {
        let source = FsSource::load(
            root.path().to_path_buf(),
            options(global.path()),
            RuleErrorFs { path, kind: io::ErrorKind::NotFound },
        )
        .unwrap();
        included(&source, "keep.txt");
    }
}

#[test]
fn invalid_utf8_rule_files_refuse_instead_of_silently_using_defaults() {
    let root = tempfile::tempdir().unwrap();
    let global = tempfile::tempdir().unwrap();
    write(root.path(), "keep.txt", "public");
    for path in [
        global.path().join("ignore"),
        global.path().join("omitcontent"),
        root.path().join(".dctxignore"),
        root.path().join(".dctxomitcontent"),
    ] {
        fs::write(&path, [0xff]).unwrap();
        let error = FsSource::open(root.path(), options(global.path())).err().unwrap();
        assert!(matches!(error, SourceError::Rule { path: failed, source }
            if failed == path && source.kind() == io::ErrorKind::InvalidData));
        fs::write(&path, "# valid UTF-8\n").unwrap();
        let source = FsSource::open(root.path(), options(global.path())).unwrap();
        included(&source, "keep.txt");
    }
}
