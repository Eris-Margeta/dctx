// SPDX-License-Identifier: Apache-2.0
//! Golden named context and session references on all executable transports.
#![forbid(unsafe_code)]
mod common;

use serde_json::{json, Value};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

const GOLDEN: &str = include_str!("../../dctx/tests/fixtures/context/ping.json");
const TEXT: &str = include_str!("../../dctx/tests/fixtures/context/ping.txt");

fn command(root: &Path) -> Command {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../dctx/tests/fixtures/repository");
    let mut command = Command::new(env!("CARGO_BIN_EXE_dctx-cli"));
    command
        .current_dir(repo)
        .env_clear()
        .env("DCTX_AUTO_CLEAN", "0")
        .env("NO_COLOR", "1");
    common::isolated_dirs(&mut command, root);
    command.env("DCTX_CACHE_DIR", root.join("cache"));
    command
}

fn server(root: &Path, verb: &str, requests: &[Value]) -> Vec<Value> {
    let mut child = command(root)
        .arg(verb)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    {
        let mut input = child.stdin.take().unwrap();
        for request in requests {
            writeln!(input, "{request}").unwrap();
        }
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn cli_context_is_golden_and_batches_share_opt_in_delivery_history() {
    let temp = tempfile::tempdir().unwrap();
    for (flags, golden) in [(vec![], TEXT), (vec!["--json"], GOLDEN)] {
        let output = command(temp.path())
            .args(["context", "ping"])
            .args(flags)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout, golden.as_bytes());
    }
    let output = command(temp.path())
        .args([
            "context",
            "ping",
            "ping",
            "--json",
            "--session",
            "a",
            "--dedupe",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let pages: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(pages[0], serde_json::from_str::<Value>(GOLDEN).unwrap());
    assert_reference(&pages[0], &pages[1]);
    let output = command(temp.path())
        .args([
            "context",
            "ping",
            "--session",
            "--help",
            "--dedupe",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, GOLDEN.as_bytes());
}

fn assert_reference(full: &Value, reference: &Value) {
    assert_eq!(reference["chunks"][0]["id"], full["chunks"][0]["id"]);
    assert_eq!(reference["chunks"][0]["ref"], true);
    assert!(reference["chunks"][0].get("content").is_none());
}

#[test]
fn rpc_context_is_golden_references_repeat_chunks_and_isolates_sessions() {
    let temp = tempfile::tempdir().unwrap();
    let replies = server(
        temp.path(),
        "rpc",
        &[
            json!({"id":0,"verb":"hello"}),
            json!({"id":1,"verb":"context","args":["ping","--budget","3000","--session","a","--dedupe","--json"]}),
            json!({"id":2,"verb":"context","args":["ping","--session","a","--dedupe"]}),
            json!({"id":3,"verb":"context","args":["ping","--session","b","--dedupe"]}),
            json!({"id":4,"verb":"context","args":["ping","--session","--for","--dedupe"]}),
            json!({"id":5,"verb":"context","args":["ping","--session","--help","--dedupe"]}),
            json!({"id":6,"verb":"context","args":["ping","--session","--json","--dedupe"]}),
        ],
    );
    assert_eq!(replies[1]["result"]["output"], GOLDEN);
    assert_eq!(replies[3]["result"]["output"], GOLDEN);
    assert_eq!(replies[4]["result"]["output"], GOLDEN);
    assert_eq!(replies[5]["result"]["output"], GOLDEN);
    assert_eq!(replies[6]["result"]["output"], GOLDEN);
    let full: Value =
        serde_json::from_str(replies[1]["result"]["output"].as_str().unwrap()).unwrap();
    let repeated: Value =
        serde_json::from_str(replies[2]["result"]["output"].as_str().unwrap()).unwrap();
    assert_reference(&full, &repeated);
}

#[test]
fn invalid_context_options_are_usage_errors_and_missing_targets_emit_empty_pages() {
    let temp = tempfile::tempdir().unwrap();
    for args in [
        vec!["ping", "--dedupe"],
        vec!["ping", "--budget", "99"],
        vec!["ping", "--session", ""],
        vec![],
    ] {
        let missing = args.is_empty();
        let output = command(temp.path())
            .arg("context")
            .args(&args)
            .output()
            .unwrap();
        // A missing TARGET and every malformed option are exit 2 with a hint and no stdout.
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(output.stdout.is_empty());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.starts_with("❌ Error: "), "{stderr}");
        assert!(stderr.contains("\nhint: "), "{stderr}");
        assert!(
            !missing || stderr.contains("context requires TARGET"),
            "{stderr}"
        );
    }
    let output = command(temp.path())
        .args(["context", "--help"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.starts_with(b"Usage:"));
    let output = command(temp.path())
        .args(["context", "missing_target", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["chunks"],
        json!([])
    );
}
