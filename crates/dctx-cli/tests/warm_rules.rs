// SPDX-License-Identifier: Apache-2.0
//! Nested rule discovery stays fresh across invocations sharing a generation.
#![forbid(unsafe_code)]
mod common;

use std::fs;
use std::process::Command;

#[test]
fn adding_editing_and_removing_nested_rules_matches_a_fresh_cache() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    fs::create_dir_all(root.join("sub/deeper")).unwrap();
    fs::write(root.join("sub/a.rs"), "pub fn AlphaBeacon() {}\n").unwrap();
    fs::write(root.join("sub/b.rs"), "pub fn BravoBeacon() {}\n").unwrap();
    fs::write(root.join("sub/deeper/c.rs"), "pub fn ChildBeacon() {}\n").unwrap();
    let query = |cache: &str| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_dctx-cli"));
        command.env_clear().current_dir(&root);
        common::isolated_dirs(&mut command, temp.path());
        let output = command
            .env("DCTX_CACHE_DIR", temp.path().join(cache))
            .env("DCTX_AUTO_CLEAN", "0")
            .args(["map", "find", "Beacon", "--json"])
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        output.stdout
    };
    let check = |expected: &[&str], absent: &[&str], round: usize| {
        let warm = query("shared-cache");
        assert_eq!(warm, query(&format!("fresh-cache-{round}")));
        assert_eq!(warm, query("shared-cache"));
        let text = String::from_utf8(warm).unwrap();
        for name in expected {
            assert!(text.contains(name), "{text}");
        }
        for name in absent {
            assert!(!text.contains(name), "{text}");
        }
    };
    check(&["AlphaBeacon", "BravoBeacon", "ChildBeacon"], &[], 0);
    fs::write(root.join("sub/.gitignore"), "a.rs\n").unwrap();
    check(&["BravoBeacon", "ChildBeacon"], &["AlphaBeacon"], 1);
    // Same-length replacement must change selection, even when source files did not change.
    fs::write(root.join("sub/.gitignore"), "b.rs\n").unwrap();
    check(&["AlphaBeacon", "ChildBeacon"], &["BravoBeacon"], 2);
    fs::write(root.join("sub/deeper/.gitignore"), "*.rs\n").unwrap();
    check(&["AlphaBeacon"], &["BravoBeacon", "ChildBeacon"], 3);
    fs::remove_file(root.join("sub/.gitignore")).unwrap();
    fs::remove_file(root.join("sub/deeper/.gitignore")).unwrap();
    check(&["AlphaBeacon", "BravoBeacon", "ChildBeacon"], &[], 4);
}
