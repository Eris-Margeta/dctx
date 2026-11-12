// SPDX-License-Identifier: Apache-2.0
//! One-call answers use the same engine bytes through CLI, MCP and engine RPC.
mod common;

use serde_json::{json, Value};
use std::io::Write;
use std::process::{Command, Output, Stdio};

struct Fixture {
    root: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().expect("fixture");
        std::fs::create_dir(root.path().join("repo")).unwrap();
        std::fs::write(root.path().join("repo/chain.py"), "def start():\n    return middle()\n\ndef middle():\n    return branch()\n\ndef branch():\n    return leaf()\n\ndef leaf():\n    return 1\n\ndef other():\n    leaf()\n    leaf()\n    leaf()\n    return leaf()\n\ndef lone():\n    return 0\n").unwrap();
        Self { root }
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_dctx-cli"));
        cmd.current_dir(self.root.path().join("repo"))
            .env_clear()
            .args(args)
            .env("DCTX_AUTO_CLEAN", "0")
            .env("DCTX_MAP_CACHE", self.root.path().join("map-cache"))
            .env("DCTX_CACHE_DIR", self.root.path().join("cache"));
        common::isolated_dirs(&mut cmd, self.root.path());
        cmd
    }

    fn output(&self, args: &[&str]) -> Output {
        self.command(args).output().expect("CLI")
    }

    fn run(&self, args: &[&str]) -> String {
        let out = self.output(args);
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            out.stderr.is_empty(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    }

    fn json(&self, args: &[&str]) -> Value {
        serde_json::from_str(&self.run(args)).unwrap()
    }

    fn transport(&self, args: &[&str], requests: &[Value]) -> Vec<Value> {
        let mut child = self
            .command(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut input = child.stdin.take().unwrap();
        for request in requests {
            writeln!(input, "{request}").unwrap();
        }
        drop(input);
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }
}

#[test]
fn exact_three_hop_trace_is_directed_and_reuses_the_index() {
    let repo = Fixture::new();
    let expected =
        "chain.py:1 start -> chain.py:4 middle -> chain.py:7 branch -> chain.py:10 leaf\n";
    assert_eq!(repo.run(&["trace", "start", "leaf"]), expected);
    assert_eq!(
        repo.run(&["map", "trace", "s:chain.py#start", "s:chain.py#leaf"]),
        expected
    );
    assert_eq!(
        repo.run(&["trace", "leaf", "start"]),
        "no path within bounds\n"
    );
    assert_eq!(
        repo.run(&["trace", "start", "lone"]),
        "no path within bounds\n"
    );
}

#[test]
fn shortest_paths_have_stable_order_and_path_and_depth_bounds() {
    let repo = Fixture::new();
    std::fs::write(repo.root.path().join("repo/diamond.py"), "def origin():\n    left()\n    right()\n\ndef left():\n    sink()\n\ndef right():\n    sink()\n\ndef sink():\n    return 0\n").unwrap();
    let expected = "diamond.py:1 origin -> diamond.py:5 left -> diamond.py:11 sink\ndiamond.py:1 origin -> diamond.py:8 right -> diamond.py:11 sink\n";
    assert_eq!(repo.run(&["trace", "origin", "sink"]), expected);
    assert_eq!(
        repo.run(&["trace", "origin", "sink", "--max-paths", "1"]),
        format!(
            "{}truncated: depth, paths or bytes bound reached\n",
            expected.lines().next().unwrap().to_string() + "\n"
        )
    );
    assert_eq!(
        repo.run(&["trace", "start", "leaf", "--max-depth", "2"]),
        "no path within bounds\ntruncated: depth, paths or bytes bound reached\n"
    );
    assert_eq!(
        repo.run(&["trace", "start", "leaf", "--max-depth", "3"]),
        "chain.py:1 start -> chain.py:4 middle -> chain.py:7 branch -> chain.py:10 leaf\n"
    );
}

#[test]
fn fan_in_impact_lists_every_site_then_transitive_callers_with_counts() {
    let repo = Fixture::new();
    assert_eq!(repo.run(&["impact", "leaf"]), "impact: 5 direct sites, 2 transitive callers, 1 files\nchain.py (5 direct sites, 2 transitive callers)\n  chain.py:8 branch [direct]\n  chain.py:14 other [direct]\n  chain.py:15 other [direct]\n  chain.py:16 other [direct]\n  chain.py:17 other [direct]\n  chain.py:1 start [caller depth 3]\n  chain.py:4 middle [caller depth 2]\n");
    let bounded = repo.json(&["impact", "leaf", "--max-depth", "1", "--json"]);
    assert_eq!(bounded["counts"]["direct_sites"], 5);
    assert_eq!(bounded["counts"]["transitive_callers"], 0);
    assert_eq!(bounded["truncated"], true);
    let bounded = repo.json(&["impact", "leaf", "--max-sites", "3", "--json"]);
    assert_eq!(bounded["counts"]["direct_sites"], 3);
    assert_eq!(bounded["truncated"], true);
}

#[test]
fn impact_finds_noncall_references_and_masks_comments_strings_and_declarations() {
    let repo = Fixture::new();
    std::fs::write(repo.root.path().join("repo/refs.py"), "def value():\n    return 1\n\ndef assign():\n    fn = value\n    # value\n    text = 'value'\n    return fn\n").unwrap();
    let page = repo.json(&["impact", "value", "--json"]);
    assert_eq!(page["counts"]["direct_sites"], 1, "{page}");
    assert_eq!(page["files"][0]["direct"][0]["line"], 5);
    assert_eq!(page["files"][0]["direct"][0]["symbol"], "assign");
}

#[test]
fn json_has_locations_counts_bounds_and_valid_byte_truncation() {
    let repo = Fixture::new();
    let trace = repo.json(&["trace", "start", "leaf", "--json"]);
    assert_eq!(trace["v"], 2);
    assert_eq!(trace["verb"], "trace");
    assert_eq!(trace["counts"]["paths"], 1);
    assert_eq!(trace["paths"][0].as_array().unwrap().len(), 4);
    assert_eq!(
        trace["paths"][0][0],
        json!({"id":"s:chain.py#start","file":"chain.py","line":1,"symbol":"start","depth":0})
    );
    assert_eq!(trace["truncated"], false);
    let small = repo.run(&["trace", "start", "leaf", "--max-bytes", "256", "--json"]);
    assert!(small.len() <= 256);
    let small: Value = serde_json::from_str(&small).unwrap();
    assert_eq!(small["truncated"], true);
    assert!(small["paths"].as_array().unwrap().is_empty());
    let small = repo.run(&["impact", "leaf", "--max-bytes", "512", "--json"]);
    assert!(small.len() <= 512);
    let small: Value = serde_json::from_str(&small).unwrap();
    assert_eq!(small["verb"], "impact");
    assert_eq!(small["truncated"], true);
    assert_eq!(small["bounds"]["max_bytes"], 512);
}

#[test]
fn bounds_errors_name_option_value_range_and_verb_before_opening_place() {
    let repo = Fixture::new();
    for (verb, ends, option, value, range) in [
        (
            "trace",
            vec!["start", "leaf"],
            "--max-depth",
            "17",
            "1..=16",
        ),
        (
            "trace",
            vec!["start", "leaf"],
            "--max-paths",
            "0",
            "1..=100",
        ),
        ("impact", vec!["leaf"], "--max-sites", "10001", "1..=10000"),
        (
            "impact",
            vec!["leaf"],
            "--max-bytes",
            "bad",
            "256..=1048576",
        ),
    ] {
        let mut args = vec![verb];
        args.extend(ends);
        args.extend([option, value, "--place", "missing"]);
        let out = repo.output(&args);
        assert_eq!(out.status.code(), Some(2));
        let error = String::from_utf8(out.stderr).unwrap();
        for expected in [option, value, range, verb] {
            assert!(error.contains(expected), "{error}");
        }
        assert!(out.stdout.is_empty());
    }
}

#[test]
fn missing_symbols_and_denied_files_never_leak_locations() {
    let repo = Fixture::new();
    std::fs::write(repo.root.path().join("repo/.dctxignore"), "denied.py\n").unwrap();
    std::fs::write(
        repo.root.path().join("repo/denied.py"),
        "def hidden():\n    leaf()\n",
    )
    .unwrap();
    assert_eq!(
        repo.run(&["trace", "hidden", "leaf"]),
        "no path within bounds\n"
    );
    assert!(!repo.run(&["impact", "leaf"]).contains("denied.py"));
    let page = repo.json(&["impact", "unknown", "--json"]);
    assert_eq!(page["counts"]["direct_sites"], 0);
    assert_eq!(page["truncated"], false);
}

#[test]
fn place_also_and_outside_notices_use_the_shared_resolution_path() {
    let repo = Fixture::new();
    std::fs::create_dir(repo.root.path().join("shared")).unwrap();
    std::fs::write(
        repo.root.path().join("shared/external.py"),
        "def external():\n    return 1\n",
    )
    .unwrap();
    std::fs::write(
        repo.root.path().join("repo/caller.py"),
        "from shared.external import external\n\ndef caller():\n    return external()\n",
    )
    .unwrap();
    let outside = repo.run(&["trace", "caller", "external"]);
    assert!(outside.contains("no path within bounds"), "{outside}");
    assert!(
        outside.contains("defined outside index: ../shared/external.py"),
        "{outside}"
    );
    assert_eq!(repo.run(&["trace", "caller", "external"]), outside);
    let stable = repo.json(&[
        "trace",
        "s:caller.py#caller",
        "s:external.py#external",
        "--json",
    ]);
    assert_eq!(
        stable["defined_outside_index"][0]["path"],
        "../shared/external.py"
    );
    let outside = repo.json(&["impact", "external", "--json"]);
    assert_eq!(outside["counts"]["direct_sites"], 2);
    assert_eq!(
        outside["defined_outside_index"][0]["path"],
        "../shared/external.py"
    );
    let joined = repo.run(&["trace", "caller", "external", "--also", "../shared"]);
    assert_eq!(
        joined,
        "repo/caller.py:3 caller -> shared/external.py:1 external\n"
    );
    assert_eq!(
        repo.run(&[
            "trace",
            "caller",
            "external",
            "--also",
            "../shared",
            "--place",
            "."
        ]),
        joined
    );
    let impact = repo.json(&["impact", "external", "--also", "../shared", "--json"]);
    assert!(impact["files"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["file"] == "repo/caller.py"));
    assert!(impact.get("defined_outside_index").is_none());
}

#[test]
fn tools_list_and_calls_and_rpc_match_cli_answers() {
    let repo = Fixture::new();
    let replies = repo.transport(&["mcp"], &[
        json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"trace","arguments":{"from":"start","to":"leaf","json":true}}}),
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"impact","arguments":{"symbol":"leaf","json":true}}}),
    ]);
    let tools = replies[0]["result"]["tools"].as_array().unwrap();
    for name in ["trace", "impact"] {
        let tool = tools.iter().find(|tool| tool["name"] == name).unwrap();
        assert_eq!(
            tool["inputSchema"]["properties"]["max_depth"]["type"],
            "integer"
        );
        assert_eq!(tool["inputSchema"]["additionalProperties"], false);
    }
    assert_eq!(
        replies[1]["result"]["content"][0]["text"],
        repo.run(&[
            "trace",
            "start",
            "leaf",
            "--json",
            "--max-paths",
            "5",
            "--max-bytes",
            "8192"
        ])
    );
    assert_eq!(
        replies[2]["result"]["content"][0]["text"],
        repo.run(&[
            "impact",
            "leaf",
            "--json",
            "--max-sites",
            "50",
            "--max-bytes",
            "8192"
        ])
    );
    let rpc = repo.transport(
        &["rpc"],
        &[
            json!({"id":1,"verb":"hello"}),
            json!({"id":2,"verb":"trace","args":["start","leaf"]}),
            json!({"id":3,"verb":"impact","args":["leaf"]}),
        ],
    );
    assert!(rpc[0]["result"]["verbs"]
        .as_array()
        .unwrap()
        .contains(&json!("trace")));
    assert_eq!(
        rpc[1]["result"]["output"],
        repo.run(&["trace", "start", "leaf", "--json"])
    );
    assert_eq!(
        rpc[2]["result"]["output"],
        repo.run(&["impact", "leaf", "--json"])
    );
}

