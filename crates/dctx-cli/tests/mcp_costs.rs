// SPDX-License-Identifier: Apache-2.0
//! Wire-byte budgets, text defaults and one-call context on a deterministic corpus.
mod common;

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

struct Fixture {
    root: tempfile::TempDir,
    child: Child,
    input: Option<ChildStdin>,
    output: BufReader<ChildStdout>,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let place = root.path().join("place");
        std::fs::create_dir(&place).unwrap();
        let mut source = String::from("pub fn beacon() -> usize { 7 }\n");
        for n in 0..80 {
            source.push_str(&format!("pub fn entry_{n:03}() -> usize {{ beacon() }}\n"));
        }
        std::fs::write(place.join("sample.rs"), source).unwrap();
        let mut command = Self::command(&root);
        let mut child = command
            .arg("mcp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        Self {
            input: child.stdin.take(),
            output: BufReader::new(child.stdout.take().unwrap()),
            child,
            root,
        }
    }

    fn command(root: &tempfile::TempDir) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_dctx-cli"));
        command
            .current_dir(root.path().join("place"))
            .env_clear()
            .env("DCTX_AUTO_CLEAN", "0")
            .env("DCTX_CACHE_DIR", root.path().join("cache"));
        common::isolated_dirs(&mut command, root.path());
        command
    }

    fn request(&mut self, request: Value) -> (usize, Value) {
        let input = self.input.as_mut().unwrap();
        writeln!(input, "{request}").unwrap();
        input.flush().unwrap();
        let mut wire = Vec::new();
        assert!(self.output.read_until(b'\n', &mut wire).unwrap() > 0);
        (wire.len(), serde_json::from_slice(&wire).unwrap())
    }

    fn call(&mut self, tool: &str, arguments: Value, budget: usize) -> String {
        let (bytes, reply) = self.request(json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
            "params":{"name":tool,"arguments":arguments}}));
        assert!(reply.get("error").is_none(), "{reply}");
        assert_ne!(reply["result"]["isError"], true, "{reply}");
        assert!(bytes <= budget, "{tool}: {bytes} wire bytes > {budget}");
        assert!(reply["result"].get("structuredContent").is_none());
        reply["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    fn cli(&self, args: &[&str]) -> String {
        let out = Self::command(&self.root).args(args).output().unwrap();
        assert!(out.status.success(), "{out:?}");
        String::from_utf8(out.stdout).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        drop(self.input.take());
        assert!(self.child.wait().unwrap().success());
    }
}

#[test]
fn full_tool_catalog_has_a_wire_byte_budget_and_preserves_existing_names() {
    let mut fixture = Fixture::new();
    let (bytes, reply) = fixture.request(json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}));
    assert!(bytes <= 6800, "tools/list: {bytes} wire bytes > 6800");
    let tools = reply["result"]["tools"].as_array().unwrap();
    let names: Vec<_> = tools
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "map",
            "find",
            "neighbors",
            "explain",
            "read",
            "hubs",
            "grep",
            "skeleton",
            "path",
            "trace",
            "impact",
            "export",
            "pack"
        ]
    );
    for tool in tools {
        assert_eq!(tool["inputSchema"]["additionalProperties"], false);
        if tool["name"] != "export" {
            assert_eq!(tool["inputSchema"]["properties"]["json"]["default"], false);
        }
    }
    let pack = tools.iter().find(|tool| tool["name"] == "pack").unwrap();
    assert_eq!(pack["inputSchema"]["required"], json!(["task"]));
    assert!(pack["inputSchema"]["properties"].get("cursor").is_none());
}

#[test]
fn find_defaults_to_cli_text_with_a_bounded_page_and_resumable_cursor() {
    let mut fixture = Fixture::new();
    let text = fixture.call("find", json!({"query":"entry_"}), 1900);
    assert_eq!(text, fixture.cli(&["map", "find", "entry_"]));
    assert!(text.contains("entry_000"), "{text}");
    let first = fixture.call(
        "find",
        json!({"query":"entry_", "limit":1, "json":true}),
        2100,
    );
    let first: Value = serde_json::from_str(&first).unwrap();
    let next = first["next"].as_str().expect("next page");
    let second = fixture.call(
        "find",
        json!({"query":"entry_", "limit":1, "cursor":next, "json":true}),
        2100,
    );
    let second: Value = serde_json::from_str(&second).unwrap();
    assert_ne!(first["records"][0]["id"], second["records"][0]["id"]);
}

