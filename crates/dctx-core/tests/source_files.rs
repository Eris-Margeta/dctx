// SPDX-License-Identifier: Apache-2.0
//! Regression controls for source-policy discovery, independent of live scratch trees.

#[path = "../../../tests/support/source_files.rs"]
mod source_files;

use std::fs;
use std::path::Path;
use std::process::Command;

fn git(root: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .is_ok_and(|output| output.status.success())
}

#[test]
fn tracked_working_sources_survive_untracked_directory_churn() {
    let dir = tempfile::tempdir().unwrap();
    if !git(dir.path(), &["init", "--quiet"]) {
        return; // The archive fallback is covered separately when Git is unavailable.
    }
    let src = dir.path().join("src");
    fs::create_dir(&src).unwrap();
    let name = if cfg!(windows) {
        "tracked name.rs"
    } else {
        "tracked name\n.rs"
    };
    let tracked = src.join(name);
    fs::write(&tracked, "original").unwrap();
    assert!(git(dir.path(), &["add", "src"]));
    fs::write(&tracked, "edited working source").unwrap();
    fs::write(src.join("untracked.rs"), "std::fs::read()").unwrap();
    std::thread::scope(|scope| {
        scope.spawn(|| {
            for _ in 0..100 {
                let scratch = src.join("disposable");
                fs::create_dir(&scratch).unwrap();
                fs::write(scratch.join("forbidden.rs"), "std::fs::read()").unwrap();
                fs::remove_dir_all(scratch).unwrap();
            }
        });
        for _ in 0..20 {
            assert_eq!(
                source_files::files(&src).as_slice(),
                std::slice::from_ref(&tracked)
            );
            assert_eq!(
                fs::read_to_string(&tracked).unwrap(),
                "edited working source"
            );
        }
    });
    fs::remove_file(&tracked).unwrap();
    assert!(source_files::files(&src).is_empty());
}

#[test]
fn archive_fallback_keeps_nested_sources_and_ignores_nonregular_entries() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::create_dir(root.join("nested")).unwrap();
    let source = root.join("nested/source.rs");
    fs::write(&source, "std::fs::read()").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(root, root.join("loop")).unwrap();
    assert_eq!(source_files::files(root), [source]);
    let vanished = root.join("removed");
    fs::create_dir(&vanished).unwrap();
    fs::remove_dir(&vanished).unwrap();
    assert!(source_files::files(&vanished).is_empty());
}
