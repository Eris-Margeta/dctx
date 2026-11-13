// SPDX-License-Identifier: Apache-2.0
//! Exit-code contract of docs/CLI-EXIT-CODES.md through the compiled CLI: 0 ok, 1 no result, 2 usage, 3 index error.
//! Twelve bad inputs each expect their code, a specific stderr message and a `hint:` line, and never a panic.
//! Every subcommand `--help` page carries an `Example:` line. Home, cache and plugin state are isolated per fixture.

#![forbid(unsafe_code)]

mod common;

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// One isolated place with a Python file and a binary file, plus an empty directory beside it.
struct Fixture {
    /// Owns every directory below; removed on drop.
    root: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().expect("fixture root");
        for folder in [
            "place",
            "empty",
            "home",
            "config",
            "cache",
            "plugins",
            "empty-path",
            "tmp",
        ] {
            fs::create_dir(root.path().join(folder)).expect("isolated directory");
        }
        let place = root.path().join("place");
        fs::write(
            place.join("a.py"),
            "def alpha():\n    return beta()\n\n\ndef beta():\n    return 1\n",
        )
        .expect("source");
        let binary: Vec<u8> = (0..=255u8).cycle().take(1024).collect();
        fs::write(place.join("logo.png"), binary).expect("binary");
        Self { root }
    }

    fn path(&self, folder: &str) -> PathBuf {
        self.root.path().join(folder)
    }

    fn output(&self, args: &[OsString]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_dctx-cli"));
        command
            .current_dir(self.path("place"))
            .env_clear()
            .args(args)
            .env("DCTX_AUTO_CLEAN", "0");
        common::isolated_dirs(&mut command, self.root.path());
        for (variable, folder) in [
            ("DCTX_PLUGIN_ROOT", "plugins"),
            ("DCTX_CACHE_DIR", "cache"),
            ("PATH", "empty-path"),
        ] {
            command.env(variable, self.path(folder));
        }
        command.output().expect("run compiled CLI")
    }

    /// Runs `args`, asserts exit `code`, a stderr containing `message` and a hint, and no panic text anywhere.
    fn refuse(&self, code: i32, message: &str, args: &[OsString]) {
        let output = self.output(args);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(
            output.status.code(),
            Some(code),
            "{args:?}: stdout {stdout}; stderr {stderr}"
        );
        assert!(
            stderr.contains(message),
            "{args:?}: want {message:?} in {stderr}"
        );
        assert!(
            stderr
                .lines()
                .any(|line| line.starts_with("hint: ") && line.len() > "hint: ".len()),
            "{args:?}: no hint in {stderr}"
        );
        for text in ["panicked", "RUST_BACKTRACE"] {
            assert!(
                !stdout.contains(text) && !stderr.contains(text),
                "{args:?}: {text} in {stdout}{stderr}"
            );
        }
    }

    /// Runs `args` and returns stdout after asserting exit 0.
    fn ok(&self, args: &[OsString]) -> String {
        let output = self.output(args);
        assert_eq!(
            output.status.code(),
            Some(0),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).expect("UTF-8 stdout")
    }
}

fn words(args: &[&str]) -> Vec<OsString> {
    args.iter().map(OsString::from).collect()
}

fn with_path(head: &[&str], path: &Path) -> Vec<OsString> {
    let mut args = words(head);
    args.push(path.as_os_str().to_owned());
    args
}

