// SPDX-License-Identifier: Apache-2.0
//! Two CLI processes reuse one generation, then a same-stamp edit rebuilds.
//! Covers DCTX-SERIOUS-GAPS G6. The content check is `digest_text`, as in `read_verified`.
//! Depends on the compiled dctx-cli and an absolute `DCTX_CACHE_DIR`. No network.
//! The second process prints `generation reused` and a lower `content_reads` count.
//! A same-length edit restored to the old mtime still shows the new symbol.

#![forbid(unsafe_code)]

use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(1);

struct Repository {
    root: PathBuf,
}

impl Repository {
    fn new() -> Self {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("dctx-e02d-warm-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("repo")).expect("repo");
        for folder in ["home", "config", "cache", "plugins", "map-cache", "local-app-data", "empty-path", "tmp"] {
            fs::create_dir(root.join(folder)).expect("isolated directory");
        }
        Self { root }
    }

    fn repo(&self) -> PathBuf {
        self.root.join("repo")
    }

    fn cache(&self) -> PathBuf {
        self.root.join("cache")
    }

    fn output(&self, args: &[&str]) -> Output {
        self.output_env(args, &[])
    }

    fn output_env(&self, args: &[&str], extra: &[(&str, &str)]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_dctx-cli"));
        command
            .current_dir(self.repo())
            .env_clear()
            .args(args)
            .env("DCTX_AUTO_CLEAN", "0")
            .env("DCTX_REUSE_REPORT", "1")
            .env("NO_COLOR", "1");
        for (variable, folder) in [
            ("HOME", "home"),
            ("USERPROFILE", "home"),
            ("XDG_CONFIG_HOME", "config"),
            ("XDG_CACHE_HOME", "cache"),
            ("LOCALAPPDATA", "local-app-data"),
            ("DCTX_PLUGIN_ROOT", "plugins"),
            ("DCTX_CACHE_DIR", "cache"),
            ("DCTX_MAP_CACHE", "map-cache"),
            ("PATH", "empty-path"),
            ("TMPDIR", "tmp"),
            ("TMP", "tmp"),
            ("TEMP", "tmp"),
        ] {
            command.env(variable, self.root.join(folder));
        }
        command.envs(extra.iter().copied());
        command.output().expect("run compiled CLI")
    }

    fn run(&self, args: &[&str]) -> (String, String) {
        let output = self.output(args);
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        assert!(output.status.success(), "{args:?}: status {}; stdout: {stdout}; stderr: {stderr}", output.status);
        (stdout, stderr)
    }
}

impl Drop for Repository {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn count_line(stderr: &str, label: &str) -> u64 {
    stderr
        .lines()
        .find_map(|line| line.strip_prefix(label)?.trim().parse().ok())
        .unwrap_or_else(|| panic!("missing {label} in {stderr}"))
}

fn records(page: &str) -> &str {
    let start = page.find("\"records\":").unwrap_or_else(|| panic!("records in {page}"));
    let tail = &page[start..];
    let end = tail.find(",\"more\"").unwrap_or_else(|| panic!("more in {page}"));
    &page[start..start + end]
}

#[test]
fn a_second_process_reuses_until_the_bytes_change() {
    let repo = Repository::new();
    let source = repo.repo().join("item.rs");
    fs::write(&source, b"fn kept() {}\n").expect("source");
    let place = repo.repo().to_string_lossy().into_owned();
    let args = ["map", "find", "kept", "--json", "--place", place.as_str()];

    let (cold, cold_err) = repo.run(&args);
    assert!(cold_err.contains("generation built"), "{cold_err}");
    assert!(!cold_err.contains("generation reused"), "{cold_err}");
    let cold_reads = count_line(&cold_err, "content_reads");
    assert!(cold_reads >= 1, "{cold_err}");
    assert_eq!(count_line(&cold_err, "cached_content_reads"), 0);
    assert!(cold.contains("s:item.rs#kept"), "{cold}");
    let cold_records = records(&cold).to_string();

    let (warm, warm_err) = repo.run(&args);
    assert!(warm_err.contains("generation reused"), "{warm_err}");
    let warm_reads = count_line(&warm_err, "content_reads");
    assert!(warm_reads < cold_reads, "cold {cold_reads} warm {warm_reads}: {warm_err}");
    assert!(count_line(&warm_err, "cached_content_reads") >= 1, "{warm_err}");
    assert_eq!(records(&warm), cold_records);

    let kept_body = b"fn kept() {}\n";
    let lost_body = b"fn lost() {}\n";
    assert_eq!(kept_body.len(), lost_body.len());
    let before = fs::metadata(&source).expect("meta");
    let stamp = before.modified().expect("mtime");
    assert_eq!(before.len(), kept_body.len() as u64);
    {
        let mut file = File::options().write(true).open(&source).expect("open");
        file.write_all(lost_body).expect("write");
    }
    File::options().write(true).open(&source).expect("stamp").set_modified(stamp).expect("restore mtime");
    let after = fs::metadata(&source).expect("meta");
    assert_eq!(after.len(), kept_body.len() as u64);
    assert_eq!(after.modified().expect("restored"), stamp);

    let lost_args = ["map", "find", "lost", "--json", "--place", place.as_str()];
    let (rebuilt, rebuilt_err) = repo.run(&lost_args);
    assert!(rebuilt_err.contains("generation built"), "{rebuilt_err}");
    assert!(!rebuilt_err.contains("generation reused"), "{rebuilt_err}");
    assert!(rebuilt.contains("s:item.rs#lost"), "{rebuilt}");
    assert!(!rebuilt.contains("#kept"), "{rebuilt}");
    assert!(!repo.repo().join(".dctx").exists());
    let listed: Vec<_> = fs::read_dir(repo.repo()).expect("list").flatten().collect();
    assert_eq!(listed.len(), 1);
    assert!(repo.cache().join("ledger.v1").is_file());
}

/// Sets `path` an hour in the past, outside the 2 s racy window, so a stat stamp can vouch for it.
fn age(path: &std::path::Path) {
    let when = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
    File::options().write(true).open(path).expect("open").set_modified(when).expect("set mtime");
}

#[test]
fn a_settled_file_is_trusted_by_its_stamp_until_strict_live_asks_for_the_bytes() {
    let repo = Repository::new();
    let source = repo.repo().join("item.rs");
    fs::write(&source, b"fn kept() {}\n").expect("source");
    age(&source);
    let place = repo.repo().to_string_lossy().into_owned();
    let args = ["map", "find", "kept", "--json", "--place", place.as_str()];
    let (cold, cold_err) = repo.run(&args);
    assert!(cold_err.contains("generation built"), "{cold_err}");

    let (warm, warm_err) = repo.run(&args);
    assert!(warm_err.contains("generation reused"), "{warm_err}");
    assert_eq!(count_line(&warm_err, "stamp_skipped_reads"), 1, "{warm_err}");
    assert_eq!(count_line(&warm_err, "cached_content_reads"), 0, "{warm_err}");
    assert_eq!(records(&warm), records(&cold));

    let output = repo.output_env(&args, &[("DCTX_FRESHNESS", "strict_live")]);
    let strict_err = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(output.status.success(), "{strict_err}");
    assert!(strict_err.contains("generation reused"), "{strict_err}");
    assert_eq!(count_line(&strict_err, "stamp_skipped_reads"), 0, "{strict_err}");
    assert_eq!(count_line(&strict_err, "cached_content_reads"), 1, "{strict_err}");
    assert_eq!(records(&String::from_utf8_lossy(&output.stdout)), records(&cold));

    let typo = repo.output_env(&args, &[("DCTX_FRESHNESS", "strict-live")]);
    assert!(!typo.status.success());
    assert!(String::from_utf8_lossy(&typo.stderr).contains("DCTX_FRESHNESS"), "{}", String::from_utf8_lossy(&typo.stderr));
}

#[test]
fn caps_early_refusal_names_the_cap_and_the_override_passes() {
    let repo = Repository::new();
    for name in ["a.rs", "b.rs", "c.rs", "d.rs"] {
        fs::write(repo.repo().join(name), format!("fn f_{}() {{}}\n", &name[..1])).expect("source");
    }
    let place = repo.repo().to_string_lossy().into_owned();
    let args = ["map", "find", "f_a", "--place", place.as_str()];

    for (variable, resource) in [("DCTX_MAX_RECORDS", "records"), ("DCTX_MAX_ENTRIES", "entries")] {
        let refused = repo.output_env(&args, &[(variable, "2")]);
        let stderr = String::from_utf8_lossy(&refused.stderr).into_owned();
        assert_eq!(refused.status.code(), Some(3), "{variable}: {stderr}");
        assert!(stderr.contains(&format!("{resource} > 2")), "{variable}: {stderr}");
        assert!(stderr.contains(variable), "{variable} is named in the hint: {stderr}");
        assert!(refused.stdout.is_empty(), "{variable}: no partial answer");

        let passed = repo.output_env(&args, &[(variable, "1000")]);
        assert!(passed.status.success(), "{variable}: {}", String::from_utf8_lossy(&passed.stderr));
        assert!(String::from_utf8_lossy(&passed.stdout).contains("f_a"), "{variable}");

        let bad = repo.output_env(&args, &[(variable, "0")]);
        assert_eq!(bad.status.code(), Some(3), "{variable}: {}", String::from_utf8_lossy(&bad.stderr));
        assert!(String::from_utf8_lossy(&bad.stderr).contains(variable), "{variable} is named when its value is invalid");
    }
}
