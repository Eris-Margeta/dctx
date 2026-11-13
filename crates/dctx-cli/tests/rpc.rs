// SPDX-License-Identifier: Apache-2.0
//! Byte parity across real CLI and RPC on the checked-in facade fixture.
#![forbid(unsafe_code)]
mod common;

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

struct Server {
    child: Child,
    input: Option<ChildStdin>,
    output: BufReader<ChildStdout>,
}

impl Server {
    fn new(command: &mut Command) -> Self {
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start server");
        let input = child.stdin.take();
        let output = BufReader::new(child.stdout.take().expect("stdout"));
        Self {
            child,
            input,
            output,
        }
    }

    fn call(&mut self, message: Value) -> Value {
        let input = self.input.as_mut().expect("stdin");
        writeln!(input, "{message}").expect("write request");
        input.flush().expect("flush");
        let mut line = String::new();
        assert!(
            self.output.read_line(&mut line).expect("read reply") > 0,
            "server exited"
        );
        serde_json::from_str(&line).expect("response JSON")
    }

    fn finish(mut self) {
        drop(self.input.take());
        let output = self.child.wait_with_output().expect("wait");
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
    }
}

fn command(root: &Path, repo: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_dctx-cli"));
    command
        .current_dir(repo)
        .env_clear()
        .env("DCTX_AUTO_CLEAN", "0")
        .env("NO_COLOR", "1");
    common::isolated_dirs(&mut command, root);
    command.env("DCTX_CACHE_DIR", root.join("cache"));
    command.env("DCTX_MAP_CACHE", root.join("map-cache"));
    command
}

#[test]
fn committed_schema_is_generated_from_the_rust_wire_types() {
    let committed = include_str!("../../../docs/protocol/dctx-rpc-v1.schema.json");
    assert_eq!(committed, dctx_cli::rpc::schema().expect("generated schema"),
        "regenerate with cargo run -p dctx-cli --example rpc_schema > docs/protocol/dctx-rpc-v1.schema.json");
}

#[test]
fn sixteen_operations_are_byte_identical_across_transports() {
    let temp = tempfile::tempdir().expect("temp");
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../dctx/tests/fixtures/repository");
    let mut rpc = Server::new(command(temp.path(), &repo).arg("rpc"));
    let hello = rpc.call(json!({"id":"hello","verb":"hello"}));
    assert_eq!(hello["result"]["protocol_version"], "1.0.0");
    let cases = [
        ("find", vec!["ping"]),
        ("find", vec!["not_present_zz"]),
        ("find", vec!["ping", "--limit", "1"]),
        ("map", vec!["m:src/lib.rs"]),
        ("read", vec!["s:src/lib.rs#ping"]),
        ("read", vec!["m:src/lib.rs", "--lines", "1-2"]),
        ("skeleton", vec!["m:src/lib.rs"]),
        ("explain", vec!["s:src/lib.rs#ping"]),
        ("neighbors", vec!["s:src/lib.rs#ping"]),
        ("grep", vec!["pub"]),
        ("hubs", vec![]),
        ("path", vec!["m:src/lib.rs", "s:src/lib.rs#ping"]),
        ("pack", vec![]),
        ("context", vec!["--for", "`ping`"]),
        ("languages", vec![]),
    ];
    for (index, (verb, args)) in cases.into_iter().enumerate() {
        let cli_args = match verb {
            "pack" | "context" => vec!["pack"],
            "languages" => vec!["languages"],
            _ => vec!["map", verb],
        };
        let cli = command(temp.path(), &repo)
            .args(cli_args)
            .args(&args)
            .arg("--json")
            .output()
            .expect("CLI");
        assert!(
            cli.status.success(),
            "{verb}: {}",
            String::from_utf8_lossy(&cli.stderr)
        );
        let reply = rpc.call(json!({"id":index,"verb":verb,"args":args}));
        assert_eq!(reply["id"], index);
        assert_eq!(reply["engine_version"], env!("CARGO_PKG_VERSION"));
        let rpc_output = reply["result"]["output"]
            .as_str()
            .unwrap_or_else(|| panic!("{verb}: {reply}"));
        assert_eq!(rpc_output.as_bytes(), cli.stdout, "CLI/RPC {verb}");
    }
    // Cache status must observe the same ledger state after all snapshot operations.
    let cli = command(temp.path(), &repo)
        .args(["cache", "status", "--json"])
        .output()
        .expect("cache status");
    assert!(cli.status.success());
    let reply = rpc.call(json!({"id":"cache","verb":"cache_status"}));
    assert_eq!(
        reply["result"]["output"]
            .as_str()
            .expect("cache output")
            .as_bytes(),
        cli.stdout
    );
    rpc.finish();
}