#[test]
fn twelve_bad_inputs_get_their_exit_code_and_a_hint() {
    let fixture = Fixture::new();
    assert!(fixture
        .ok(&words(&["map", "find", "alpha"]))
        .contains("a.py#alpha"));
    fixture.refuse(2, "unknown command `frobnicate`", &words(&["frobnicate"]));
    fixture.refuse(
        2,
        "unknown or inapplicable option --bogus",
        &words(&["map", "find", "alpha", "--bogus"]),
    );
    fixture.refuse(2, "requires a target", &words(&["map", "neighbors"]));
    fixture.refuse(
        1,
        "m:nope.py is not in the index",
        &words(&["map", "read", "m:nope.py"]),
    );
    fixture.refuse(
        2,
        "limit or depth exceeds the verb's bounds: --depth 9 is above neighbors' maximum 4 (allowed range 1..=4)",
        &words(&["map", "neighbors", "s:a.py#alpha", "--depth", "9"]),
    );
    fixture.refuse(2, "requires a target", &words(&["map", "find", ""]));
    let mut missing = words(&["map", "find", "alpha", "--place"]);
    missing.push(fixture.path("missing").into_os_string());
    fixture.refuse(3, "cannot open the place", &missing);
    fixture.refuse(
        2,
        "stale continuation",
        &words(&[
            "map",
            "find",
            "alpha",
            "--cursor",
            "v1|999999|find|s:a.py#alpha",
        ]),
    );
    fixture.refuse(
        2,
        "outside the place",
        &words(&["map", "read", "m:../outside.py"]),
    );
    fixture.refuse(
        1,
        "logo.png is a binary file",
        &words(&["map", "read", "logo.png"]),
    );
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        let mut bad = words(&["map", "read"]);
        bad.push(OsString::from_vec(vec![0xff, 0xfe, b'.', b'p', b'y']));
        fixture.refuse(2, "not valid UTF-8", &bad);
    }
    fixture.refuse(
        1,
        "nothing to pack",
        &with_path(&["pack"], &fixture.path("empty")),
    );
    assert!(fixture
        .ok(&words(&["map", "read", "s:a.py#alpha"]))
        .contains("return beta()"));
}

#[test]
fn grep_limits_are_independent_of_literal_case_and_windows_style_paths() {
    let fixture = Fixture::new();
    fs::write(
        fixture.path("place").join("a.py"),
        "def alpha():\n    return \"ESCALATED TODO HTTP_ERROR\"\n",
    )
    .expect("all-caps literals");
    // Shell quotes are removed before argv reaches the CLI. The literal is one argument
    // here too; path-shaped literals simulate what MSYS argument conversion can produce.
    for literal in [
        "ESCALATED",
        "TODO",
        "HTTP_ERROR",
        "alpha",
        "C:/msys64/ESCALATED",
        r"C:\repo\ESCALATED",
    ] {
        for limit in ["1", "40"] {
            let page = fixture.ok(&words(&["map", "grep", literal, "--limit", limit]));
            assert!(page.contains(" grep\n"), "{page}");
        }
        for limit in 41..=60 {
            fixture.refuse(
                2,
                &format!("limit or depth exceeds the verb's bounds: --limit {limit} is above grep's maximum 40 (allowed range 1..=40)"),
                &words(&["map", "grep", literal, "--limit", &limit.to_string()]),
            );
        }
    }
}

#[test]
fn map_bounds_use_the_displayed_verb_and_preserve_exit_2() {
    let fixture = Fixture::new();
    for (verb, target, option, value, detail) in [
        (
            "grep",
            "ESCALATED",
            "--limit",
            "0",
            "--limit 0 is below grep's minimum 1 (allowed range 1..=40)",
        ),
        (
            "find",
            "alpha",
            "--limit",
            "50",
            "--limit 50 is above find's maximum 40 (allowed range 1..=40)",
        ),
        (
            "map",
            "a.py",
            "--limit",
            "61",
            "--limit 61 is above map's maximum 60 (allowed range 1..=60)",
        ),
        (
            "skeleton",
            "a.py",
            "--limit",
            "61",
            "--limit 61 is above skeleton's maximum 60 (allowed range 1..=60)",
        ),
        (
            "hubs",
            "",
            "--limit",
            "61",
            "--limit 61 is above hubs' maximum 60 (allowed range 1..=60)",
        ),
        (
            "map",
            "a.py",
            "--depth",
            "0",
            "--depth 0 is below map's minimum 1 (allowed range 1..=3)",
        ),
        (
            "map",
            "a.py",
            "--depth",
            "4",
            "--depth 4 is above map's maximum 3 (allowed range 1..=3)",
        ),
        (
            "neighbors",
            "s:a.py#alpha",
            "--depth",
            "5",
            "--depth 5 is above neighbors' maximum 4 (allowed range 1..=4)",
        ),
    ] {
        let mut args = words(&["map", verb]);
        if !target.is_empty() {
            args.push(target.into());
        }
        args.extend(words(&[option, value]));
        fixture.refuse(
            2,
            &format!("limit or depth exceeds the verb's bounds: {detail}"),
            &args,
        );
    }
    for (verb, target, option, maximum) in [
        ("map", "a.py", "--limit", "60"),
        ("neighbors", "s:a.py#alpha", "--limit", "60"),
        ("map", "a.py", "--depth", "3"),
        ("neighbors", "s:a.py#alpha", "--depth", "4"),
    ] {
        fixture.ok(&words(&["map", verb, target, option, maximum]));
    }
}

