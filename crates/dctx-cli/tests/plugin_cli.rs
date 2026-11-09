// SPDX-License-Identifier: Apache-2.0
//! Plugin command text output through the real binary.
//! Text mode prints plain text: no JSON fragments and no facade counters on stdout.
//! Text refusals go to stderr as `❌ Error:` plus a `hint:` line; `--json` stays on stdout.

#![forbid(unsafe_code)]

mod common;

use std::path::Path;
use std::process::{Command, Output};

fn dctx(root: &Path, args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_dctx-cli"));
    command.env_clear().current_dir(root);
    common::isolated_dirs(&mut command, root);
    command
        .env("DCTX_PLUGIN_ROOT", root.join("plugins"))
        .env("DCTX_CACHE_DIR", root.join("cache"))
        .env("DCTX_AUTO_CLEAN", "0")
        .args(args)
        .output()
        .unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn assert_plain(output: &Output) {
    let stdout = text(&output.stdout);
    for leak in ["facade_git", "spawns=", "git_children", ",\"", "\":"] {
        assert!(!stdout.contains(leak), "{leak} in {stdout}");
    }
}

#[test]
fn text_mode_management_commands_print_plain_text() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let search = dctx(root, &["plugin", "search", "git"]);
    assert!(search.status.success());
    assert_plain(&search);
    assert!(text(&search.stdout).contains("no plugin index is cached"));
    let json = dctx(root, &["plugin", "search", "git", "--json"]);
    assert!(json.status.success());
    assert!(text(&json.stdout).starts_with("{\"ok\":true,\"command\":\"search\""));
    assert!(text(&json.stdout).contains("\"trust\":\"absent\""));
    let mut commands = vec![&["plugin", "list"][..], &["plugin", "info", "dctx/git"]];
    if cfg!(feature = "git-worker") {
        commands.extend([&["plugin", "verify"][..], &["plugin", "update", "--check"]]);
    }
    for args in commands {
        let output = dctx(root, args);
        assert!(
            output.status.success(),
            "{args:?}: {}",
            text(&output.stderr)
        );
        assert_plain(&output);
        assert!(text(&output.stdout).contains("dctx/git"), "{args:?}");
    }
}

#[cfg(feature = "git-worker")]
#[test]
fn git_without_consent_refuses_on_stderr_with_the_approve_hint() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let refused = dctx(root, &["git", "status"]);
    assert_eq!(refused.status.code(), Some(1));
    assert!(refused.stdout.is_empty(), "{}", text(&refused.stdout));
    let stderr = text(&refused.stderr);
    assert!(
        stderr.starts_with("❌ Error: git: dctx/git needs your consent"),
        "{stderr}"
    );
    assert!(stderr.contains("(ConsentRequired)"), "{stderr}");
    assert!(
        stderr.contains("hint:") && stderr.contains("dctx plugin enable dctx/git --approve"),
        "{stderr}"
    );
    let json = dctx(root, &["git", "status", "--json"]);
    assert_eq!(json.status.code(), Some(1));
    assert!(text(&json.stdout).contains("\"error\":\"ConsentRequired\""));
    assert!(json.stderr.is_empty());

    let enabled = dctx(root, &["plugin", "enable", "dctx/git", "--approve"]);
    if cfg!(windows) {
        // The Windows plugin store is read-only: the refusal says so instead of a store failure.
        assert!(stderr.contains("read-only on Windows"), "{stderr}");
        assert_eq!(enabled.status.code(), Some(1));
        assert!(enabled.stdout.is_empty(), "{}", text(&enabled.stdout));
        let refusal = text(&enabled.stderr);
        assert!(
            refusal.starts_with("❌ Error: enable: the plugin store is read-only on Windows"),
            "{refusal}"
        );
        assert!(refusal.contains("(ReadOnlyStore)") && refusal.contains("hint:"));
        let json = dctx(root, &["plugin", "disable", "dctx/git", "--json"]);
        assert!(text(&json.stdout).contains("\"error\":\"ReadOnlyStore\""));
        return;
    }
    assert!(enabled.status.success(), "{}", text(&enabled.stderr));
    assert_eq!(text(&enabled.stdout), "enabled dctx/git\n");
    let admitted = dctx(root, &["git", "status"]);
    assert!(admitted.status.success(), "{}", text(&admitted.stderr));
    assert_plain(&admitted);
    assert!(text(&admitted.stdout).starts_with("dctx/git status: admitted"));
    let json = dctx(root, &["git", "status", "--json"]);
    let body = text(&json.stdout);
    assert_eq!(body.matches("\"facade_spawns\"").count(), 1, "{body}");

    let disabled = dctx(root, &["plugin", "disable", "dctx/git"]);
    assert!(disabled.status.success());
    assert_plain(&disabled);
    let refused = dctx(root, &["git", "protect"]);
    assert_eq!(refused.status.code(), Some(1));
    assert!(refused.stdout.is_empty());
    assert!(text(&refused.stderr).contains("(Disabled)"));
}

#[test]
fn missing_home_refuses_instead_of_a_working_directory_root() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let output = Command::new(env!("CARGO_BIN_EXE_dctx-cli"))
        .env_clear()
        .current_dir(root)
        .env("DCTX_AUTO_CLEAN", "0")
        .env("DCTX_CACHE_DIR", root.join("cache"))
        .args(["plugin", "enable", "dctx/git", "--approve"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1), "{}", text(&output.stdout));
    assert!(text(&output.stderr).contains("no plugin root"));
    assert!(!root.join(".config").exists());
    assert!(!root.join("Library").exists());
    let help = Command::new(env!("CARGO_BIN_EXE_dctx-cli"))
        .env_clear()
        .current_dir(root)
        .env("DCTX_AUTO_CLEAN", "0")
        .args(["plugin", "help"])
        .output()
        .unwrap();
    assert!(help.status.success());
    assert!(text(&help.stdout).contains("Usage:"));
}