#[test]
fn rpc_errors_keep_cli_codes_and_any_emitted_page() {
    let temp = tempfile::tempdir().expect("temp");
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../dctx/tests/fixtures/repository");
    let mut rpc = Server::new(command(temp.path(), &repo).arg("rpc"));
    rpc.call(json!({"id":0,"verb":"hello"}));
    for (index, args) in [
        vec!["read", "m:absent.rs"],
        vec!["find", ""],
        vec!["find", "ping", "--place", "missing-directory"],
    ]
    .into_iter()
    .enumerate()
    {
        let cli = command(temp.path(), &repo)
            .arg("map")
            .args(&args)
            .arg("--json")
            .output()
            .expect("CLI");
        let reply = rpc.call(json!({"id":index,"verb":args[0],"args":args[1..]}));
        assert_eq!(
            reply["error"]["code"],
            cli.status.code().expect("exit code")
        );
        assert_eq!(
            reply["error"]["output"]
                .as_str()
                .expect("error output")
                .as_bytes(),
            cli.stdout
        );
        assert_eq!(
            format!(
                "❌ Error: {}\nhint: {}\n",
                reply["error"]["message"].as_str().expect("message"),
                reply["error"]["hint"].as_str().expect("hint")
            )
            .as_bytes(),
            cli.stderr
        );
    }
    for (verb, base, args) in [
        ("languages", vec!["languages"], vec!["--bad"]),
        ("cache_status", vec!["cache", "status"], vec!["--bad"]),
        ("pack", vec!["pack"], vec!["--ext", "rs"]),
        ("context", vec!["pack"], vec!["--for", "`absent`"]),
        ("pack", vec!["pack"], vec!["--for", "--json"]),
    ] {
        let cli = command(temp.path(), &repo)
            .args(base)
            .args(&args)
            .arg("--json")
            .output()
            .expect("CLI error");
        let reply = rpc.call(json!({"id":verb,"verb":verb,"args":args}));
        let error = &reply["error"];
        assert_eq!(
            error["code"],
            cli.status.code().expect("status"),
            "{verb}: {reply}"
        );
        assert_eq!(
            error["output"].as_str().expect("stdout").as_bytes(),
            cli.stdout,
            "{verb}"
        );
        let failure = dctx_cli::exit::Failure::new(
            cli.status.code().expect("status"),
            error["message"].as_str().expect("message"),
            error["hint"].as_str().expect("hint"),
        );
        assert_eq!(failure.render().as_bytes(), cli.stderr, "{verb}");
    }
    // Explicit format arguments must work on commands that reject duplicate flags.
    for verb in ["languages", "cache_status"] {
        assert!(rpc
            .call(json!({"id":verb,"verb":verb,"args":["--json"]}))
            .get("result")
            .is_some());
    }
    assert!(rpc
        .call(json!({"id":"still-alive","verb":"languages"}))
        .get("result")
        .is_some());
    rpc.finish();
}

#[test]
fn terminating_the_process_aborts_the_session_and_a_new_engine_can_start() {
    let temp = tempfile::tempdir().expect("temp");
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../dctx/tests/fixtures/repository");
    let mut rpc = Server::new(command(temp.path(), &repo).arg("rpc"));
    assert!(rpc
        .call(json!({"id":1,"verb":"hello"}))
        .get("result")
        .is_some());
    rpc.child.kill().expect("terminate engine");
    assert!(!rpc.child.wait().expect("wait for termination").success());
    let mut restarted = Server::new(command(temp.path(), &repo).arg("rpc"));
    assert!(restarted
        .call(json!({"id":1,"verb":"hello"}))
        .get("result")
        .is_some());
    assert!(restarted
        .call(json!({"id":2,"verb":"find","args":["ping"]}))
        .get("result")
        .is_some());
    restarted.finish();
}