#[test]
fn malformed_windows_style_argv_gets_its_own_diagnostic() {
    let fixture = Fixture::new();
    for (args, message) in [
        (
            vec!["map", "grep", "ESCALATED\r"],
            "target is too long or contains control characters",
        ),
        (
            vec!["map", "grep", "ESCALATED", "--limit", "50\r"],
            "invalid number: 50\r",
        ),
        (
            vec!["map", "grep", "ESCALATED", "--limit\r", "50"],
            "unknown or inapplicable option --limit\r",
        ),
        (
            vec!["map", "grep", "ESCALATED", "--in", "C:/msys64/src"],
            "path must be relative to the place",
        ),
        (
            vec!["map", "grep", "ESCALATED", "--depth", "9"],
            "unknown or inapplicable option --depth",
        ),
    ] {
        fixture.refuse(2, message, &words(&args));
    }
}

#[test]
fn a_gone_page_still_reaches_stdout_before_exit_1() {
    let fixture = Fixture::new();
    let output = fixture.output(&words(&["map", "read", "m:nope.py"]));
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("gone\tm:nope.py"),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn every_subcommand_help_has_an_example() {
    let fixture = Fixture::new();
    let mut pages = vec![
        vec!["map", "--help"],
        vec!["pack", "--help"],
        vec!["languages", "--help"],
    ];
    for verb in [
        "map",
        "find",
        "neighbors",
        "explain",
        "read",
        "skeleton",
        "grep",
        "hubs",
        "path",
        "export",
    ] {
        pages.push(vec!["map", verb, "--help"]);
    }
    for page in pages {
        let text = fixture.ok(&words(&page));
        assert!(
            text.lines().any(|line| line.starts_with("Example: dctx ")),
            "{page:?}: {text}"
        );
    }
}

#[test]
fn a_bare_dctx_without_a_terminal_prints_usage_and_exits_2() {
    let fixture = Fixture::new();
    // `Command::output` gives the child a null stdin and piped stdout: neither is a terminal.
    let output = fixture.output(&[]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty(), "{:?}", output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("needs a terminal"), "{stderr}");
    assert!(stderr.contains("hint: usage: dctx <COMMAND>"), "{stderr}");
    assert!(!stderr.contains('\x1b'), "{stderr}");
}

#[test]
fn cache_and_template_failures_follow_the_contract() {
    let fixture = Fixture::new();
    fixture.refuse(
        2,
        "cache: purge requires --root PATH or --all",
        &words(&["cache", "purge"]),
    );
    fixture.refuse(
        2,
        "cache: purge requires --yes",
        &words(&["cache", "purge", "--all"]),
    );
    fixture.refuse(
        2,
        "cache: unknown cache command",
        &words(&["cache", "frobnicate"]),
    );
    fixture.refuse(
        2,
        "template: unknown template action",
        &words(&["template", "x"]),
    );
    let missing = fixture.path("missing.dctx_template");
    fixture.refuse(
        3,
        &format!("template: failed to read template {}", missing.display()),
        &with_path(&["template", "build"], &missing),
    );
}

#[test]
fn pack_help_is_the_pack_page_only() {
    let fixture = Fixture::new();
    let page = fixture.ok(&words(&["pack", "--help"]));
    assert!(page.starts_with("Usage: dctx pack"), "{page}");
    assert!(page.contains("Pack filters"));
    for global in ["precision:", "dctx cache", "Template Commands"] {
        assert!(!page.contains(global), "{global}: {page}");
    }
    let global = fixture.ok(&words(&["--help"]));
    assert!(global.contains("hubs|path|export>"), "{global}");
}
