// SPDX-License-Identifier: Apache-2.0
//! Subprocess coverage of the opt-in stderr phase report on cold and warm finds.
#![forbid(unsafe_code)]

mod common;

use std::process::{Command, Output};

struct Fixture(tempfile::TempDir);

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().expect("fixture");
        std::fs::create_dir(root.path().join("repo")).expect("repo");
        std::fs::write(
            root.path().join("repo/lib.rs"),
            "pub fn timing_probe() {}\n",
        )
        .expect("source");
        Self(root)
    }

    fn run(&self, timing: Option<&str>, arguments: &[&str]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_dctx-cli"));
        command
            .env_clear()
            .current_dir(self.0.path().join("repo"))
            .env("DCTX_AUTO_CLEAN", "0")
            .env("DCTX_CACHE_DIR", self.0.path().join("cache"))
            .args(arguments);
        common::isolated_dirs(&mut command, self.0.path());
        if let Some(value) = timing {
            command.env("DCTX_TIMINGS", value);
        }
        command.output().expect("CLI output")
    }
}

fn report(output: &Output) -> serde_json::Value {
    let stderr = std::str::from_utf8(&output.stderr).expect("UTF-8 stderr");
    let lines: Vec<_> = stderr
        .lines()
        .filter(|line| line.contains("dctx_timings"))
        .collect();
    assert_eq!(lines.len(), 1, "{stderr}");
    let value: serde_json::Value = serde_json::from_str(lines[0]).expect("JSON line");
    assert_eq!(value.as_object().expect("object").len(), 1);
    let phases = value["dctx_timings"].as_object().expect("phases");
    for name in [
        "config_and_root",
        "generation_open",
        "decode",
        "query",
        "render",
        "write",
    ] {
        assert!(
            phases.get(name).is_some_and(|value| value.is_u64()),
            "{value}"
        );
    }
    value
}

#[test]
fn cold_and_warm_find_emit_one_json_line_only_when_enabled() {
    let fixture = Fixture::new();
    let args = ["map", "find", "timing_probe", "--json", "--place", "."];
    let cold = fixture.run(Some("1"), &args);
    assert!(cold.status.success(), "{cold:?}");
    report(&cold);
    let warm = fixture.run(Some("1"), &args);
    assert!(warm.status.success(), "{warm:?}");
    report(&warm);
    let expected: serde_json::Value = serde_json::from_slice(&warm.stdout).expect("stdout JSON");
    for setting in [None, Some("0"), Some("")] {
        let plain = fixture.run(setting, &args);
        assert!(plain.status.success(), "{plain:?}");
        assert!(plain.stderr.is_empty(), "{plain:?}");
        let actual: serde_json::Value = serde_json::from_slice(&plain.stdout).expect("stdout JSON");
        assert_eq!(actual, expected);
    }
}

#[test]
fn failure_exit_still_emits_one_json_line() {
    let fixture = Fixture::new();
    let output = fixture.run(Some("1"), &["map", "find"]);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    report(&output);
    assert!(String::from_utf8_lossy(&output.stderr).contains("hint:"));
    let plain = fixture.run(None, &["map", "find"]);
    assert_eq!(plain.status.code(), Some(2));
    assert!(!String::from_utf8_lossy(&plain.stderr).contains("dctx_timings"));
}
