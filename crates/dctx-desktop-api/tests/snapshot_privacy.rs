//! Privacy regressions for desktop snapshot findings F01, F02, F03, F04 and F14.
//! Each refusal sits beside a positive control. `SourceSpy` records every list,
//! metadata and content read that reaches the confined source and fails the test
//! when a forbidden path is touched.

use std::fs::{self, File};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use dctx_desktop_api::{generate_snapshot, DesktopConfig, SourceSpy, MAX_RULE_FILE_BYTES};

fn scratch(label: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let path = std::env::temp_dir().join(format!("dctx-desktop-privacy-{label}-{n}-{nanos}"));
    fs::create_dir_all(&path).expect("temp");
    path
}

fn pack_files(output: &str) -> u64 {
    output
        .split("files=")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|number| number.parse().ok())
        .unwrap_or(u64::MAX)
}

fn assert_read(spy: &SourceSpy, path: &str) {
    assert!(spy.reads().iter().any(|read| read == path), "missing read {path}: {:?}", spy.paths());
}

fn assert_not_read(spy: &SourceSpy, path: &str) {
    assert!(!spy.reads().iter().any(|read| read == path), "read forbidden {path}: {:?}", spy.paths());
}

fn finish(root: &PathBuf) {
    let _ = fs::remove_dir_all(root);
}

/// F01: extension, hidden, empty and size filters must exclude content reads.
#[test]
fn desktop_filters_block_content_reads_and_keep_a_match() {
    let extensions = scratch("ext");
    fs::write(extensions.join("main.rs"), b"fn main() {}").expect("rs");
    fs::write(extensions.join("private.txt"), b"PRIVATE_SENTINEL").expect("txt");
    let mut only_rs = DesktopConfig::default();
    only_rs.filter_extensions = Some(vec!["rs".to_string()]);
    let spy = SourceSpy::arm();
    let allowed = generate_snapshot(&extensions, &only_rs);
    assert!(allowed.success, "{:?}", allowed.error);
    assert_eq!(pack_files(&allowed.output), 1, "{}", allowed.output);
    assert_read(&spy, "main.rs");
    assert!(!spy.touched("private.txt"), "filtered file was touched: {:?}", spy.paths());
    assert!(!allowed.output.contains("PRIVATE_SENTINEL"));
    drop(spy);
    finish(&extensions);

    let hidden = scratch("hidden");
    fs::write(hidden.join(".secret"), b"HIDDEN_OK").expect("secret");
    fs::write(hidden.join("private.txt"), b"PRIVATE_SENTINEL").expect("txt");
    let mut only_hidden = DesktopConfig::default();
    only_hidden.only_hidden = true;
    let spy = SourceSpy::arm();
    let allowed = generate_snapshot(&hidden, &only_hidden);
    assert!(allowed.success, "{:?}", allowed.error);
    assert_read(&spy, ".secret");
    assert!(!spy.touched("private.txt"), "{:?}", spy.paths());
    drop(spy);
    finish(&hidden);

    let empty = scratch("empty");
    fs::write(empty.join("keep.rs"), b"").expect("empty");
    fs::write(empty.join("private.txt"), b"PRIVATE_SENTINEL").expect("txt");
    let mut only_empty = DesktopConfig::default();
    only_empty.only_empty = true;
    let spy = SourceSpy::arm();
    let allowed = generate_snapshot(&empty, &only_empty);
    assert!(allowed.success, "{:?}", allowed.error);
    assert_read(&spy, "keep.rs");
    assert_not_read(&spy, "private.txt");
    drop(spy);
    finish(&empty);

    let sized = scratch("size");
    fs::write(sized.join("keep.rs"), b"ok").expect("keep");
    fs::write(sized.join("private.txt"), vec![b'x'; 200]).expect("big");
    let mut max_size = DesktopConfig::default();
    max_size.max_size_mb = Some(0.0001);
    let spy = SourceSpy::arm();
    let allowed = generate_snapshot(&sized, &max_size);
    assert!(allowed.success, "{:?}", allowed.error);
    assert_read(&spy, "keep.rs");
    assert_not_read(&spy, "private.txt");
    drop(spy);
    finish(&sized);

    let refused_root = scratch("none");
    fs::write(refused_root.join("private.txt"), b"PRIVATE_SENTINEL").expect("txt");
    let refused = generate_snapshot(&refused_root, &only_rs);
    assert_eq!(refused.error.as_deref(), Some("All files filtered out"));
    assert!(!refused.success);
    finish(&refused_root);
}

