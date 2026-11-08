// SPDX-License-Identifier: Apache-2.0
//! Source confinement is shared by each executable surface.
#![cfg(unix)]
mod common;

use serde_json::{json, Value};
use std::io::Write;
use std::process::{Command, Output, Stdio};

struct Fixture(tempfile::TempDir);
impl Fixture {
    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_dctx-cli"));
        command.env_clear().current_dir(self.0.path().join("place"));
        common::isolated_dirs(&mut command, self.0.path());
        command
            .env("DCTX_CACHE_DIR", self.0.path().join("c"))
            .env("DCTX_AUTO_CLEAN", "0");
        command
    }

    fn server(&self, verb: &str, requests: &[Value]) -> Output {
        let mut child = self
            .command()
            .arg(verb)
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
        child.wait_with_output().unwrap()
    }
}

fn check(output: &Output) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stdout.contains("OUTSIDE_SENTINEL"), "{stdout}");
    assert!(!stderr.contains("OUTSIDE_SENTINEL"), "{stderr}");
}

#[test]
fn cli_mcp_and_rpc_do_not_return_symlink_or_traversal_targets() {
    let fixture = Fixture(tempfile::tempdir().unwrap());
    let root = fixture.0.path().join("place");
    let outside = fixture.0.path().join("outside");
    std::fs::create_dir(&root).unwrap();
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(root.join("kept.rs"), b"pub fn kept() {}\n").unwrap();
    std::fs::write(outside.join("secret.rs"), b"pub fn OUTSIDE_SENTINEL() {}\n").unwrap();
    std::os::unix::fs::symlink(outside.join("secret.rs"), root.join("escape.rs")).unwrap();
    std::os::unix::fs::symlink(&outside, root.join("linked")).unwrap();
    let targets = [
        "m:kept.rs",
        "m:escape.rs",
        "m:linked/secret.rs",
        "m:../outside/secret.rs",
    ];
    for target in targets {
        let output = fixture
            .command()
            .args(["map", "read", target, "--json"])
            .output()
            .unwrap();
        check(&output);
        if target == "m:kept.rs" {
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(String::from_utf8_lossy(&output.stdout).contains("pub fn kept"));
        }
    }
    let mut rpc = vec![json!({"id":0,"verb":"hello"})];
    let mut mcp = Vec::new();
    for (id, target) in targets.into_iter().enumerate() {
        rpc.push(json!({"id":id+1,"verb":"read","args":[target]}));
        mcp.push(json!({"jsonrpc":"2.0","id":id+1,"method":"tools/call","params":{"name":"read","arguments":{"id":target,"json":true}}}));
    }
    for (verb, requests) in [("rpc", rpc), ("mcp", mcp)] {
        let output = fixture.server(verb, &requests);
        check(&output);
        assert!(output.status.success());
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains("pub fn kept"), "{verb}: {text}");
        assert_eq!(text.lines().count(), requests.len());
        for line in text.lines() {
            serde_json::from_str::<Value>(line).unwrap();
        }
    }
}

#[cfg(target_os = "linux")]
#[test]
fn sparse_template_and_offline_bundle_inputs_are_refused() {
    let fixture = Fixture(tempfile::tempdir().unwrap());
    std::fs::create_dir(fixture.0.path().join("place")).unwrap();
    let input = fixture.0.path().join("sparse");
    std::fs::File::create(&input)
        .unwrap()
        .set_len(1 << 40)
        .unwrap();
    let output = fixture
        .command()
        .args(["template", "build"])
        .arg(&input)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("byte limit"));
    let output = fixture
        .command()
        .args(["template", "list", "--dir"])
        .arg(fixture.0.path())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("byte limit"));
    let output = fixture
        .command()
        .args(["plugin", "install", "--id", "example.test", "--file"])
        .arg(&input)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("limit of"));
}
