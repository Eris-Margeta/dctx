// SPDX-License-Identifier: Apache-2.0
//! The executable's CLI and MCP transports share direct query output and policy.

use std::io::Write;
use std::process::{Command, Stdio};

use dctx_plugin_protocol::{decode_object, DecodeLimits, Json};

fn member<'a>(value: &'a Json, key: &str) -> &'a Json {
    let Json::Object(fields) = value else {
        panic!("object: {value:?}");
    };
    &fields
        .iter()
        .find(|(name, _)| name == key)
        .expect("member")
        .1
}

#[test]
fn cli_and_mcp_read_identical_bytes_without_opening_an_unusable_cache_or_sibling_rules() {
    let fixture = tempfile::tempdir().expect("fixture");
    let place = fixture.path().join("place");
    std::fs::create_dir_all(place.join("src/nested")).expect("dirs");
    std::fs::create_dir_all(place.join("unrelated")).expect("sibling");
    std::fs::write(place.join(".gitignore"), "src/nested/blocked.py\n").expect("root rules");
    std::fs::write(place.join("src/.gitignore"), "*.py\n!kept.py\n").expect("ancestor rules");
    std::fs::write(
        place.join("src/nested/kept.py"),
        "def target():\n    return 'needle'\n",
    )
    .expect("file");
    std::fs::write(
        place.join("src/nested/blocked.py"),
        "def hidden():\n    return 'SECRET'\n",
    )
    .expect("hidden");
    std::fs::write(place.join("unrelated/.gitignore"), [0xff]).expect("invalid unrelated rule");
    let cache = fixture.path().join("unusable-cache");
    std::fs::write(&cache, "a file cannot be a cache directory").expect("bad cache");
    let command = || {
        let mut command = Command::new(env!("CARGO_BIN_EXE_dctx-cli"));
        command
            .current_dir(&place)
            .env("XDG_CONFIG_HOME", fixture.path().join("config"))
            .env("DCTX_CACHE_DIR", &cache)
            .env_remove("DCTX_PROFILE")
            .env_remove("DCTX_REUSE_REPORT");
        command
    };
    let cli = command()
        .args(["read", "s:src/nested/kept.py#target"])
        .output()
        .expect("CLI");
    assert!(
        cli.status.success(),
        "{}",
        String::from_utf8_lossy(&cli.stderr)
    );
    assert!(cli.stderr.is_empty());
    let mut child = command()
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("MCP");
    child.stdin.take().expect("stdin").write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{}}\n{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{\"name\":\"read\",\"arguments\":{\"id\":\"s:src/nested/kept.py#target\"}}}\n").expect("requests");
    let output = child.wait_with_output().expect("response");
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let response = std::str::from_utf8(&output.stdout)
        .expect("UTF8")
        .lines()
        .nth(1)
        .expect("tool reply");
    let response = decode_object(response.as_bytes(), DecodeLimits::WIRE).expect("JSON");
    let result = member(&response, "result");
    let Json::Array(content) = member(result, "content") else {
        panic!("content array");
    };
    let Json::Str(text) = member(&content[0], "text") else {
        panic!("text");
    };
    assert_eq!(text.as_bytes(), cli.stdout);
    for args in [
        vec!["map", "find", "needle", "--in", "src/nested"],
        vec!["map", "grep", "needle", "--in", "src/nested"],
        vec!["map", "skeleton", "src/nested/kept.py"],
    ] {
        let output = command().args(args).output().expect("query");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = String::from_utf8(output.stdout).expect("UTF8");
        assert!(text.contains("kept.py"), "{text}");
        assert!(
            !text.contains("SECRET") && !text.contains("hidden"),
            "{text}"
        );
    }
    assert_eq!(
        std::fs::read_to_string(cache).expect("cache unchanged"),
        "a file cannot be a cache directory"
    );
}
