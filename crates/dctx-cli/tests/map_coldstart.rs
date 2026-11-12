// SPDX-License-Identifier: Apache-2.0
//! Fresh-process map reuse, live edits, and a release-only latency budget.

#![forbid(unsafe_code)]

mod common;

use std::fmt::Write as _;
use std::fs::{self, File, FileTimes};
use std::process::{Command, Output};
use std::time::{Duration, Instant, SystemTime};

struct Repository(tempfile::TempDir);

impl Repository {
    fn new(files: usize) -> Self {
        let root = tempfile::tempdir().expect("fixture");
        let repo = root.path().join("repo");
        fs::create_dir(&repo).expect("repo");
        // Exercise settled stamps without sleeping; edits below remain live writes.
        let settled = SystemTime::now() - Duration::from_secs(60);
        for file in 0..files {
            let dir = repo.join(format!("group{:02}", file / 100));
            fs::create_dir_all(&dir).expect("group");
            let path = dir.join(format!("unit{file:04}.rs"));
            let mut source = String::new();
            for symbol in 0..20 {
                writeln!(source, "/// Probe number {file}, helper {symbol}.").expect("comment");
                writeln!(
                    source,
                    "pub fn latency_probe_{file:04}_{symbol:02}() -> u32 {{ {} }}",
                    file + symbol
                )
                .expect("function");
            }
            fs::write(&path, source).expect("source");
            File::options()
                .write(true)
                .open(path)
                .expect("file")
                .set_times(FileTimes::new().set_modified(settled))
                .expect("settled mtime");
        }
        Self(root)
    }

    fn find(&self, name: &str) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_dctx-cli"));
        command
            .env_clear()
            .current_dir(self.0.path().join("repo"))
            .args(["map", "find", name, "--place", "."])
            .env("DCTX_AUTO_CLEAN", "0")
            .env("DCTX_REUSE_REPORT", "1")
            .env("DCTX_CACHE_DIR", self.0.path().join("cache"))
            .env("DCTX_PLUGIN_ROOT", self.0.path().join("plugins"));
        common::isolated_dirs(&mut command, self.0.path());
        let output = command.output().expect("fresh CLI process");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        output
    }
}

#[test]
fn one_shot_find_sees_edits_additions_and_deletions() {
    let repo = Repository::new(2);
    let before = repo.find("latency_probe_0000_00");
    assert!(String::from_utf8_lossy(&before.stdout).contains("s:group00/unit0000.rs#"));
    let warm = repo.find("latency_probe_0000_00");
    assert_eq!(before.stdout, warm.stdout);
    assert!(String::from_utf8_lossy(&warm.stderr).contains("generation reused"));

    // Same-length edit to a settled file; the next process must reject the old index.
    let path = repo.0.path().join("repo/group00/unit0000.rs");
    let source = fs::read_to_string(&path).expect("source");
    fs::write(
        &path,
        source.replace("latency_probe_0000_00", "changed_probe_0000_00"),
    )
    .expect("edit");
    let changed = repo.find("changed_probe_0000_00");
    assert!(String::from_utf8_lossy(&changed.stdout).contains("#changed_probe_0000_00"));
    assert!(String::from_utf8_lossy(&changed.stderr).contains("generation built"));
    assert!(
        !String::from_utf8_lossy(&repo.find("latency_probe_0000_00").stdout)
            .contains("#latency_probe_0000_00")
    );

    let added = repo.0.path().join("repo/added.rs");
    fs::write(&added, "pub fn added_probe() {}\n").expect("addition");
    assert!(String::from_utf8_lossy(&repo.find("added_probe").stdout).contains("s:added.rs#"));
    fs::remove_file(added).expect("deletion");
    assert!(!String::from_utf8_lossy(&repo.find("added_probe").stdout).contains("s:added.rs#"));
}

#[test]
fn release_one_shot_find_1500_files_under_300_ms() {
    // The latency budget holds only for a Linux release build; elsewhere the test is a no-op.
    if cfg!(debug_assertions) || !cfg!(target_os = "linux") {
        eprintln!("skipped: one-shot latency budget needs a Linux release build");
        return;
    }
    let repo = Repository::new(1500);
    let name = "latency_probe_0750_10";
    let cold = repo.find(name);
    assert!(String::from_utf8_lossy(&cold.stderr).contains("generation built"));
    let mut samples = Vec::new();
    for _ in 0..5 {
        let start = Instant::now();
        let warm = repo.find(name);
        let elapsed = start.elapsed();
        assert_eq!(warm.stdout, cold.stdout);
        assert!(String::from_utf8_lossy(&warm.stdout).contains("#latency_probe_0750_10"));
        assert!(String::from_utf8_lossy(&warm.stderr).contains("generation reused"));
        samples.push(elapsed);
    }
    samples.sort_unstable();
    assert!(
        samples[2] < Duration::from_millis(300),
        "one-shot find p50 exceeds 300 ms: {samples:?}"
    );
}
