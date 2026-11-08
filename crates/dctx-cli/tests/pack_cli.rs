// SPDX-License-Identifier: Apache-2.0
//! Binary pack of two source files through the CLI.
//! Gap G1 of DCTX-SERIOUS-GAPS.md: an explicit pack carries each path and body.
//! Depends on the compiled `dctx-cli` binary and a private temporary directory.
//! Used by `cargo nextest run -p dctx-cli`.
//! `--output` bytes equal the stdout payload; the status line stays on stdout.

mod common;

use std::ffi::OsStr;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Per-process counter: `SystemTime` has microsecond granularity on macOS, so two tests of this
/// binary can read the same clock value and would otherwise share (and delete) one fixture root.
static NEXT: AtomicU64 = AtomicU64::new(1);

struct Fixture {
    root: PathBuf,
    repo: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let serial = NEXT.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "dctx-pack-cli-{}-{nonce}-{serial}",
            std::process::id()
        ));
        let repo = root.join("repo");
        fs::create_dir_all(repo.join("nested")).expect("fixture");
        for folder in ["home", "config", "cache", "data", "tmp"] {
            fs::create_dir_all(root.join(folder)).expect("isolated directory");
        }
        fs::write(repo.join("alpha.rs"), "fn alpha_body() { 11 }\n").expect("alpha");
        fs::write(repo.join("nested/beta.txt"), "beta body line\n").expect("beta");
        Self { root, repo }
    }

    fn run(&self, args: &[&str]) -> Vec<u8> {
        let mut command = Command::new(env!("CARGO_BIN_EXE_dctx-cli"));
        command.current_dir(&self.repo).env_clear().args(args);
        common::isolated_dirs(&mut command, &self.root);
        command.env("DCTX_CACHE_DIR", self.root.join("cache"));
        command.env("DCTX_AUTO_CLEAN", "0");
        command.env("NO_COLOR", "1");
        command.env("PATH", "");
        assert!(command.get_envs().any(|(key, value)| {
            key == OsStr::new("DCTX_AUTO_CLEAN") && value == Some(OsStr::new("0"))
        }));
        let output = command.output().expect("run compiled CLI");
        assert!(
            output.status.success(),
            "{args:?}: status {}; stdout bytes {}; stderr {}",
            output.status,
            output.stdout.len(),
            String::from_utf8_lossy(&output.stderr),
        );
        assert!(
            output.stderr.is_empty(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output.stdout
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn payload(stdout: &[u8]) -> &[u8] {
    let marker = b"pack bytes=";
    let pos = stdout
        .windows(marker.len())
        .rposition(|window| window == marker)
        .expect("status line");
    &stdout[..pos]
}

fn contains(bytes: &[u8], text: &str) -> bool {
    bytes
        .windows(text.len())
        .any(|window| window == text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_binary_emits_both_paths_and_bodies_and_output_matches_stdout() {
        let fixture = Fixture::new();
        let stdout = fixture.run(&["pack", "."]);
        let page = payload(&stdout);
        let status = &stdout[page.len()..];
        assert!(
            status.starts_with(b"pack bytes="),
            "{}",
            String::from_utf8_lossy(status)
        );
        assert!(contains(status, "retention=stdout"));
        for text in [
            "alpha.rs",
            "fn alpha_body() { 11 }",
            "nested/beta.txt",
            "beta body line",
        ] {
            assert!(
                contains(page, text),
                "missing {text} in {}",
                String::from_utf8_lossy(page)
            );
        }

        let output = fixture.root.join("pack-out.txt");
        let reported = fixture.run(&["pack", ".", "--output", output.to_str().expect("utf8 path")]);
        assert!(
            reported.starts_with(b"pack bytes="),
            "{}",
            String::from_utf8_lossy(&reported)
        );
        assert!(contains(&reported, "retention=explicit"));
        let saved = fs::read(&output).expect("output file");
        assert_eq!(saved, page);
        assert!(saved.starts_with(b"[DCTX_LLM_CONTEXT_SNAPSHOT_"));
        assert!(output.is_file());
        assert!(!fixture.repo.join(".dctx").exists());
    }
}
