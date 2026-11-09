// SPDX-License-Identifier: Apache-2.0
//! Binary pack of two source files through the CLI.
//! Gap G1 of DCTX-SERIOUS-GAPS.md: an explicit pack carries each path and body.
//! Depends on the compiled `dctx-cli` binary and a private temporary directory.
//! Used by `cargo nextest run -p dctx-cli`.
//! `--output` bytes equal stdout, which is exactly the page; the status line goes to stderr.

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

    fn command(&self, args: &[&str]) -> Command {
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
        command
    }

    /// Runs a successful pack; returns stdout and the one stderr status line.
    fn run(&self, args: &[&str]) -> (Vec<u8>, String) {
        let output = self.command(args).output().expect("run compiled CLI");
        assert!(
            output.status.success(),
            "{args:?}: status {}; stdout bytes {}; stderr {}",
            output.status,
            output.stdout.len(),
            String::from_utf8_lossy(&output.stderr),
        );
        let stderr = String::from_utf8(output.stderr).expect("utf8 stderr");
        assert!(
            stderr.starts_with("pack bytes=")
                && stderr.ends_with('\n')
                && stderr.lines().count() == 1,
            "{args:?}: {stderr}"
        );
        (output.stdout, stderr)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
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
        let (page, status) = fixture.run(&["pack", "."]);
        assert!(page.starts_with(b"[DCTX_LLM_CONTEXT_SNAPSHOT_"));
        assert!(
            page.ends_with(b"\">\n"),
            "{}",
            String::from_utf8_lossy(&page)
        );
        assert!(!contains(&page, "pack bytes="));
        assert!(status.contains("retention=stdout"), "{status}");
        assert!(
            status.contains(&format!("pack bytes={} ", page.len())),
            "{status}"
        );
        for text in [
            "alpha.rs",
            "fn alpha_body() { 11 }",
            "nested/beta.txt",
            "beta body line",
        ] {
            assert!(
                contains(&page, text),
                "missing {text} in {}",
                String::from_utf8_lossy(&page)
            );
        }
        // Every line, including the last end marker, is a whole line for line-based readers.
        let text = String::from_utf8(page.clone()).expect("utf8 page");
        let last = text.lines().last().expect("last line");
        assert!(last.starts_with("</FILE_CONTENT_END ID=\"F"), "{last}");

        let output = fixture.root.join("pack-out.txt");
        let (reported, status) =
            fixture.run(&["pack", ".", "--output", output.to_str().expect("utf8 path")]);
        assert!(
            reported.is_empty(),
            "{}",
            String::from_utf8_lossy(&reported)
        );
        assert!(status.contains("retention=explicit"), "{status}");
        let saved = fs::read(&output).expect("output file");
        assert_eq!(saved, page);
        assert!(output.is_file());
        assert!(!fixture.repo.join(".dctx").exists());
    }

    #[test]
    fn pack_json_stdout_is_exactly_one_json_document() {
        let fixture = Fixture::new();
        let (stdout, status) = fixture.run(&["pack", ".", "--json"]);
        assert!(
            stdout.ends_with(b"}\n"),
            "{}",
            String::from_utf8_lossy(&stdout)
        );
        let document: serde_json::Value =
            serde_json::from_slice(&stdout).expect("stdout is one JSON document");
        assert_eq!(document["files"], 2, "{document}");
        assert!(
            status.starts_with(&format!("pack bytes={} files=2 ", stdout.len())),
            "{status}"
        );
    }

    #[test]
    fn missing_clipboard_tool_names_the_tools_and_suggests_output() {
        let fixture = Fixture::new();
        let output = fixture
            .command(&["pack", ".", "--clipboard"])
            .env("DISPLAY", "")
            .env("WAYLAND_DISPLAY", "")
            .output()
            .expect("run compiled CLI");
        assert_eq!(output.status.code(), Some(3), "{output:?}");
        assert!(output.stdout.is_empty(), "{output:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("no clipboard tool found on PATH (tried "),
            "{stderr}"
        );
        assert!(stderr.contains("--output FILE"), "{stderr}");
        assert!(!stderr.contains("PATH is an existing"), "{stderr}");
    }

    #[test]
    fn output_refuses_a_directory_target_and_replaces_a_regular_file() {
        let fixture = Fixture::new();
        let directory = fixture.root.join("out-dir");
        fs::create_dir(&directory).expect("directory target");
        let output = fixture
            .command(&[
                "pack",
                ".",
                "--output",
                directory.to_str().expect("utf8 path"),
            ])
            .output()
            .expect("run compiled CLI");
        assert_eq!(output.status.code(), Some(3), "{output:?}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("non-regular"));
        let target = fixture.root.join("context.txt");
        fs::write(
            &target,
            "old context that is longer than nothing at all\n".repeat(400),
        )
        .expect("old");
        fixture.run(&["pack", ".", "-o", target.to_str().expect("utf8 path")]);
        let saved = fs::read(&target).expect("saved");
        assert!(saved.starts_with(b"[DCTX_LLM_CONTEXT_SNAPSHOT_"));
        assert!(!contains(&saved, "old context"));
    }

    #[cfg(unix)]
    #[test]
    fn output_never_writes_through_a_symlink() {
        let fixture = Fixture::new();
        let victim = fixture.root.join("home/.bashrc");
        fs::write(&victim, "keep me\n").expect("victim");
        let link = fixture.repo.join("context.txt");
        std::os::unix::fs::symlink(&victim, &link).expect("link");
        let output = fixture
            .command(&["pack", ".", "-o", "context.txt"])
            .output()
            .expect("run compiled CLI");
        assert_eq!(output.status.code(), Some(3), "{output:?}");
        assert_eq!(fs::read(&victim).expect("victim"), b"keep me\n");
        assert!(fs::symlink_metadata(&link)
            .expect("link")
            .file_type()
            .is_symlink());
    }
}