#[test]
fn cycles_aliases_and_multiple_files_keep_unique_callers_and_ordered_groups() {
    let repo = Fixture::new();
    std::fs::write(
        repo.root.path().join("repo/a.py"),
        "def cycle():\n    return cycle()\n",
    )
    .unwrap();
    std::fs::write(repo.root.path().join("repo/b.py"), "from a import cycle as renamed\n\ndef invoke():\n    renamed()\n\ndef upper():\n    invoke()\n").unwrap();
    let page = repo.json(&["impact", "cycle", "--json"]);
    assert_eq!(
        page["counts"],
        json!({"direct_sites": 3, "transitive_callers": 1, "files": 2}),
        "{page}"
    );
    assert_eq!(page["files"][0]["file"], "a.py");
    assert_eq!(page["files"][0]["direct"][0]["line"], 2);
    assert_eq!(page["files"][1]["file"], "b.py");
    assert_eq!(page["files"][1]["direct"][1]["line"], 4);
    assert_eq!(page["truncated"], false);
    assert_eq!(repo.run(&["trace", "cycle", "cycle"]), "a.py:1 cycle\n");
    assert_eq!(
        repo.run(&["trace", "invoke", "cycle"]),
        "b.py:3 invoke -> a.py:1 cycle\n"
    );
}

#[test]
fn ambiguous_names_require_ids_and_only_shortest_paths_are_returned() {
    let repo = Fixture::new();
    std::fs::write(
        repo.root.path().join("repo/short.py"),
        "def quick():\n    leaf()\n    start()\n\ndef lone():\n    return 1\n",
    )
    .unwrap();
    assert_eq!(
        repo.run(&["trace", "quick", "leaf"]),
        "short.py:1 quick -> chain.py:10 leaf\n"
    );
    let out = repo.output(&["trace", "lone", "leaf"]);
    let error = String::from_utf8(out.stderr).unwrap();
    assert!(
        error.contains("ambiguous symbol") && error.contains("stable id"),
        "{error}"
    );
    assert!(out.stdout.is_empty());
    assert_eq!(
        repo.run(&["trace", "s:short.py#lone", "leaf"]),
        "no path within bounds\n"
    );
}
