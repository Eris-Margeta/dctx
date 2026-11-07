// SPDX-License-Identifier: Apache-2.0
//! Two CLI processes reuse one generation, then a same-stamp edit rebuilds.
//! Covers DCTX-SERIOUS-GAPS G6. The content check is `digest_text`, as in `read_verified`.
//! Depends on the compiled dctx-cli and an absolute `DCTX_CACHE_DIR`. No network.
//! The second process prints `generation reused` and a lower `content_reads` count.
//! A same-length edit restored to the old mtime still shows the new symbol.

#![forbid(unsafe_code)]

mod common;

use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
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
        for folder in [
            "home",
            "config",
            "cache",
            "plugins",
            "map-cache",
            "local-app-data",
            "empty-path",
            "tmp",
        ] {
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
        self.command(args, extra)
            .output()
            .expect("run compiled CLI")
    }

    fn command(&self, args: &[&str], extra: &[(&str, &str)]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_dctx-cli"));
        command
            .current_dir(self.repo())
            .env_clear()
            .args(args)
            .env("DCTX_AUTO_CLEAN", "0")
            .env("DCTX_REUSE_REPORT", "1")
            .env("NO_COLOR", "1");
        common::isolated_dirs(&mut command, &self.root);
        for (variable, folder) in [
            ("DCTX_PLUGIN_ROOT", "plugins"),
            ("DCTX_CACHE_DIR", "cache"),
            ("DCTX_MAP_CACHE", "map-cache"),
            ("PATH", "empty-path"),
        ] {
            command.env(variable, self.root.join(folder));
        }
        command.envs(extra.iter().copied());
        command
    }

    fn run(&self, args: &[&str]) -> (String, String) {
        let output = self.output(args);
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        assert!(
            output.status.success(),
            "{args:?}: status {}; stdout: {stdout}; stderr: {stderr}",
            output.status
        );
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
    let start = page
        .find("\"records\":")
        .unwrap_or_else(|| panic!("records in {page}"));
    let tail = &page[start..];
    let end = tail
        .find(",\"more\"")
        .unwrap_or_else(|| panic!("more in {page}"));
    &page[start..start + end]
}

