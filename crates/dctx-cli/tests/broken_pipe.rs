// SPDX-License-Identifier: Apache-2.0
//! DCX-P-C4: a reader that closes stdout early (`dctx pack . | head -1`) ends the run with exit 0
//! and no panic message, as `cat` and `rg` do. The test closes the read end before the child writes,
//! so the first write fails with EPIPE every time.
//! Depends on the compiled `dctx-cli` binary and a private temporary directory.
//! Run by `cargo test -p dctx-cli broken_pipe`.

#![cfg(unix)]

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Per-process counter: `SystemTime` has microsecond granularity on macOS, so two tests of this
/// binary can read the same clock value and would otherwise share (and delete) one fixture root.
static NEXT: AtomicU64 = AtomicU64::new(1);

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let serial = NEXT.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "dctx-broken-pipe-{}-{nonce}-{serial}",
            std::process::id()
        ));
        for folder in ["home", "config", "cache", "data", "tmp", "repo"] {
            fs::create_dir_all(root.join(folder)).expect("isolated directory");
        }
        fs::write(root.join("repo/alpha.rs"), "fn alpha_body() { 11 }\n").expect("alpha");
        fs::write(root.join("repo/beta.txt"), "beta body line\n".repeat(200)).expect("beta");
        Self { root }
    }

    /// Runs the CLI with its stdout closed on the reader side before it can write.
    fn run_with_closed_stdout(&self, args: &[&str]) -> (Option<i32>, String) {
        let mut command = Command::new(env!("CARGO_BIN_EXE_dctx-cli"));
        command
            .current_dir(self.root.join("repo"))
            .env_clear()
            .args(args);
        common::isolated_dirs(&mut command, &self.root);
        command.env("DCTX_CACHE_DIR", self.root.join("cache"));
        command
            .env("DCTX_AUTO_CLEAN", "0")
            .env("NO_COLOR", "1")
            .env("PATH", "");
        let mut child = command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn CLI");
        drop(child.stdout.take());
        let output = child.wait_with_output().expect("wait");
        (
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(Path::new(&self.root));
    }
}

#[test]
fn broken_pipe_on_pack_exits_zero_without_a_panic() {
    let fixture = Fixture::new();
    let (code, stderr) = fixture.run_with_closed_stdout(&["pack", "."]);
    assert!(!stderr.contains("panicked"), "stderr: {stderr}");
    assert!(!stderr.contains("Broken pipe"), "stderr: {stderr}");
    assert_eq!(code, Some(0), "stderr: {stderr}");
}

#[test]
fn broken_pipe_on_help_exits_zero_without_a_panic() {
    let fixture = Fixture::new();
    let (code, stderr) = fixture.run_with_closed_stdout(&["--help"]);
    assert!(!stderr.contains("panicked"), "stderr: {stderr}");
    assert_eq!(code, Some(0), "stderr: {stderr}");
}

#[test]
fn broken_pipe_does_not_hide_other_failures() {
    let fixture = Fixture::new();
    let (code, stderr) = fixture.run_with_closed_stdout(&["pack", "--no-such-flag"]);
    assert_eq!(code, Some(2), "usage errors keep exit 2: {stderr}");
    assert!(stderr.contains("unknown pack flag"), "stderr: {stderr}");
}
