// SPDX-License-Identifier: Apache-2.0
//! Pack filter acceptance for `dctx pack`.
//! Implements ADR-011 and ADR-009: each flag narrows one fixture to exact paths.
//! Depends on the compiled `dctx-cli` binary and a private temporary directory.
//! Used by `cargo nextest run -p dctx-cli`.
//! The default page omits the binary file; `--binary` is the opt-in.

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

struct Fixture {
    root: PathBuf,
    repo: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).expect("clock").as_nanos();
        let root = std::env::temp_dir().join(format!("dctx-pack-filters-{}-{nonce}", std::process::id()));
        let repo = root.join("repo");
        fs::create_dir_all(repo.join("dist")).expect("fixture");
        for folder in ["home", "config", "cache", "data", "tmp"] {
            fs::create_dir_all(root.join(folder)).expect("isolated directory");
        }
        fs::write(repo.join("lib.rs"), "fn lib_marker() {}\n").expect("rs");
        fs::write(repo.join("notes.md"), "notes_marker\n").expect("md");
        fs::write(repo.join("empty.md"), "").expect("empty");
        fs::write(repo.join(".secret.md"), "secret_marker\n").expect("hidden");
        fs::write(repo.join("dist/skipped.txt"), "skipped_marker\n").expect("ignored");
        let mut png = b"\x89PNG\0png_marker".to_vec();
        png.extend_from_slice(&[0, 1, 2, 3]);
        fs::write(repo.join("logo.png"), png).expect("png");
        let mut big = b"big_marker\n".to_vec();
        big.resize(1_048_576 + 64, b'a');
        fs::write(repo.join("big.txt"), big).expect("big");
        Self { root, repo }
    }

    fn pack(&self, args: &[&str]) -> String {
        let mut command = Command::new(env!("CARGO_BIN_EXE_dctx-cli"));
        command.current_dir(&self.repo).env_clear().arg("pack").arg(self.repo.as_os_str()).args(args);
        for (variable, folder) in [
            ("HOME", "home"),
            ("USERPROFILE", "home"),
            ("XDG_CONFIG_HOME", "config"),
            ("XDG_CACHE_HOME", "cache"),
            ("XDG_DATA_HOME", "data"),
            ("DCTX_CACHE_DIR", "cache"),
            ("TMPDIR", "tmp"),
            ("TMP", "tmp"),
            ("TEMP", "tmp"),
        ] {
            command.env(variable, self.root.join(folder));
        }
        command.env("DCTX_AUTO_CLEAN", "0");
        command.env("NO_COLOR", "1");
        command.env("PATH", "");
        assert!(command.get_envs().any(|(key, value)| {
            key == OsStr::new("DCTX_AUTO_CLEAN") && value == Some(OsStr::new("0"))
        }));
        let output = command.output().expect("run compiled CLI");
        assert!(
            output.status.success(),
            "{args:?}: status {}; stderr {}",
            output.status,
            String::from_utf8_lossy(&output.stderr),
        );
        assert!(output.stderr.is_empty(), "{args:?}: {}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8(output.stdout).expect("utf8 stdout")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn payload(stdout: &str) -> &str {
    stdout.rfind("pack bytes=").map(|pos| &stdout[..pos]).expect("status line")
}

fn file_paths(page: &str) -> BTreeSet<String> {
    let start = page.find("<DIRECTORY_TREE>").expect("tree");
    let rest = &page[start..];
    let end = rest.find("</DIRECTORY_TREE>").expect("tree end");
    let mut paths = BTreeSet::new();
    for line in rest[..end].lines() {
        let Some(rest) = line.trim_start().strip_prefix("[F] ") else {
            continue;
        };
        let head = rest.split(" (ID:").next().unwrap_or(rest);
        let path = head.strip_suffix(" [IGNORED]").unwrap_or(head).trim();
        if !path.is_empty() {
            paths.insert(path.to_string());
        }
    }
    paths
}

fn status_files(stdout: &str) -> u64 {
    let pos = stdout.rfind("pack bytes=").expect("status");
    let line = &stdout[pos..];
    let files = line.split("files=").nth(1).and_then(|rest| rest.split_whitespace().next()).expect("files");
    files.parse().expect("files number")
}

fn expect_paths(stdout: &str, expected: &[&str]) {
    let page = payload(stdout);
    let got = file_paths(page);
    let want: BTreeSet<_> = expected.iter().map(|path| (*path).to_string()).collect();
    assert_eq!(got, want, "{page}");
    assert_eq!(status_files(stdout), u64::try_from(want.len()).expect("len"));
    assert!(!page.contains("png_marker"), "binary bytes leaked");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_paths() -> [&'static str; 5] {
        ["big.txt", "empty.md", "lib.rs", "notes.md", ".secret.md"]
    }

    #[test]
    fn each_filter_narrows_to_exact_paths_and_default_omits_binary() {
        let fixture = Fixture::new();
        let packed = fixture.pack(&[]);
        expect_paths(&packed, &default_paths());
        assert!(payload(&packed).contains("fn lib_marker() {}"));
        assert!(payload(&packed).contains("notes_marker"));
        assert!(payload(&packed).contains("big_marker"));
        assert!(payload(&packed).contains("secret_marker"));
        assert!(!payload(&packed).contains("logo.png"));
        assert!(!payload(&packed).contains("skipped_marker"));

        expect_paths(&fixture.pack(&["--ext", "rs"]), &["lib.rs"]);
        expect_paths(&fixture.pack(&["--ext", "RS"]), &["lib.rs"]);
        expect_paths(&fixture.pack(&["--ext", "md"]), &[".secret.md", "empty.md", "notes.md"]);
        expect_paths(&fixture.pack(&["--empty"]), &["empty.md"]);
        expect_paths(&fixture.pack(&["--non-empty"]), &[".secret.md", "big.txt", "lib.rs", "notes.md"]);
        expect_paths(&fixture.pack(&["--large", "1"]), &["big.txt"]);
        expect_paths(&fixture.pack(&["--small", "1"]), &[".secret.md", "empty.md", "lib.rs", "notes.md"]);
        expect_paths(&fixture.pack(&["--hidden"]), &[".secret.md"]);
        expect_paths(&fixture.pack(&["--include", "*.rs"]), &["lib.rs"]);
        expect_paths(&fixture.pack(&["--exclude", "*.md"]), &["big.txt", "lib.rs"]);
        expect_paths(&fixture.pack(&["--include", "*.png"]), &[]);
        let binary_only = fixture.pack(&["--include", "*.png", "--binary"]);
        expect_paths(&binary_only, &["logo.png"]);
        assert!(payload(&binary_only).contains("[Binary file:"));

        let with_binary = fixture.pack(&["--binary"]);
        expect_paths(
            &with_binary,
            &[".secret.md", "big.txt", "empty.md", "lib.rs", "logo.png", "notes.md"],
        );
        assert!(payload(&with_binary).contains("PATH=\"logo.png\""));
        assert!(payload(&with_binary).contains("[Binary file:"));

        let with_ignored = fixture.pack(&["--debug-ignored"]);
        expect_paths(
            &with_ignored,
            &[".secret.md", "big.txt", "dist/skipped.txt", "empty.md", "lib.rs", "notes.md"],
        );
        assert!(payload(&with_ignored).contains("dist/skipped.txt"));
        assert!(!payload(&with_ignored).contains("skipped_marker"));
    }

    #[test]
    fn pack_help_names_every_flag() {
        let fixture = Fixture::new();
        let mut command = Command::new(env!("CARGO_BIN_EXE_dctx-cli"));
        command.current_dir(&fixture.repo).env_clear().args(["pack", "--help"]);
        command.env("DCTX_AUTO_CLEAN", "0");
        command.env("HOME", fixture.root.join("home"));
        command.env("PATH", "");
        let output = command.output().expect("help");
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        let text = String::from_utf8(output.stdout).expect("utf8");
        assert!(text.contains("Pack filters"));
        for flag in [
            "--ext",
            "--binary",
            "--empty",
            "--non-empty",
            "--large",
            "--small",
            "--hidden",
            "--debug-ignored",
            "--include",
            "--exclude",
            "--output",
            "-o",
            "--clipboard",
            "-c",
            "--json",
        ] {
            assert!(text.contains(flag), "missing {flag}");
        }
    }
}
