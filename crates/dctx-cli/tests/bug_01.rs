// SPDX-License-Identifier: Apache-2.0
//! DCX-BUG-01 regressions through the compiled CLI, with isolated cache state.
#![forbid(unsafe_code)]

mod common;

use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

fn command(root: &Path, place: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_dctx-cli"));
    command
        .current_dir(place)
        .env_clear()
        .env("DCTX_AUTO_CLEAN", "0")
        .env("DCTX_CACHE_DIR", root.join("cache"));
    common::isolated_dirs(&mut command, root);
    command
}

fn ok(output: Output) -> String {
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("UTF-8 page")
}

#[test]
fn pack_small_fixture_keeps_source_order_when_budget_cuts_a_section() {
    let state = tempfile::tempdir().expect("state");
    let place = Path::new(env!("CARGO_MANIFEST_DIR")).join("../dctx-map-parse/tests/fixtures/rust");
    let page = ok(command(state.path(), &place)
        .args(["pack", "--for", "helper", "--budget", "103"])
        .output()
        .expect("pack"));
    assert!(page.contains("## find helper"), "{page}");
    assert!(
        !page.contains("## read") || page.contains("s:src/support.rs#helper\tfn\tsrc/support.rs"),
        "{page}"
    );
    assert!(!page.lines().any(|line| line == "}"), "{page}");
    assert!((page.len() * 2).div_ceil(7) <= 103, "{page}");
}

#[test]
fn pack_accepts_a_file_target_in_the_smallest_fixture_crate() {
    let state = tempfile::tempdir().expect("state");
    let place = Path::new(env!("CARGO_MANIFEST_DIR")).join("../dctx-map-parse/tests/fixtures/rust");
    let page = ok(command(state.path(), &place)
        .args(["pack", "--for", "src/lib.rs"])
        .output()
        .expect("pack"));
    assert!(page.contains("## find src/lib.rs\n"), "{page}");
    assert!(page.contains("s:src/lib.rs#"), "{page}");
    assert!(page.contains("## read m:src/lib.rs\n"), "{page}");
    assert!(page.contains("#![forbid(unsafe_code)]"), "{page}");
}

#[test]
fn read_budget_cut_has_no_dangling_json_separator_before_the_marker() {
    let state = tempfile::tempdir().expect("state");
    let place = state.path().join("fixtures");
    fs::create_dir(&place).expect("fixtures");
    let text = format!(
        "{{\n  \"first\": {{\n    \"name\": \"short\"\n  }},\n  \"second\": \"{}\"\n}}\n",
        "x".repeat(1000)
    );
    fs::write(place.join("sample.json"), text).expect("source");
    for args in [
        vec!["map", "read", "sample.json", "--budget", "70"],
        vec!["read", "sample.json", "--budget", "70"],
    ] {
        let page = ok(command(state.path(), &place)
            .args(&args)
            .output()
            .expect("read"));
        let (prefix, _) = page.split_once("[DCTX_TRUNCATED]").expect("budget cut");
        let last = prefix.lines().last().expect("source prefix").trim();
        assert!(
            !last.chars().all(|ch| "}],; )".contains(ch)),
            "{args:?}: {page}"
        );
        assert!(!page.contains('\u{fffd}'), "{page}");
    }
}

#[test]
fn release_recipe_checks_the_current_root_changelog() {
    let justfile = include_str!("../../../justfile");
    let recipe = justfile
        .split_once("\nrelease:\n")
        .expect("release recipe")
        .1;
    assert!(!recipe.contains("docs"));
    assert!(recipe.contains("\"^## \\[$VERSION\\]\" CHANGELOG.md"));
}