#[test]
fn read_defaults_to_cli_source_text_and_json_is_opt_in() {
    let mut fixture = Fixture::new();
    let text = fixture.call("read", json!({"id":"s:sample.rs#beacon"}), 600);
    assert_eq!(text, fixture.cli(&["map", "read", "s:sample.rs#beacon"]));
    assert!(text.contains("pub fn beacon()"), "{text}");
    let structured = fixture.call(
        "read",
        json!({"id":"s:sample.rs#beacon", "json":true}),
        1000,
    );
    let structured: Value = serde_json::from_str(&structured).unwrap();
    assert_eq!(structured["v"], 2);
    assert_eq!(structured["records"][0]["id"], "s:sample.rs#beacon");
}

#[test]
fn neighbors_defaults_to_cli_text_and_preserves_caller_locations() {
    let mut fixture = Fixture::new();
    let text = fixture.call(
        "neighbors",
        json!({"id":"s:sample.rs#beacon", "dir":"in", "edge":["calls"]}),
        2000,
    );
    assert_eq!(
        text,
        fixture.cli(&[
            "map",
            "neighbors",
            "s:sample.rs#beacon",
            "--dir",
            "in",
            "--edge",
            "calls"
        ])
    );
    assert!(text.contains("entry_000"), "{text}");
    assert!(text.contains("sample.rs"), "{text}");
}

#[test]
fn pack_answers_definition_callers_and_source_in_one_bounded_call() {
    let mut fixture = Fixture::new();
    let text = fixture.call("pack", json!({"task":"`beacon`"}), 7400);
    assert_eq!(
        text,
        fixture.cli(&["pack", "--for", "`beacon`", "--budget", "2000"])
    );
    for expected in [
        "## find beacon",
        "s:sample.rs#beacon",
        "## neighbors",
        "entry_000",
        "## read",
        "pub fn beacon()",
    ] {
        assert!(text.contains(expected), "missing {expected}: {text}");
    }
    let structured = fixture.call(
        "pack",
        json!({"task":"`beacon`", "budget":1000, "json":true}),
        4100,
    );
    assert_eq!(
        structured,
        fixture.cli(&["pack", "--for", "`beacon`", "--budget", "1000", "--json"])
    );
    let structured: Value = serde_json::from_str(&structured).unwrap();
    assert_eq!(structured["verb"], "context");
    // A flag-shaped task stays a value and cannot select another root or output path.
    fixture.call("pack", json!({"task":"--place"}), 7400);
}

#[test]
fn graph_answer_defaults_are_bounded_and_explicit_limits_still_win() {
    let mut fixture = Fixture::new();
    let text = fixture.call("impact", json!({"symbol":"beacon"}), 8700);
    assert!(text.contains("truncated"), "{text}");
    let structured = fixture.call("impact", json!({"symbol":"beacon", "json":true}), 10000);
    let structured: Value = serde_json::from_str(&structured).unwrap();
    assert_eq!(structured["bounds"]["max_bytes"], 8192);
    assert_eq!(structured["bounds"]["max_sites"], 50);
    let explicit = fixture.call(
        "impact",
        json!({"symbol":"beacon", "json":true, "max_bytes":65536, "max_sites":1000}),
        65536,
    );
    assert_eq!(explicit, fixture.cli(&["impact", "beacon", "--json"]));
    let trace = fixture.call(
        "trace",
        json!({"from":"entry_000", "to":"beacon", "json":true}),
        1200,
    );
    let trace: Value = serde_json::from_str(&trace).unwrap();
    assert_eq!(trace["bounds"]["max_paths"], 5);
    assert_eq!(trace["bounds"]["max_bytes"], 8192);
}
