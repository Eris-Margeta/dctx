// SPDX-License-Identifier: Apache-2.0
//! Restricted MCP profiles transport the full profile's exact engine bytes.
mod common;

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

struct Server {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}

impl Server {
    fn new(root: &Path, place: &Path, args: &[&str]) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_dctx-cli"));
        command
            .args(args)
            .current_dir(place)
            .env_clear()
            .env("DCTX_AUTO_CLEAN", "0")
            .env("DCTX_CACHE_DIR", root.join("cache"));
        common::isolated_dirs(&mut command, root);
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("MCP server");
        Self {
            input: child.stdin.take().expect("stdin"),
            output: BufReader::new(child.stdout.take().expect("stdout")),
            child,
        }
    }

    fn request(&mut self, request: &Value) -> Vec<u8> {
        writeln!(self.input, "{request}").expect("request");
        self.input.flush().expect("flush");
        let mut reply = Vec::new();
        assert!(self.output.read_until(b'\n', &mut reply).expect("reply") > 0);
        reply
    }

    fn finish(self) {
        drop(self.input);
        let output = self.child.wait_with_output().expect("exit");
        assert!(output.status.success(), "{output:?}");
        assert!(output.stderr.is_empty(), "{output:?}");
    }
}

fn decode(reply: &[u8]) -> Value {
    serde_json::from_slice(reply).expect("JSON reply")
}

#[test]
fn three_tools_have_full_profile_byte_parity_and_reject_every_other_tool() {
    let fixture = tempfile::tempdir().expect("fixture");
    let place = fixture.path().join("place");
    std::fs::create_dir(&place).expect("place");
    let source = "pub fn beacon() -> &'static str {\n    \"héllo \\\"MCP\\\" \\\\ path\"\n}\n";
    std::fs::write(place.join("sample.rs"), source).expect("source");
    let mut full = Server::new(fixture.path(), &place, &["mcp"]);
    let list = json!({"jsonrpc":"2.0","id":1,"method":"tools/list"});
    let full_tools = decode(&full.request(&list))["result"]["tools"].clone();

    // The aliases and inline flag use the same selector as `dctx mcp`.
    for args in [
        vec!["mcp", "--profile", "three"],
        vec!["serve", "--profile=three"],
        vec!["serve", "--mcp", "--profile", "three"],
    ] {
        let mut three = Server::new(fixture.path(), &place, &args);
        let listed = decode(&three.request(&list));
        let selected = listed["result"]["tools"].as_array().expect("tools");
        let names: Vec<_> = selected
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["map", "find", "read"]);
        for tool in full_tools.as_array().expect("full tools") {
            if let Some(selected) = selected.iter().find(|t| t["name"] == tool["name"]) {
                assert_eq!(selected["inputSchema"], tool["inputSchema"]);
            } else {
                let reply = decode(&three.request(&json!({
                    "jsonrpc":"2.0","id":2,"method":"tools/call",
                    "params":{"name":tool["name"],"arguments":{}}
                })));
                assert_eq!(reply["error"]["code"], -32602, "{reply}");
                assert!(reply["result"].is_null(), "{reply}");
            }
        }
        for json_output in [false, true] {
            for (name, mut arguments) in [
                ("map", json!({"target":"sample.rs","depth":2,"limit":5})),
                (
                    "find",
                    json!({"query":"beacon","kind":["fn"],"in":"sample.rs","limit":5}),
                ),
                (
                    "read",
                    json!({"id":"m:sample.rs","lines":"2-2","full":true}),
                ),
            ] {
                arguments["json"] = json!(json_output);
                arguments["budget"] = json!(2000);
                let request = json!({"jsonrpc":"2.0","id":"parity","method":"tools/call",
                    "params":{"name":name,"arguments":arguments}});
                let expected = full.request(&request);
                let result = decode(&expected);
                assert!(result["result"]["isError"].is_null(), "{result}");
                let text = result["result"]["content"][0]["text"]
                    .as_str()
                    .expect("text");
                assert!(
                    text.contains(if name == "read" { "héllo" } else { "beacon" }),
                    "{text}"
                );
                assert_eq!(
                    three.request(&request),
                    expected,
                    "{name}, json={json_output}"
                );

                // Argument errors are transported identically as well.
                let invalid = json!({"jsonrpc":"2.0","id":"bad","method":"tools/call",
                    "params":{"name":name,"arguments":{}}});
                let expected = full.request(&invalid);
                assert_eq!(decode(&expected)["result"]["isError"], true);
                assert_eq!(three.request(&invalid), expected);
            }
        }
        three.finish();
    }
    full.finish();
}