/// F02: `.gitignore` excludes and `.dctxomitcontent` omits, with an included control.
#[test]
fn project_ignore_and_omit_files_block_private_reads() {
    let ignored = scratch("gitignore");
    fs::write(ignored.join(".gitignore"), b"private.txt\n").expect("ignore");
    fs::write(ignored.join("keep.rs"), b"fn keep() {}\n").expect("keep");
    fs::write(ignored.join("private.txt"), b"PRIVATE_SENTINEL").expect("private");
    let spy = SourceSpy::arm();
    let response = generate_snapshot(&ignored, &DesktopConfig::default());
    assert!(response.success, "{:?}", response.error);
    assert_eq!(pack_files(&response.output), 2, "gitignore + keep.rs, not private: {}", response.output);
    assert_read(&spy, "keep.rs");
    assert!(!spy.touched("private.txt"), "{:?}", spy.paths());
    assert!(!response.output.contains("PRIVATE_SENTINEL"));
    drop(spy);
    finish(&ignored);

    let omitted = scratch("omit");
    fs::write(omitted.join(".dctxomitcontent"), b"private.txt\n").expect("omit");
    fs::write(omitted.join("keep.rs"), b"fn keep() {}\n").expect("keep");
    fs::write(omitted.join("private.txt"), b"PRIVATE_SENTINEL").expect("private");
    let spy = SourceSpy::arm();
    let response = generate_snapshot(&omitted, &DesktopConfig::default());
    assert!(response.success, "{:?}", response.error);
    assert_eq!(pack_files(&response.output), 3, "omit stays in the tree: {}", response.output);
    assert_read(&spy, "keep.rs");
    assert_not_read(&spy, "private.txt");
    assert!(!response.output.contains("PRIVATE_SENTINEL"));
    drop(spy);
    finish(&omitted);

    // `.gitignore` then `.dctxignore`: last match re-includes keep.rs only.
    let ordered = scratch("order");
    fs::write(ordered.join(".gitignore"), b"keep.rs\nprivate.txt\n").expect("ignore");
    fs::write(ordered.join(".dctxignore"), b"!keep.rs\n").expect("dctx");
    fs::write(ordered.join("keep.rs"), b"fn keep() {}\n").expect("keep");
    fs::write(ordered.join("private.txt"), b"PRIVATE_SENTINEL").expect("private");
    let spy = SourceSpy::arm();
    let response = generate_snapshot(&ordered, &DesktopConfig::default());
    assert!(response.success, "{:?}", response.error);
    assert_read(&spy, "keep.rs");
    assert!(!spy.touched("private.txt"), "{:?}", spy.paths());
    drop(spy);
    finish(&ordered);
}

/// F03: literal, wildcard and nested negations cannot list or read a `.git` path.
#[test]
fn git_negations_never_list_or_read_git() {
    assert_git_closed("literal", "!.git/\n", ".git/config");
    assert_git_closed("wildcard", "!.g*/\n", ".git/config");
    assert_git_closed("nested", "!nested/.git/\n!nested/.git/**\n", "nested/.git/config");
}

fn assert_git_closed(label: &str, rules: &str, secret: &str) {
    let root = scratch(label);
    if let Some(parent) = root.join(secret).parent() {
        fs::create_dir_all(parent).expect("parent");
    }
    fs::write(root.join(".dctxignore"), rules).expect("rules");
    fs::write(root.join("keep.txt"), b"keep").expect("keep");
    fs::write(root.join(secret), b"PRIVATE_SENTINEL").expect("secret");
    let spy = SourceSpy::arm();
    let response = generate_snapshot(&root, &DesktopConfig::default());
    assert!(response.success, "{label}: {:?}", response.error);
    assert_eq!(pack_files(&response.output), 1, "{label}: {}", response.output);
    assert_read(&spy, "keep.txt");
    assert!(!spy.git_touched(), "{label} touched .git: {:?}", spy.paths());
    assert!(!response.output.contains("PRIVATE_SENTINEL"));
    drop(spy);
    finish(&root);
}

