// SPDX-License-Identifier: Apache-2.0
//! MCP tool arguments are literal values: a flag-shaped id or query never selects another place.
#![cfg(unix)]
mod common;

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

/// Runs one `dctx mcp` session in `place` and returns the text of every tool reply.
fn session(
    root: &std::path::Path,
    place: &std::path::Path,
    calls: &[Value],
) -> Vec<(bool, String)> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_dctx-cli"));
    command
        .arg("mcp")
        .current_dir(place)
        .env_clear()
        .env("DCTX_AUTO_CLEAN", "0")
        .env("DCTX_CACHE_DIR", root.join("cache"));
    common::isolated_dirs(&mut command, root);
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("MCP server");
    let mut input = child.stdin.take().expect("stdin");
    let mut output = BufReader::new(child.stdout.take().expect("stdout"));
    let mut replies = Vec::new();
    for (id, arguments) in calls.iter().enumerate() {
        let request = json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":arguments});
        writeln!(input, "{request}").expect("request");
        input.flush().expect("flush");
        let mut line = Vec::new();
        assert!(output.read_until(b'\n', &mut line).expect("reply") > 0);
        let reply: Value = serde_json::from_slice(&line).expect("JSON reply");
        let text = reply["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        replies.push((reply["result"]["isError"] == json!(true), text));
    }
    drop(input);
    child.wait().expect("exit");
    replies
}

#[test]
fn flag_shaped_ids_and_queries_never_move_the_place_outside_the_root() {
    let fixture = tempfile::tempdir().expect("fixture");
    let outside = fixture.path().join("outside");
    let place = fixture.path().join("place");
    std::fs::create_dir_all(&outside).expect("outside");
    std::fs::create_dir_all(&place).expect("place");
    std::fs::write(
        outside.join("s.py"),
        "def outside_secret():\n    return 'OUTSIDE-SECRET-TOKEN'\n",
    )
    .expect("secret");
    std::fs::write(place.join("inside.py"), "def inside():\n    return 1\n").expect("source");
    // Symlinks named like flags, as a hostile checkout could commit them.
    for name in ["--lines", "--in", "--json", "--depth", "--to"] {
        std::os::unix::fs::symlink(&outside, place.join(name)).expect("symlink");
    }
    let replies = session(
        fixture.path(),
        &place,
        &[
            json!({"name":"read","arguments":{"id":"--place","lines":"s.py"}}),
            json!({"name":"grep","arguments":{"query":"--place","in":"OUTSIDE-SECRET-TOKEN"}}),
            json!({"name":"grep","arguments":{"query":"--place","json":true}}),
            json!({"name":"find","arguments":{"query":"--place","in":"outside_secret"}}),
            json!({"name":"map","arguments":{"target":"--place","depth":1}}),
            json!({"name":"skeleton","arguments":{"target":"--place"}}),
            json!({"name":"neighbors","arguments":{"id":"--place","depth":1}}),
            json!({"name":"explain","arguments":{"id":"--place"}}),
            json!({"name":"path","arguments":{"from":"--place","to":"s.py"}}),
            json!({"name":"read","arguments":{"id":"--place","context":true}}),
        ],
    );
    for (error, text) in &replies {
        assert!(!text.contains("OUTSIDE-SECRET-TOKEN"), "{error} {text}");
        assert!(!text.contains("outside_secret"), "{error} {text}");
    }
    // A flag-shaped query is searched for literally inside the root.
    std::fs::write(place.join("flags.py"), "# pass --place to pick a root\n").expect("flags");
    let replies = session(
        fixture.path(),
        &place,
        &[json!({"name":"grep","arguments":{"query":"--place"}})],
    );
    let (error, text) = &replies[0];
    assert!(!error && text.contains("flags.py"), "{text}");
}