#[test]
fn switching_places_keeps_both_generations_warm_in_one_bounded_cache() {
    let repo = Repository::new();
    // Equal basenames must still have distinct canonical identities.
    let a = repo.repo().join("a/project");
    let b = repo.repo().join("b/project");
    for place in [&a, &b] {
        fs::create_dir_all(place).expect("place");
        fs::write(place.join("item.rs"), "fn kept() {}\n").expect("source");
        age(&place.join("item.rs"));
    }
    let a = a.to_str().expect("path");
    let b = b.to_str().expect("path");
    let mut first_page = String::new();
    for (place, reused) in [(a, false), (b, false), (a, true), (b, true)] {
        let (page, report) = repo.run(&["map", "find", "kept", "--json", "--place", place]);
        assert_eq!(report.contains("generation reused"), reused, "{report}");
        if first_page.is_empty() {
            first_page = records(&page).to_string();
        }
        assert_eq!(records(&page), first_page);
        if reused {
            assert_eq!(count_line(&report, "content_reads"), 0, "{report}");
        }
    }
    let life = dctx_cli::host::load_ledger(&repo.cache()).expect("ledger");
    let status = life.status();
    assert_eq!(status.roots.len(), 2);
    assert!(status.admitted > 0 && status.admitted <= status.hard_cap);

    // Canonical aliases select the same retained root, without another registration.
    let alias = format!("{a}/.");
    let (_, report) = repo.run(&["map", "find", "kept", "--place", &alias]);
    assert!(report.contains("generation reused"), "{report}");
    assert_eq!(
        dctx_cli::host::load_ledger(&repo.cache())
            .expect("ledger")
            .status()
            .roots
            .len(),
        2
    );

    // Registering another place must not give it a separate capacity allowance.
    let cap = 65_536 + status.admitted;
    repo.run(&["cache", "config", "--max-bytes", &cap.to_string()]);
    let c = repo.repo().join("c/project");
    fs::create_dir_all(&c).expect("third place");
    fs::write(c.join("item.rs"), "fn kept() {}\n").expect("source");
    let output = repo.output(&["map", "find", "kept", "--place", c.to_str().expect("path")]);
    // Admission may safely evict an older unleased generation, or refuse if none
    // is reclaimable. Either way the aggregate charge stays inside the same cap.
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(3), "{stderr}");
        assert!(stderr.contains("cache limit"), "{stderr}");
    }
    let after = dctx_cli::host::load_ledger(&repo.cache())
        .expect("bounded ledger")
        .status();
    assert_eq!(after.roots.len(), 3);
    assert_eq!(after.hard_cap, cap);
    assert!(after.admitted <= cap - 65_536);
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
    assert!(
        warm_reads < cold_reads,
        "cold {cold_reads} warm {warm_reads}: {warm_err}"
    );
    assert!(
        count_line(&warm_err, "cached_content_reads") >= 1,
        "{warm_err}"
    );
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
    File::options()
        .write(true)
        .open(&source)
        .expect("stamp")
        .set_modified(stamp)
        .expect("restore mtime");
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
    File::options()
        .write(true)
        .open(path)
        .expect("open")
        .set_modified(when)
        .expect("set mtime");
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
    assert_eq!(
        count_line(&warm_err, "stamp_skipped_reads"),
        1,
        "{warm_err}"
    );
    assert_eq!(
        count_line(&warm_err, "cached_content_reads"),
        0,
        "{warm_err}"
    );
    assert_eq!(records(&warm), records(&cold));

    let output = repo.output_env(&args, &[("DCTX_FRESHNESS", "strict_live")]);
    let strict_err = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(output.status.success(), "{strict_err}");
    assert!(strict_err.contains("generation reused"), "{strict_err}");
    assert_eq!(
        count_line(&strict_err, "stamp_skipped_reads"),
        0,
        "{strict_err}"
    );
    assert_eq!(
        count_line(&strict_err, "cached_content_reads"),
        1,
        "{strict_err}"
    );
    assert_eq!(
        records(&String::from_utf8_lossy(&output.stdout)),
        records(&cold)
    );

    let typo = repo.output_env(&args, &[("DCTX_FRESHNESS", "strict-live")]);
    assert!(!typo.status.success());
    assert!(
        String::from_utf8_lossy(&typo.stderr).contains("DCTX_FRESHNESS"),
        "{}",
        String::from_utf8_lossy(&typo.stderr)
    );
}

#[test]
fn caps_early_refusal_names_the_cap_and_the_override_passes() {
    let repo = Repository::new();
    for name in ["a.rs", "b.rs", "c.rs", "d.rs"] {
        fs::write(
            repo.repo().join(name),
            format!("fn f_{}() {{}}\n", &name[..1]),
        )
        .expect("source");
    }
    let place = repo.repo().to_string_lossy().into_owned();
    let args = ["map", "find", "f_a", "--place", place.as_str()];

    for (variable, resource) in [
        ("DCTX_MAX_RECORDS", "records"),
        ("DCTX_MAX_ENTRIES", "entries"),
    ] {
        let refused = repo.output_env(&args, &[(variable, "2")]);
        let stderr = String::from_utf8_lossy(&refused.stderr).into_owned();
        assert_eq!(refused.status.code(), Some(3), "{variable}: {stderr}");
        assert!(
            stderr.contains(&format!("{resource} > 2")),
            "{variable}: {stderr}"
        );
        assert!(
            stderr.contains(variable),
            "{variable} is named in the hint: {stderr}"
        );
        assert!(refused.stdout.is_empty(), "{variable}: no partial answer");

        let passed = repo.output_env(&args, &[(variable, "1000")]);
        assert!(
            passed.status.success(),
            "{variable}: {}",
            String::from_utf8_lossy(&passed.stderr)
        );
        assert!(
            String::from_utf8_lossy(&passed.stdout).contains("f_a"),
            "{variable}"
        );

        let bad = repo.output_env(&args, &[(variable, "0")]);
        assert_eq!(
            bad.status.code(),
            Some(3),
            "{variable}: {}",
            String::from_utf8_lossy(&bad.stderr)
        );
        assert!(
            String::from_utf8_lossy(&bad.stderr).contains(variable),
            "{variable} is named when its value is invalid"
        );
    }
}

