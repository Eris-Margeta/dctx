// SPDX-License-Identifier: Apache-2.0
//! Agent setup bytes and usage through the compiled binary, without project state.

mod common;

use std::process::{Command, Output};

fn output(args: &[&str]) -> Output {
    let root = tempfile::tempdir().expect("isolated root");
    let mut command = Command::new(env!("CARGO_BIN_EXE_dctx-cli"));
    command.env_clear().current_dir(root.path()).args(args);
    common::isolated_dirs(&mut command, root.path());
    let cache = root.path().join("unused-cache");
    command.env("DCTX_CACHE_DIR", &cache);
    let output = command.output().expect("run CLI");
    assert!(!cache.exists(), "agent-setup must not initialize a cache");
    assert!(!root.path().join(".dctx").exists());
    output
}

#[test]
fn formats_match_golden_bytes_on_repeated_runs() {
    for (format, golden) in [
        (
            "agents",
            include_bytes!("golden/agent_setup_agents.txt").as_slice(),
        ),
        (
            "claude",
            include_bytes!("golden/agent_setup_claude.txt").as_slice(),
        ),
        (
            "plain",
            include_bytes!("golden/agent_setup_plain.txt").as_slice(),
        ),
    ] {
        for _ in 0..2 {
            let result = output(&["agent-setup", "--format", format]);
            assert!(result.status.success(), "{result:?}");
            assert!(result.stderr.is_empty(), "{result:?}");
            assert_eq!(result.stdout, golden, "{format}");
            assert!(result.stdout.split(|byte| *byte == b'\n').count() <= 26);
        }
    }
    let default = output(&["agent-setup"]);
    assert!(default.status.success());
    assert!(default.stderr.is_empty());
    assert_eq!(
        default.stdout,
        include_bytes!("golden/agent_setup_agents.txt")
    );
}

#[test]
fn invalid_arguments_are_usage_errors() {
    for args in [
        vec!["--format"],
        vec!["--format", "unknown"],
        vec!["--unknown"],
        vec!["agents"],
        vec!["--format", "agents", "extra"],
        vec!["--format", "agents", "--format", "plain"],
    ] {
        let mut words = vec!["agent-setup"];
        words.extend(args);
        let result = output(&words);
        assert_eq!(result.status.code(), Some(2), "{words:?}: {result:?}");
        assert!(result.stdout.is_empty());
        let error = String::from_utf8(result.stderr).expect("UTF-8 error");
        assert!(error.contains("agent-setup:"), "{error}");
        assert!(error.contains("hint:"), "{error}");
    }
}

#[test]
fn help_explains_the_default_and_has_an_example() {
    for flag in ["--help", "-h"] {
        let result = output(&["agent-setup", flag]);
        assert!(result.status.success());
        assert!(result.stderr.is_empty());
        let help = String::from_utf8(result.stdout).expect("UTF-8 help");
        assert!(help.contains("Usage: dctx agent-setup [--format claude|agents|plain]"));
        assert!(help.contains("default: agents"));
        assert!(help.contains("Example:"));
    }
}
