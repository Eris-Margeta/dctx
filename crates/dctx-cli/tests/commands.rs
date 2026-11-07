// SPDX-License-Identifier: Apache-2.0
//! Removed implicit commands refuse before creating state; explicit pack remains usable.

use std::{fs, process::Command};

#[test]
fn implicit_paths_and_snapshot_flags_are_usage_errors_without_writes() {
    let case = tempfile::tempdir().unwrap();
    fs::write(case.path().join("keep.rs"), "fn keep() {}\n").unwrap();
    for args in [
        vec!["."],
        vec![case.path().to_str().unwrap()],
        vec!["-d"],
        vec!["--diff"],
        vec!["-t"],
        vec!["--tree"],
        vec!["-r"],
        vec!["--reset"],
        vec!["-c"],
        vec!["--clipboard"],
        vec!["--ext", "rs"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_dctx-cli"))
            .current_dir(case.path())
            .env("DCTX_AUTO_CLEAN", "0")
            .args(&args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{args:?}: {output:?}");
        assert!(output.stdout.is_empty(), "{args:?}: {output:?}");
        assert!(!output.stderr.is_empty());
        assert_eq!(
            fs::read_dir(case.path()).unwrap().count(),
            1,
            "{args:?} wrote project files"
        );
    }
    let output = Command::new(env!("CARGO_BIN_EXE_dctx-cli"))
        .current_dir(case.path())
        .env("DCTX_AUTO_CLEAN", "0")
        .args(["pack", "."])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(String::from_utf8(output.stdout)
        .unwrap()
        .contains("fn keep() {}"));
    assert_eq!(fs::read_dir(case.path()).unwrap().count(), 1);
}

#[test]
fn help_has_only_current_command_forms() {
    let output = Command::new(env!("CARGO_BIN_EXE_dctx-cli"))
        .args(["--help"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).unwrap();
    assert!(help.contains("dctx pack"));
    for removed in [
        "dctx [PATH]",
        "dctx .",
        "--diff",
        "--tree ",
        "--reset",
        "legacy",
    ] {
        assert!(!help.contains(removed), "{removed}: {help}");
    }
}