/// F04: a symlinked or non-regular rule file is refused; a real in-root file still works.
#[test]
fn symlink_and_non_regular_rule_files_are_refused() {
    let outside = scratch("outside");
    fs::write(outside.join("rules"), b"keep.txt\n# OUTSIDE_SENTINEL\n").expect("outside");
    let linked = scratch("link");
    fs::write(linked.join("keep.txt"), b"VISIBLE_CONTROL").expect("keep");
    #[cfg(unix)]
    std::os::unix::fs::symlink(outside.join("rules"), linked.join(".dctxignore")).expect("symlink");
    #[cfg(unix)]
    {
        let spy = SourceSpy::arm();
        let refused = generate_snapshot(&linked, &DesktopConfig::default());
        assert!(!refused.success);
        let error = refused.error.unwrap_or_default();
        assert!(error.contains("symlink"), "{error}");
        assert!(error.contains(".dctxignore"), "{error}");
        assert!(!error.contains("OUTSIDE_SENTINEL"), "{error}");
        assert!(!spy.touched("keep.txt"), "symlink rule still scanned the project: {:?}", spy.paths());
        drop(spy);
    }
    finish(&linked);
    finish(&outside);

    let directory = scratch("dir-rule");
    fs::create_dir(directory.join(".dctxignore")).expect("dir");
    fs::write(directory.join("keep.txt"), b"VISIBLE_CONTROL").expect("keep");
    let refused = generate_snapshot(&directory, &DesktopConfig::default());
    assert!(!refused.success);
    let error = refused.error.unwrap_or_default();
    assert!(error.contains("not a regular file"), "{error}");
    assert!(!error.contains("VISIBLE_CONTROL"), "{error}");
    finish(&directory);

    let normal = scratch("normal-rule");
    fs::write(normal.join(".dctxignore"), b"secret.txt\n").expect("rules");
    fs::write(normal.join("keep.txt"), b"VISIBLE_CONTROL").expect("keep");
    fs::write(normal.join("secret.txt"), b"PRIVATE_SENTINEL").expect("secret");
    let spy = SourceSpy::arm();
    let allowed = generate_snapshot(&normal, &DesktopConfig::default());
    assert!(allowed.success, "{:?}", allowed.error);
    assert_read(&spy, "keep.txt");
    assert!(!spy.touched("secret.txt"), "{:?}", spy.paths());
    assert!(!allowed.output.contains("PRIVATE_SENTINEL"));
    drop(spy);
    finish(&normal);
}

/// F14: an oversized `.dctxignore` is refused after a bounded read; a small file still works.
#[test]
fn oversized_dctxignore_is_refused_after_a_bounded_read() {
    let huge = scratch("huge-rule");
    let file = File::create(huge.join(".dctxignore")).expect("create");
    file.set_len(MAX_RULE_FILE_BYTES + 1).expect("len");
    drop(file);
    fs::write(huge.join("keep.txt"), b"VISIBLE_CONTROL").expect("keep");
    let refused = generate_snapshot(&huge, &DesktopConfig::default());
    assert!(!refused.success);
    let error = refused.error.unwrap_or_default();
    assert!(error.contains("exceeds"), "{error}");
    assert!(error.contains(&MAX_RULE_FILE_BYTES.to_string()), "{error}");
    assert!(!error.contains("VISIBLE_CONTROL"), "{error}");
    finish(&huge);

    let small = scratch("small-rule");
    fs::write(small.join(".dctxignore"), b"secret.txt\n").expect("rules");
    fs::write(small.join("keep.txt"), b"VISIBLE_CONTROL").expect("keep");
    fs::write(small.join("secret.txt"), b"PRIVATE_SENTINEL").expect("secret");
    let spy = SourceSpy::arm();
    let allowed = generate_snapshot(&small, &DesktopConfig::default());
    assert!(allowed.success, "{:?}", allowed.error);
    assert_read(&spy, "keep.txt");
    assert!(!spy.touched("secret.txt"), "{:?}", spy.paths());
    drop(spy);
    finish(&small);
}