#[test]
fn pack_capacity_refusals_name_the_limit_and_a_working_override() {
    let repo = Repository::new();
    for name in ["a.rs", "b.rs", "c.rs", "d.rs"] {
        fs::write(repo.repo().join(name), "fn kept() {}\n").expect("source");
    }
    for args in [&["pack", "."][..], &["pack", ".", "--json"][..]] {
        for (variable, resource, limit) in [
            ("DCTX_MAX_SOURCE_BYTES", "source_bytes", "1"),
            ("DCTX_MAX_ENTRIES", "entries", "2"),
        ] {
            let refused = repo.output_env(args, &[(variable, limit)]);
            let stderr = String::from_utf8_lossy(&refused.stderr);
            assert_eq!(refused.status.code(), Some(3), "{args:?}: {stderr}");
            assert!(refused.stdout.is_empty(), "no partial answer");
            assert!(stderr.contains(resource), "{stderr}");
            assert!(stderr.contains(variable), "{stderr}");
            assert!(
                stderr.contains("dctx pack <a smaller directory>"),
                "{stderr}"
            );
            assert!(!stderr.contains("check that PATH"), "{stderr}");
            assert!(!stderr.contains("cache gc"), "{stderr}");
            let passed = repo.output_env(args, &[(variable, "1000")]);
            assert!(
                passed.status.success(),
                "{}",
                String::from_utf8_lossy(&passed.stderr)
            );
        }
    }
}

#[test]
fn an_empty_tight_cache_refuses_without_suggesting_useless_gc() {
    let repo = Repository::new();
    fs::write(repo.repo().join("a.rs"), "fn kept() {}\n").expect("source");
    repo.run(&["cache", "config", "--max-bytes", "65536"]);
    for args in [
        &["map", "find", "kept"][..],
        &["pack", "--for", "`kept`"][..],
    ] {
        let refused = repo.output(args);
        let stderr = String::from_utf8_lossy(&refused.stderr);
        assert_eq!(refused.status.code(), Some(3), "{stderr}");
        assert!(refused.stdout.is_empty());
        assert!(stderr.contains("cache capacity"), "{stderr}");
        assert!(stderr.contains("--place <a smaller directory>"), "{stderr}");
        assert!(!stderr.contains("cache gc"), "{stderr}");
        assert!(!stderr.contains("check that PATH"), "{stderr}");
    }
    assert_eq!(
        dctx_cli::host::load_ledger(&repo.cache())
            .expect("ledger")
            .admitted(),
        0
    );

    let mut child = repo
        .command(&["mcp"], &[])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("MCP");
    child.stdin.take().expect("stdin").write_all(
        b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{}}\n\
        {\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n\
        {\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{\"name\":\"find\",\"arguments\":{\"query\":\"kept\"}}}\n"
    ).expect("request");
    let output = child.wait_with_output().expect("MCP output");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).expect("JSON");
    assert!(text.contains("\"isError\":true"), "{text}");
    assert!(text.contains("cache capacity"), "{text}");
    assert!(text.contains("\\nhint: "), "{text}");
    assert!(text.contains("--place <a smaller directory>"), "{text}");
    assert!(!text.contains("cache gc"), "{text}");
}
